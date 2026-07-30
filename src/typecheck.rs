//! Local type checking, name resolution, and typed AST metadata.
//!
//! # Local reasoning model (AI-friendly)
//!
//! Type and name resolution are intentionally **function-local**:
//! - Each function body has its own scope stack.
//! - Inference uses only local declarations, parameters, and expression context.
//! - There is no whole-program type environment that agents must index.
//!
//! Agents can therefore rewrite a single function safely when they keep:
//! 1. the function signature,
//! 2. the local `let` / `let mut` bindings in textual order,
//! 3. the types of called functions (from their signatures only).

use crate::ast::*;
use crate::diagnostics::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::span::{SourceFile, Span};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I64,
    F64,
    Bool,
    Unit,
    Str,
    CChar,
    CString,
    Named(String),
    Struct(String),
    Ref { mutable: bool, inner: Box<Type> },
}

impl Type {
    pub fn is_copy(&self) -> bool {
        matches!(
            self,
            Type::I64 | Type::F64 | Type::Bool | Type::Unit | Type::CChar | Type::Ref { .. }
        )
    }

    pub fn is_scalar_ffi(&self) -> bool {
        matches!(
            self,
            Type::I64 | Type::F64 | Type::Bool | Type::Unit | Type::CChar
        )
    }

    pub fn display(&self) -> String {
        match self {
            Type::I64 => "i64".into(),
            Type::F64 => "f64".into(),
            Type::Bool => "bool".into(),
            Type::Unit => "unit".into(),
            Type::Str => "str".into(),
            Type::CChar => "c_char".into(),
            Type::CString => "c_string".into(),
            Type::Named(n) | Type::Struct(n) => n.clone(),
            Type::Ref { mutable, inner } => {
                if *mutable {
                    format!("&mut {}", inner.display())
                } else {
                    format!("&{}", inner.display())
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    pub fields: Vec<(String, Type)>,
    pub is_c_repr: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnInfo {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub is_extern: bool,
    pub body: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypedProgram {
    pub structs: HashMap<String, StructInfo>,
    pub functions: HashMap<String, FnInfo>,
    pub order: Vec<String>,
    pub has_structs: bool,
    pub has_arenas: bool,
    pub has_refs: bool,
    pub has_strings: bool,
    pub has_advanced_ffi: bool,
}

impl TypedProgram {
    pub fn needs_borrow_check(&self) -> bool {
        self.has_refs
    }
}

pub fn typecheck(
    _file: &SourceFile,
    program: &Program,
    diagnostics: &mut Diagnostics,
) -> Option<TypedProgram> {
    let mut checker = Typechecker {
        diagnostics,
        structs: HashMap::new(),
        functions: HashMap::new(),
        order: Vec::new(),
        has_structs: false,
        has_arenas: false,
        has_refs: false,
        has_strings: false,
        has_advanced_ffi: false,
        scopes: Vec::new(),
        current_return: Type::Unit,
        in_unsafe: false,
        expr_types: HashMap::new(),
    };

    // Collect structs first.
    for item in &program.items {
        if let Item::Struct(def) = item {
            checker.define_struct(def);
        }
    }

    // Collect functions / externs.
    for item in &program.items {
        match item {
            Item::Function(f) => checker.define_function(f),
            Item::Extern(e) => checker.define_extern(e),
            Item::Struct(_) => {}
        }
    }

    // Typecheck bodies.
    let bodies: Vec<_> = program
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) => Some(f.clone()),
            _ => None,
        })
        .collect();

    for f in &bodies {
        checker.check_function(f);
    }

    if checker.diagnostics.has_errors() {
        return None;
    }

    // Rebuild function map with checked bodies (bodies already in FnInfo from define).
    Some(TypedProgram {
        structs: checker.structs,
        functions: checker.functions,
        order: checker.order,
        has_structs: checker.has_structs,
        has_arenas: checker.has_arenas,
        has_refs: checker.has_refs,
        has_strings: checker.has_strings,
        has_advanced_ffi: checker.has_advanced_ffi,
    })
}

struct Local {
    ty: Type,
    mutable: bool,
    moved: bool,
}

struct Typechecker<'a> {
    diagnostics: &'a mut Diagnostics,
    structs: HashMap<String, StructInfo>,
    functions: HashMap<String, FnInfo>,
    order: Vec<String>,
    has_structs: bool,
    has_arenas: bool,
    has_refs: bool,
    has_strings: bool,
    has_advanced_ffi: bool,
    scopes: Vec<HashMap<String, Local>>,
    current_return: Type,
    in_unsafe: bool,
    #[allow(dead_code)]
    expr_types: HashMap<usize, Type>,
}

impl<'a> Typechecker<'a> {
    fn define_struct(&mut self, def: &StructDef) {
        if self.structs.contains_key(&def.name) {
            self.diagnostics.error_at(
                def.span,
                format!("struct `{}` is already defined", def.name),
            );
            return;
        }
        self.has_structs = true;
        if def.is_c_repr {
            self.has_advanced_ffi = true;
        }
        let mut fields = Vec::new();
        for field in &def.fields {
            if let Some(ty) = self.resolve_type(&field.ty) {
                fields.push((field.name.clone(), ty));
            }
        }
        self.structs.insert(
            def.name.clone(),
            StructInfo {
                name: def.name.clone(),
                fields,
                is_c_repr: def.is_c_repr,
                span: def.span,
            },
        );
    }

    fn define_function(&mut self, f: &Function) {
        if self.functions.contains_key(&f.name) {
            self.diagnostics
                .error_at(f.span, format!("function `{}` is already defined", f.name));
            return;
        }
        let params = self.resolve_params(&f.params);
        let return_type = self.resolve_type(&f.return_type).unwrap_or(Type::Unit);
        self.order.push(f.name.clone());
        self.functions.insert(
            f.name.clone(),
            FnInfo {
                name: f.name.clone(),
                params,
                return_type,
                is_extern: false,
                body: Some(f.body.clone()),
                span: f.span,
            },
        );
    }

    fn define_extern(&mut self, e: &ExternFn) {
        if self.functions.contains_key(&e.name) {
            self.diagnostics
                .error_at(e.span, format!("function `{}` is already defined", e.name));
            return;
        }
        let params = self.resolve_params(&e.params);
        let return_type = self.resolve_type(&e.return_type).unwrap_or(Type::Unit);

        for (_, ty) in &params {
            self.check_ffi_type(ty, e.span);
        }
        self.check_ffi_type(&return_type, e.span);

        self.order.push(e.name.clone());
        self.functions.insert(
            e.name.clone(),
            FnInfo {
                name: e.name.clone(),
                params,
                return_type,
                is_extern: true,
                body: None,
                span: e.span,
            },
        );
    }

    fn check_ffi_type(&mut self, ty: &Type, span: Span) {
        match ty {
            Type::I64 | Type::F64 | Type::Bool | Type::Unit | Type::CChar => {}
            Type::CString | Type::Str => {
                self.has_advanced_ffi = true;
                self.has_strings = true;
            }
            Type::Struct(name) => {
                if let Some(info) = self.structs.get(name) {
                    if info.is_c_repr {
                        self.has_advanced_ffi = true;
                    } else {
                        self.diagnostics.error_at(
                            span,
                            format!(
                                "FFI type `{name}` must be declared as `struct c {name}` for C layout"
                            ),
                        );
                    }
                }
            }
            Type::Ref { .. } => {
                self.diagnostics.error_at(
                    span,
                    "raw references are not allowed directly in extern signatures; use C types",
                );
            }
            Type::Named(name) => {
                self.diagnostics
                    .error_at(span, format!("unknown FFI type `{name}`"));
            }
        }
    }

    fn resolve_params(&mut self, params: &[Param]) -> Vec<(String, Type)> {
        let mut out = Vec::new();
        for p in params {
            if let Some(ty) = self.resolve_type(&p.ty) {
                out.push((p.name.clone(), ty));
            }
        }
        out
    }

    fn resolve_type(&mut self, ty: &TypeExpr) -> Option<Type> {
        match &ty.kind {
            TypeExprKind::Unit => Some(Type::Unit),
            TypeExprKind::Named(name) => match name.as_str() {
                "i64" => Some(Type::I64),
                "f64" => Some(Type::F64),
                "bool" => Some(Type::Bool),
                "unit" => Some(Type::Unit),
                "str" => {
                    self.has_strings = true;
                    Some(Type::Str)
                }
                "c_char" => {
                    self.has_advanced_ffi = true;
                    Some(Type::CChar)
                }
                "c_string" => {
                    self.has_advanced_ffi = true;
                    self.has_strings = true;
                    Some(Type::CString)
                }
                other => {
                    if self.structs.contains_key(other) {
                        self.has_structs = true;
                        Some(Type::Struct(other.to_string()))
                    } else {
                        self.diagnostics
                            .error_at(ty.span, format!("unknown type `{other}`"));
                        None
                    }
                }
            },
            TypeExprKind::Ref { mutable, inner } => {
                self.has_refs = true;
                let inner = self.resolve_type(inner)?;
                Some(Type::Ref {
                    mutable: *mutable,
                    inner: Box::new(inner),
                })
            }
        }
    }

    fn check_function(&mut self, f: &Function) {
        let info = self.functions.get(&f.name).cloned();
        let Some(info) = info else { return };
        self.current_return = info.return_type.clone();
        self.scopes.clear();
        self.push_scope();
        for (name, ty) in &info.params {
            self.declare(name, ty.clone(), false, f.span);
        }
        self.check_block(&f.body);
        self.pop_scope();
    }

    fn check_block(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        self.pop_scope();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                mutable,
                ty,
                value,
                span,
            } => {
                let value_ty = self.check_expr(value);
                let final_ty = if let Some(ann) = ty {
                    let ann_ty = self.resolve_type(ann).unwrap_or(value_ty.clone());
                    if ann_ty != value_ty && !self.can_coerce(&value_ty, &ann_ty) {
                        self.diagnostics.error_at(
                            *span,
                            format!(
                                "type mismatch: expected `{}`, found `{}`",
                                ann_ty.display(),
                                value_ty.display()
                            ),
                        );
                    }
                    ann_ty
                } else {
                    value_ty
                };
                // Move non-copy values out of the RHS name if applicable.
                self.maybe_move_expr(value);
                self.declare(name, final_ty, *mutable, *span);
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                let value_ty = self.check_expr(value);
                match &target.kind {
                    ExprKind::Name(name) => {
                        let info = self
                            .lookup(name)
                            .map(|l| (l.mutable, l.moved, l.ty.clone()));
                        if let Some((mutable, moved, ty)) = info {
                            if !mutable {
                                self.diagnostics.push(
                                    Diagnostic::error_at_code(
                                        *span,
                                        DiagnosticCode::E0302,
                                        format!("cannot assign to immutable variable `{name}`"),
                                    )
                                    .help(format!(
                                        "declare it as `let mut {name} = ...` if mutation is intended"
                                    )),
                                );
                            }
                            if moved {
                                self.diagnostics.push(
                                    Diagnostic::error_at_code(
                                        *span,
                                        DiagnosticCode::E0303,
                                        format!("variable `{name}` was moved"),
                                    )
                                    .help("use the value before it is moved, or clone/copy if the type allows"),
                                );
                            }
                            if ty != value_ty {
                                self.diagnostics.push(
                                    Diagnostic::error_at_code(
                                        *span,
                                        DiagnosticCode::E0301,
                                        format!(
                                            "type mismatch: expected `{}`, found `{}`",
                                            ty.display(),
                                            value_ty.display()
                                        ),
                                    )
                                    .help("change the assigned expression type, or cast with `as` when converting numbers"),
                                );
                            }
                        } else {
                            self.diagnostics.push(
                                Diagnostic::error_at_code(
                                    *span,
                                    DiagnosticCode::E0300,
                                    format!("undefined variable `{name}`"),
                                )
                                .help("declare it with `let` before use, or check spelling"),
                            );
                        }
                    }
                    ExprKind::Field { .. } | ExprKind::Deref { .. } | ExprKind::Index { .. } => {
                        let target_ty = self.check_expr(target);
                        if target_ty != value_ty {
                            self.diagnostics.error_at(
                                *span,
                                format!(
                                    "type mismatch: expected `{}`, found `{}`",
                                    target_ty.display(),
                                    value_ty.display()
                                ),
                            );
                        }
                    }
                    _ => {
                        self.diagnostics
                            .error_at(*span, "invalid assignment target");
                    }
                }
                self.maybe_move_expr(value);
            }
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::Return { value, span } => {
                let ty = if let Some(v) = value {
                    let t = self.check_expr(v);
                    self.maybe_move_expr(v);
                    t
                } else {
                    Type::Unit
                };
                if ty != self.current_return {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            *span,
                            DiagnosticCode::E0304,
                            format!(
                                "return type mismatch: expected `{}`, found `{}`",
                                self.current_return.display(),
                                ty.display()
                            ),
                        )
                        .help(
                            "return a value of the declared type, or change the function signature",
                        ),
                    );
                }
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                span,
            } => {
                let cond_ty = self.check_expr(cond);
                if cond_ty != Type::Bool {
                    self.diagnostics.error_at(
                        *span,
                        format!("condition must be `bool`, found `{}`", cond_ty.display()),
                    );
                }
                self.check_block(then_block);
                if let Some(e) = else_block {
                    self.check_block(e);
                }
            }
            Stmt::While { cond, body, span } => {
                let cond_ty = self.check_expr(cond);
                if cond_ty != Type::Bool {
                    self.diagnostics.error_at(
                        *span,
                        format!("condition must be `bool`, found `{}`", cond_ty.display()),
                    );
                }
                self.check_block(body);
            }
            Stmt::Arena { body, .. } => {
                self.has_arenas = true;
                self.check_block(body);
            }
            Stmt::Unsafe { body, .. } => {
                let prev = self.in_unsafe;
                self.in_unsafe = true;
                self.check_block(body);
                self.in_unsafe = prev;
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Type {
        self.check_expr_expected(expr, None)
    }

    fn check_expr_expected(&mut self, expr: &Expr, _expected: Option<&Type>) -> Type {
        let ty = match &expr.kind {
            ExprKind::Int(_) => Type::I64,
            ExprKind::Float(_) => Type::F64,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Str(_) => {
                self.has_strings = true;
                Type::Str
            }
            ExprKind::Name(name) if name == "()" => Type::Unit,
            ExprKind::Name(name) => {
                if let Some((moved, ty)) = self.lookup(name).map(|l| (l.moved, l.ty.clone())) {
                    if moved {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                expr.span,
                                DiagnosticCode::E0303,
                                format!("use of moved value `{name}`"),
                            )
                            .help("this value was moved earlier; use it before the move, or redesign ownership"),
                        );
                    }
                    ty
                } else if self.functions.contains_key(name) {
                    // Function as value not supported; only for error message.
                    self.diagnostics
                        .error_at(expr.span, format!("function `{name}` must be called"));
                    Type::Unit
                } else {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0300,
                            format!("undefined variable `{name}`"),
                        )
                        .help("declare it with `let` before use, or check spelling"),
                    );
                    Type::Unit
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                let t = self.check_expr(inner);
                match op {
                    UnaryOp::Neg => {
                        if matches!(t, Type::I64 | Type::F64) {
                            t
                        } else {
                            self.diagnostics
                                .error_at(expr.span, format!("cannot negate `{}`", t.display()));
                            Type::I64
                        }
                    }
                    UnaryOp::Not => {
                        if t != Type::Bool {
                            self.diagnostics.error_at(
                                expr.span,
                                format!("`not` requires `bool`, found `{}`", t.display()),
                            );
                        }
                        Type::Bool
                    }
                }
            }
            ExprKind::Binary { op, left, right } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                self.check_binary(*op, &lt, &rt, expr.span)
            }
            ExprKind::Call { callee, args } => {
                let fname = match &callee.kind {
                    ExprKind::Name(n) => n.clone(),
                    _ => {
                        self.diagnostics
                            .error_at(expr.span, "only direct function calls are supported");
                        return Type::Unit;
                    }
                };
                // Builtins
                if let Some(ty) = self.check_builtin(&fname, args, expr.span) {
                    return ty;
                }
                let Some(info) = self.functions.get(&fname).cloned() else {
                    self.diagnostics
                        .error_at(expr.span, format!("undefined function `{fname}`"));
                    return Type::Unit;
                };
                if info.is_extern && !self.in_unsafe {
                    // Allow top-level calls in Phase 1 for scalar FFI convenience,
                    // but warn that extern calls are unsafe.
                    // Plan says extern is unsafe by default — require unsafe for advanced FFI.
                    if self.has_advanced_types_in_fn(&info) {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                expr.span,
                                DiagnosticCode::E0500,
                                format!(
                                    "extern function `{fname}` must be called inside `unsafe`"
                                ),
                            )
                            .note("Kroa cannot verify C library behavior across the FFI boundary")
                            .help("wrap the call in an `unsafe:` block, preferably inside a small safe wrapper"),
                        );
                    }
                }
                if args.len() != info.params.len() {
                    self.diagnostics.error_at(
                        expr.span,
                        format!(
                            "function `{fname}` expects {} argument(s), found {}",
                            info.params.len(),
                            args.len()
                        ),
                    );
                }
                for (arg, (_, pty)) in args.iter().zip(info.params.iter()) {
                    let at = self.check_expr_expected(arg, Some(pty));
                    if at != *pty && !self.can_coerce(&at, pty) {
                        self.diagnostics.error_at(
                            arg.span,
                            format!(
                                "argument type mismatch: expected `{}`, found `{}`",
                                pty.display(),
                                at.display()
                            ),
                        );
                    }
                    self.maybe_move_expr(arg);
                }
                info.return_type
            }
            ExprKind::Field { base, field } => {
                let base_ty = self.check_expr(base);
                match base_ty {
                    Type::Struct(name) => {
                        if let Some(info) = self.structs.get(&name) {
                            if let Some((_, ty)) = info.fields.iter().find(|(n, _)| n == field) {
                                ty.clone()
                            } else {
                                self.diagnostics.error_at(
                                    expr.span,
                                    format!("struct `{name}` has no field `{field}`"),
                                );
                                Type::Unit
                            }
                        } else {
                            Type::Unit
                        }
                    }
                    Type::Ref { inner, .. } => {
                        if let Type::Struct(name) = inner.as_ref() {
                            if let Some(info) = self.structs.get(name) {
                                if let Some((_, ty)) = info.fields.iter().find(|(n, _)| n == field)
                                {
                                    return ty.clone();
                                }
                            }
                        }
                        self.diagnostics
                            .error_at(expr.span, "field access on non-struct type");
                        Type::Unit
                    }
                    _ => {
                        self.diagnostics
                            .error_at(expr.span, "field access on non-struct type");
                        Type::Unit
                    }
                }
            }
            ExprKind::StructLit { name, fields } => {
                self.has_structs = true;
                let Some(info) = self.structs.get(name).cloned() else {
                    self.diagnostics
                        .error_at(expr.span, format!("unknown struct `{name}`"));
                    return Type::Unit;
                };
                for (fname, fexpr) in fields {
                    let ft = self.check_expr(fexpr);
                    match info.fields.iter().find(|(n, _)| n == fname) {
                        Some((_, expected))
                            if expected == &ft || self.can_coerce(&ft, expected) => {}
                        Some((_, expected)) => self.diagnostics.error_at(
                            fexpr.span,
                            format!(
                                "field `{fname}` expected `{}`, found `{}`",
                                expected.display(),
                                ft.display()
                            ),
                        ),
                        None => self.diagnostics.error_at(
                            fexpr.span,
                            format!("struct `{name}` has no field `{fname}`"),
                        ),
                    }
                }
                Type::Struct(name.clone())
            }
            ExprKind::Cast { expr: inner, ty } => {
                let from = self.check_expr(inner);
                let to = self.resolve_type(ty).unwrap_or(from.clone());
                if !self.can_cast(&from, &to) {
                    self.diagnostics.error_at(
                        expr.span,
                        format!("cannot cast `{}` to `{}`", from.display(), to.display()),
                    );
                }
                to
            }
            ExprKind::Ref {
                mutable,
                expr: inner,
            } => {
                self.has_refs = true;
                if *mutable {
                    self.check_mut_place(inner, expr.span);
                }
                let inner_ty = self.check_expr(inner);
                Type::Ref {
                    mutable: *mutable,
                    inner: Box::new(inner_ty),
                }
            }
            ExprKind::Deref { expr: inner } => {
                self.has_refs = true;
                let t = self.check_expr(inner);
                match t {
                    Type::Ref { inner, .. } => *inner,
                    Type::CString => Type::CChar,
                    _ => {
                        self.diagnostics
                            .error_at(expr.span, format!("cannot dereference `{}`", t.display()));
                        Type::Unit
                    }
                }
            }
            ExprKind::Index { .. } => {
                self.diagnostics
                    .error_at(expr.span, "indexing is not available in Alpha-1.0.0");
                Type::Unit
            }
        };
        ty
    }

    fn check_mut_place(&mut self, place: &Expr, span: Span) {
        match &place.kind {
            ExprKind::Name(name) => {
                if let Some(local) = self.lookup(name) {
                    if !local.mutable {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                span,
                                DiagnosticCode::E0302,
                                format!(
                                    "cannot borrow `{name}` as mutable because it is immutable"
                                ),
                            )
                            .help(format!(
                                "declare it as `let mut {name} = ...` if mutation is intended"
                            )),
                        );
                    }
                }
            }
            ExprKind::Index { base, .. } => {
                self.check_mut_place(base, span);
            }
            ExprKind::Deref { expr } => {
                let t = self.check_expr(expr);
                if let Type::Ref { mutable: false, .. } = t {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            span,
                            DiagnosticCode::E0302,
                            "cannot create `&mut` through a shared reference",
                        )
                        .help("borrow the original place as `&mut` instead"),
                    );
                }
            }
            _ => {}
        }
    }

    fn check_builtin(&mut self, name: &str, args: &[Expr], span: Span) -> Option<Type> {
        match name {
            "print_i64" => {
                self.expect_arity(name, args, 1, span);
                if let Some(a) = args.first() {
                    let t = self.check_expr(a);
                    if t != Type::I64 {
                        self.diagnostics.error_at(
                            a.span,
                            format!("`print_i64` expects `i64`, found `{}`", t.display()),
                        );
                    }
                }
                Some(Type::Unit)
            }
            "print_f64" => {
                self.expect_arity(name, args, 1, span);
                if let Some(a) = args.first() {
                    let t = self.check_expr(a);
                    if t != Type::F64 {
                        self.diagnostics.error_at(
                            a.span,
                            format!("`print_f64` expects `f64`, found `{}`", t.display()),
                        );
                    }
                }
                Some(Type::Unit)
            }
            "print_bool" => {
                self.expect_arity(name, args, 1, span);
                if let Some(a) = args.first() {
                    let t = self.check_expr(a);
                    if t != Type::Bool {
                        self.diagnostics.error_at(
                            a.span,
                            format!("`print_bool` expects `bool`, found `{}`", t.display()),
                        );
                    }
                }
                Some(Type::Unit)
            }
            "print_str" => {
                self.has_strings = true;
                self.expect_arity(name, args, 1, span);
                if let Some(a) = args.first() {
                    let t = self.check_expr(a);
                    if t != Type::Str {
                        self.diagnostics.error_at(
                            a.span,
                            format!("`print_str` expects `str`, found `{}`", t.display()),
                        );
                    }
                }
                Some(Type::Unit)
            }
            "to_c_string" => {
                self.has_strings = true;
                self.has_advanced_ffi = true;
                self.expect_arity(name, args, 1, span);
                if let Some(a) = args.first() {
                    let t = self.check_expr(a);
                    if t != Type::Str {
                        self.diagnostics.error_at(
                            a.span,
                            format!("`to_c_string` expects `str`, found `{}`", t.display()),
                        );
                    }
                }
                Some(Type::CString)
            }
            _ => None,
        }
    }

    fn expect_arity(&mut self, name: &str, args: &[Expr], n: usize, span: Span) {
        if args.len() != n {
            self.diagnostics.error_at(
                span,
                format!("`{name}` expects {n} argument(s), found {}", args.len()),
            );
        }
    }

    fn has_advanced_types_in_fn(&self, info: &FnInfo) -> bool {
        info.params
            .iter()
            .any(|(_, t)| matches!(t, Type::Str | Type::CString | Type::Struct(_)))
            || matches!(
                info.return_type,
                Type::Str | Type::CString | Type::Struct(_)
            )
    }

    fn check_binary(&mut self, op: BinaryOp, lt: &Type, rt: &Type, span: Span) -> Type {
        use BinaryOp::*;
        match op {
            Add | Sub | Mul | Div | Rem => {
                if lt == rt && matches!(lt, Type::I64 | Type::F64) {
                    lt.clone()
                } else {
                    self.diagnostics.error_at(
                        span,
                        format!(
                            "operator requires matching numeric types, found `{}` and `{}`",
                            lt.display(),
                            rt.display()
                        ),
                    );
                    Type::I64
                }
            }
            Eq | NotEq => {
                if lt == rt {
                    Type::Bool
                } else {
                    self.diagnostics.error_at(
                        span,
                        format!("cannot compare `{}` and `{}`", lt.display(), rt.display()),
                    );
                    Type::Bool
                }
            }
            Lt | LtEq | Gt | GtEq => {
                if lt == rt && matches!(lt, Type::I64 | Type::F64) {
                    Type::Bool
                } else {
                    self.diagnostics.error_at(
                        span,
                        format!(
                            "comparison requires matching numeric types, found `{}` and `{}`",
                            lt.display(),
                            rt.display()
                        ),
                    );
                    Type::Bool
                }
            }
            And | Or => {
                if lt == &Type::Bool && rt == &Type::Bool {
                    Type::Bool
                } else {
                    self.diagnostics.error_at(
                        span,
                        format!(
                            "logical operator requires `bool`, found `{}` and `{}`",
                            lt.display(),
                            rt.display()
                        ),
                    );
                    Type::Bool
                }
            }
        }
    }

    fn can_coerce(&self, from: &Type, to: &Type) -> bool {
        from == to
    }

    fn can_cast(&self, from: &Type, to: &Type) -> bool {
        matches!(
            (from, to),
            (Type::I64, Type::F64)
                | (Type::F64, Type::I64)
                | (Type::I64, Type::I64)
                | (Type::F64, Type::F64)
                | (Type::Bool, Type::I64)
                | (Type::Str, Type::CString)
        ) || from == to
    }

    fn maybe_move_expr(&mut self, expr: &Expr) {
        if let ExprKind::Name(name) = &expr.kind {
            if let Some(local) = self.lookup_mut(name) {
                if !local.ty.is_copy() {
                    local.moved = true;
                }
            }
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, ty: Type, mutable: bool, span: Span) {
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains_key(name) {
                self.diagnostics.error_at(
                    span,
                    format!("variable `{name}` is already declared in this scope"),
                );
                return;
            }
            scope.insert(
                name.to_string(),
                Local {
                    ty,
                    mutable,
                    moved: false,
                },
            );
        }
    }

    fn lookup(&self, name: &str) -> Option<&Local> {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.get(name) {
                return Some(local);
            }
        }
        None
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Local> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                return scope.get_mut(name);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;
    use crate::span::SourceFile;

    fn check(src: &str) -> TypedProgram {
        let file = SourceFile::new("t.kroa".into(), src.into());
        let mut d = Diagnostics::new();
        let tokens = lexer::lex(&file, &mut d).unwrap();
        let prog = parser::parse(&file, &tokens, &mut d).unwrap();
        typecheck(&file, &prog, &mut d).unwrap_or_else(|| panic!("{}", d.render_all(&file)))
    }

    #[test]
    fn checks_simple_main() {
        let t = check("fn main() -> i64:\n    return 1\n");
        assert!(t.functions.contains_key("main"));
    }
}
