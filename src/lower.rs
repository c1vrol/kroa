//! Lower typed AST into Kroa IR.

use crate::ast::{BinaryOp, Block, Expr, ExprKind, Stmt, TypeExprKind, UnaryOp};
use crate::diagnostics::Diagnostics;
use crate::ir::{
    BasicBlock, BinOp, BlockId, ExternFn as IrExtern, Function as IrFunction, Inst, InstKind,
    Module, Terminator, UnOp, ValueId,
};
use crate::span::Span;
use crate::typecheck::{Type, TypedProgram};
use std::collections::HashMap;

pub fn lower(typed: &TypedProgram, diagnostics: &mut Diagnostics) -> Module {
    let mut externs = Vec::new();
    let mut functions = Vec::new();
    let mut deferred_errors: Vec<(Span, String)> = Vec::new();

    for name in &typed.order {
        let Some(info) = typed.functions.get(name) else {
            continue;
        };
        if info.is_extern {
            externs.push(IrExtern {
                name: info.name.clone(),
                params: info.params.iter().map(|(_, t)| t.clone()).collect(),
                return_type: info.return_type.clone(),
            });
            continue;
        }
        let Some(body) = &info.body else { continue };
        let mut builder = FunctionBuilder::new(
            info.name.clone(),
            info.params.clone(),
            info.return_type.clone(),
            info.span,
            typed,
            diagnostics,
        );
        builder.lower_block_stmts(body);
        if !builder.current_terminated() {
            if info.return_type == Type::Unit {
                builder.set_term(Terminator::Return(None));
            } else {
                deferred_errors.push((
                    info.span,
                    format!(
                        "function `{}` must return a value of type `{}`",
                        info.name,
                        info.return_type.display()
                    ),
                ));
                builder.set_term(Terminator::Unreachable);
            }
        }
        functions.push(builder.finish());
    }

    for (span, msg) in deferred_errors {
        diagnostics.error_at(span, msg);
    }

    Module {
        structs: typed.structs.clone(),
        functions,
        externs,
    }
}

#[derive(Clone)]
struct LocalSlot {
    ptr: ValueId,
    ty: Type,
    #[allow(dead_code)]
    mutable: bool,
}

struct FunctionBuilder<'a> {
    name: String,
    params: Vec<(String, Type, ValueId)>,
    return_type: Type,
    span: Span,
    typed: &'a TypedProgram,
    diagnostics: &'a mut Diagnostics,
    blocks: Vec<BasicBlock>,
    current: usize,
    next_value: u32,
    next_block: u32,
    scopes: Vec<HashMap<String, LocalSlot>>,
    arena_depth: u32,
    /// True when control flow has fully diverged (all paths returned).
    sealed: bool,
}

impl<'a> FunctionBuilder<'a> {
    fn new(
        name: String,
        params_in: Vec<(String, Type)>,
        return_type: Type,
        span: Span,
        typed: &'a TypedProgram,
        diagnostics: &'a mut Diagnostics,
    ) -> Self {
        let mut next_value = 0u32;
        let mut param_vals = Vec::new();
        for (n, t) in &params_in {
            let id = ValueId(next_value);
            next_value += 1;
            param_vals.push((n.clone(), t.clone(), id));
        }

        let entry = BasicBlock {
            id: BlockId(0),
            name: "entry".into(),
            insts: Vec::new(),
            terminator: Terminator::Unreachable,
        };

        let mut builder = Self {
            name,
            params: param_vals.clone(),
            return_type,
            span,
            typed,
            diagnostics,
            blocks: vec![entry],
            current: 0,
            next_value,
            next_block: 1,
            scopes: vec![HashMap::new()],
            arena_depth: 0,
            sealed: false,
        };

        // Allocate slots for parameters and store incoming values.
        for (n, t, vid) in param_vals {
            let ptr = builder.alloca(t.clone(), span);
            builder.push_store(ptr, vid, span);
            builder.scopes[0].insert(
                n,
                LocalSlot {
                    ptr,
                    ty: t,
                    mutable: false,
                },
            );
        }
        builder
    }

    fn finish(self) -> IrFunction {
        IrFunction {
            name: self.name,
            params: self.params,
            return_type: self.return_type,
            blocks: self.blocks,
            next_value: self.next_value,
            span: self.span,
        }
    }

    fn lower_block_stmts(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.stmts {
            if self.current_terminated() {
                break;
            }
            self.lower_stmt(stmt);
        }
        // Exit any nested arenas opened in this block are handled per Arena stmt.
        self.pop_scope();
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        if self.sealed {
            return;
        }
        match stmt {
            Stmt::Let {
                name,
                mutable,
                value,
                span,
                ..
            } => {
                let (vid, ty) = self.lower_expr(value);
                let ptr = self.alloca(ty.clone(), *span);
                let moved = if ty.is_copy() {
                    vid
                } else {
                    self.push_valued(ty.clone(), InstKind::Move { value: vid }, *span)
                };
                self.push_store(ptr, moved, *span);
                self.declare(
                    name,
                    LocalSlot {
                        ptr,
                        ty,
                        mutable: *mutable,
                    },
                );
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                let (vid, _) = self.lower_expr(value);
                match &target.kind {
                    ExprKind::Name(name) => {
                        if let Some(local) = self.lookup(name).cloned() {
                            self.push_store(local.ptr, vid, *span);
                        }
                    }
                    ExprKind::Field { base, field } => {
                        // load struct, insert field, store back if base is name
                        if let ExprKind::Name(bname) = &base.kind {
                            if let Some(local) = self.lookup(bname).cloned() {
                                let agg = self.push_valued(
                                    local.ty.clone(),
                                    InstKind::Load { ptr: local.ptr },
                                    *span,
                                );
                                let idx = self.field_index(&local.ty, field).unwrap_or(0);
                                let new_agg = self.push_valued(
                                    local.ty.clone(),
                                    InstKind::InsertField {
                                        agg,
                                        index: idx,
                                        value: vid,
                                    },
                                    *span,
                                );
                                self.push_store(local.ptr, new_agg, *span);
                            }
                        } else {
                            self.diagnostics.error_at(
                                *span,
                                "assignment to nested fields is limited in this phase",
                            );
                        }
                    }
                    ExprKind::Deref { expr } => {
                        let (ptr, _) = self.lower_expr(expr);
                        self.push_store(ptr, vid, *span);
                    }
                    _ => {
                        self.diagnostics
                            .error_at(*span, "unsupported assignment target in lowering");
                    }
                }
            }
            Stmt::Expr { expr, .. } => {
                let _ = self.lower_expr(expr);
            }
            Stmt::Return { value, span } => {
                // Exit all open arenas before returning.
                for _ in 0..self.arena_depth {
                    self.push_inst(Type::Unit, InstKind::ArenaExit, *span);
                }
                let ret = if let Some(v) = value {
                    let (vid, _) = self.lower_expr(v);
                    Some(vid)
                } else {
                    None
                };
                self.set_term(Terminator::Return(ret));
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                span,
            } => {
                let (c, _) = self.lower_expr(cond);
                let then_id = self.new_block("then");
                let else_id = self.new_block("else");
                self.set_term(Terminator::Branch {
                    cond: c,
                    then_block: then_id,
                    else_block: else_id,
                });

                self.switch_to(then_id);
                self.lower_block_stmts(then_block);
                let then_term = self.current_terminated();

                self.switch_to(else_id);
                if let Some(e) = else_block {
                    self.lower_block_stmts(e);
                }
                let else_term = self.current_terminated();

                if then_term && else_term {
                    self.sealed = true;
                } else {
                    let join_id = self.new_block("join");
                    if !then_term {
                        self.switch_to(then_id);
                        self.set_term(Terminator::Jump(join_id));
                    }
                    if !else_term {
                        self.switch_to(else_id);
                        self.set_term(Terminator::Jump(join_id));
                    }
                    self.switch_to(join_id);
                }
                let _ = span;
            }
            Stmt::While { cond, body, span } => {
                let header = self.new_block("loop.header");
                let body_id = self.new_block("loop.body");
                let exit = self.new_block("loop.exit");
                self.set_term(Terminator::Jump(header));

                self.switch_to(header);
                let (c, _) = self.lower_expr(cond);
                self.set_term(Terminator::Branch {
                    cond: c,
                    then_block: body_id,
                    else_block: exit,
                });

                self.switch_to(body_id);
                self.lower_block_stmts(body);
                if !self.current_terminated() {
                    self.set_term(Terminator::Jump(header));
                }

                self.switch_to(exit);
                let _ = span;
            }
            Stmt::Arena { body, span } => {
                self.push_inst(Type::Unit, InstKind::ArenaEnter, *span);
                self.arena_depth += 1;
                self.lower_block_stmts(body);
                if !self.current_terminated() {
                    self.push_inst(Type::Unit, InstKind::ArenaExit, *span);
                    self.arena_depth -= 1;
                }
            }
            Stmt::Unsafe { body, .. } => {
                self.lower_block_stmts(body);
            }
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> (ValueId, Type) {
        match &expr.kind {
            ExprKind::Int(v) => {
                let id = self.push_valued(Type::I64, InstKind::ConstI64(*v), expr.span);
                (id, Type::I64)
            }
            ExprKind::Float(v) => {
                let id = self.push_valued(Type::F64, InstKind::ConstF64(*v), expr.span);
                (id, Type::F64)
            }
            ExprKind::Bool(v) => {
                let id = self.push_valued(Type::Bool, InstKind::ConstBool(*v), expr.span);
                (id, Type::Bool)
            }
            ExprKind::Str(s) => {
                let id = self.push_valued(Type::Str, InstKind::ConstStr(s.clone()), expr.span);
                (id, Type::Str)
            }
            ExprKind::Name(name) if name == "()" => {
                let id = self.push_valued(Type::Unit, InstKind::Nop, expr.span);
                (id, Type::Unit)
            }
            ExprKind::Name(name) => {
                let local = self.lookup(name).cloned().expect("resolved name");
                let id = self.push_valued(
                    local.ty.clone(),
                    InstKind::Load { ptr: local.ptr },
                    expr.span,
                );
                (id, local.ty)
            }
            ExprKind::Unary { op, expr: inner } => {
                let (v, ty) = self.lower_expr(inner);
                let uop = match op {
                    UnaryOp::Neg => UnOp::Neg,
                    UnaryOp::Not => UnOp::Not,
                };
                let out_ty = if matches!(op, UnaryOp::Not) {
                    Type::Bool
                } else {
                    ty
                };
                let id = self.push_valued(
                    out_ty.clone(),
                    InstKind::Unary { op: uop, value: v },
                    expr.span,
                );
                (id, out_ty)
            }
            ExprKind::Binary { op, left, right } => {
                let (l, lt) = self.lower_expr(left);
                let (r, _) = self.lower_expr(right);
                let bop = match op {
                    BinaryOp::Add => BinOp::Add,
                    BinaryOp::Sub => BinOp::Sub,
                    BinaryOp::Mul => BinOp::Mul,
                    BinaryOp::Div => BinOp::Div,
                    BinaryOp::Rem => BinOp::Rem,
                    BinaryOp::Eq => BinOp::Eq,
                    BinaryOp::NotEq => BinOp::NotEq,
                    BinaryOp::Lt => BinOp::Lt,
                    BinaryOp::LtEq => BinOp::LtEq,
                    BinaryOp::Gt => BinOp::Gt,
                    BinaryOp::GtEq => BinOp::GtEq,
                    BinaryOp::And => BinOp::And,
                    BinaryOp::Or => BinOp::Or,
                };
                let out_ty = match op {
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                    | BinaryOp::And
                    | BinaryOp::Or => Type::Bool,
                    _ => lt,
                };
                let id = self.push_valued(
                    out_ty.clone(),
                    InstKind::Binary {
                        op: bop,
                        left: l,
                        right: r,
                    },
                    expr.span,
                );
                (id, out_ty)
            }
            ExprKind::Call { callee, args } => {
                let fname = match &callee.kind {
                    ExprKind::Name(n) => n.clone(),
                    _ => unreachable!(),
                };
                let mut arg_ids = Vec::new();
                for a in args {
                    arg_ids.push(self.lower_expr(a).0);
                }
                if fname == "to_c_string" {
                    let id = self.push_valued(
                        Type::CString,
                        InstKind::ToCString { value: arg_ids[0] },
                        expr.span,
                    );
                    return (id, Type::CString);
                }
                let (ret_ty, is_extern) = if let Some(info) = self.typed.functions.get(&fname) {
                    (info.return_type.clone(), info.is_extern)
                } else {
                    // builtins
                    (Type::Unit, true)
                };
                let id = self.push_valued(
                    ret_ty.clone(),
                    InstKind::Call {
                        name: fname,
                        args: arg_ids,
                        is_extern,
                    },
                    expr.span,
                );
                (id, ret_ty)
            }
            ExprKind::Field { base, field } => {
                let (agg, ty) = self.lower_expr(base);
                let idx = self.field_index(&ty, field).unwrap_or(0);
                let field_ty = self.field_type(&ty, field).unwrap_or(Type::Unit);
                let id = self.push_valued(
                    field_ty.clone(),
                    InstKind::ExtractField { agg, index: idx },
                    expr.span,
                );
                (id, field_ty)
            }
            ExprKind::StructLit { name, fields } => {
                let info = self.typed.structs.get(name).cloned().unwrap();
                let mut vals = Vec::new();
                for (fname, _) in &info.fields {
                    let fexpr = fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .map(|(_, e)| e)
                        .expect("field present");
                    vals.push(self.lower_expr(fexpr).0);
                }
                let ty = Type::Struct(name.clone());
                let id = self.push_valued(
                    ty.clone(),
                    InstKind::StructAgg {
                        name: name.clone(),
                        fields: vals,
                    },
                    expr.span,
                );
                (id, ty)
            }
            ExprKind::Cast { expr: inner, ty } => {
                let (v, _) = self.lower_expr(inner);
                let to = self.resolve_type_expr(ty);
                if to == Type::CString {
                    let id =
                        self.push_valued(to.clone(), InstKind::ToCString { value: v }, expr.span);
                    return (id, to);
                }
                let id = self.push_valued(
                    to.clone(),
                    InstKind::Cast {
                        value: v,
                        to: to.clone(),
                    },
                    expr.span,
                );
                (id, to)
            }
            ExprKind::Ref {
                mutable,
                expr: inner,
            } => self.lower_ref(*mutable, inner, expr.span),
            ExprKind::Deref { expr: inner } => {
                let (ptr, pty) = self.lower_expr(inner);
                let inner_ty = match pty {
                    Type::Ref { inner, .. } => *inner,
                    other => other,
                };
                let id = self.push_valued(inner_ty.clone(), InstKind::Deref { ptr }, expr.span);
                (id, inner_ty)
            }
            ExprKind::Index { .. } => {
                self.diagnostics
                    .error_at(expr.span, "indexing is not available in Alpha-1.0.0");
                let id = self.push_valued(Type::Unit, InstKind::Nop, expr.span);
                (id, Type::Unit)
            }
        }
    }

    fn resolve_type_expr(&self, ty: &crate::ast::TypeExpr) -> Type {
        match &ty.kind {
            TypeExprKind::Named(n) => match n.as_str() {
                "i64" => Type::I64,
                "f64" => Type::F64,
                "bool" => Type::Bool,
                "str" => Type::Str,
                "c_string" => Type::CString,
                "c_char" => Type::CChar,
                other => Type::Struct(other.to_string()),
            },
            TypeExprKind::Unit => Type::Unit,
            TypeExprKind::Ref { mutable, inner } => Type::Ref {
                mutable: *mutable,
                inner: Box::new(self.resolve_type_expr(inner)),
            },
        }
    }

    fn lower_ref(&mut self, mutable: bool, inner: &Expr, span: Span) -> (ValueId, Type) {
        match &inner.kind {
            ExprKind::Name(name) => {
                let local = self.lookup(name).cloned().expect("name");
                let ty = Type::Ref {
                    mutable,
                    inner: Box::new(local.ty.clone()),
                };
                let id = self.push_valued(
                    ty.clone(),
                    InstKind::Ref {
                        mutable,
                        place: local.ptr,
                    },
                    span,
                );
                (id, ty)
            }
            _ => {
                let (v, ty) = self.lower_expr(inner);
                let ptr = self.alloca(ty.clone(), span);
                self.push_store(ptr, v, span);
                let rty = Type::Ref {
                    mutable,
                    inner: Box::new(ty),
                };
                let id = self.push_valued(
                    rty.clone(),
                    InstKind::Ref {
                        mutable,
                        place: ptr,
                    },
                    span,
                );
                (id, rty)
            }
        }
    }

    fn field_index(&self, ty: &Type, field: &str) -> Option<usize> {
        let name = match ty {
            Type::Struct(n) => n,
            Type::Ref { inner, .. } => match inner.as_ref() {
                Type::Struct(n) => n,
                _ => return None,
            },
            _ => return None,
        };
        self.typed
            .structs
            .get(name)?
            .fields
            .iter()
            .position(|(n, _)| n == field)
    }

    fn field_type(&self, ty: &Type, field: &str) -> Option<Type> {
        let name = match ty {
            Type::Struct(n) => n,
            Type::Ref { inner, .. } => match inner.as_ref() {
                Type::Struct(n) => n,
                _ => return None,
            },
            _ => return None,
        };
        self.typed
            .structs
            .get(name)?
            .fields
            .iter()
            .find(|(n, _)| n == field)
            .map(|(_, t)| t.clone())
    }

    fn alloca(&mut self, ty: Type, span: Span) -> ValueId {
        // Alloca "returns" a pointer-like value; we type it as Ref for simplicity in IR typing,
        // but store the element type in a parallel convention: ty is element type, id is ptr.
        self.push_valued(ty, InstKind::Alloca, span)
    }

    fn push_store(&mut self, ptr: ValueId, value: ValueId, span: Span) {
        self.push_inst(Type::Unit, InstKind::Store { ptr, value }, span);
    }

    fn push_valued(&mut self, ty: Type, kind: InstKind, span: Span) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.blocks[self.current].insts.push(Inst {
            id: Some(id),
            ty,
            kind,
            span,
        });
        id
    }

    fn push_inst(&mut self, ty: Type, kind: InstKind, span: Span) {
        self.blocks[self.current].insts.push(Inst {
            id: None,
            ty,
            kind,
            span,
        });
    }

    fn new_block(&mut self, label: &str) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        self.blocks.push(BasicBlock {
            id,
            name: format!("{label}_{}", id.0),
            insts: Vec::new(),
            terminator: Terminator::Unreachable,
        });
        id
    }

    fn switch_to(&mut self, id: BlockId) {
        self.current = self
            .blocks
            .iter()
            .position(|b| b.id == id)
            .expect("block exists");
    }

    fn set_term(&mut self, term: Terminator) {
        self.blocks[self.current].terminator = term;
    }

    fn current_terminated(&self) -> bool {
        self.sealed
            || !matches!(
                self.blocks[self.current].terminator,
                Terminator::Unreachable
            )
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, slot: LocalSlot) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), slot);
    }

    fn lookup(&self, name: &str) -> Option<&LocalSlot> {
        for scope in self.scopes.iter().rev() {
            if let Some(s) = scope.get(name) {
                return Some(s);
            }
        }
        None
    }
}
