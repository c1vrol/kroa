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
    Enum(String),
    Ref { mutable: bool, inner: Box<Type> },
    Array { element: Box<Type>, len: usize },
    Slice(Box<Type>),
}

impl Type {
    pub fn is_copy(&self) -> bool {
        match self {
            Type::I64 | Type::F64 | Type::Bool | Type::Unit | Type::CChar => true,
            // Shared references may be copied. Mutable references are exclusive.
            Type::Ref { mutable, .. } => !*mutable,
            Type::Array { element, .. } => element.is_copy(),
            Type::Slice(_) => false,
            Type::Str | Type::CString | Type::Named(_) | Type::Struct(_) | Type::Enum(_) => false,
        }
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
            Type::Named(n) | Type::Struct(n) | Type::Enum(n) => n.clone(),
            Type::Ref { mutable, inner } => {
                if *mutable {
                    format!("&mut {}", inner.display())
                } else {
                    format!("&{}", inner.display())
                }
            }
            Type::Array { element, len } => format!("[{}; {len}]", element.display()),
            Type::Slice(element) => format!("[{}]", element.display()),
        }
    }

    pub fn element_type(&self) -> Option<&Type> {
        match self {
            Type::Array { element, .. } | Type::Slice(element) => Some(element),
            Type::Ref { inner, .. } => inner.element_type(),
            _ => None,
        }
    }

    pub fn array_len(&self) -> Option<usize> {
        match self {
            Type::Array { len, .. } => Some(*len),
            Type::Ref { inner, .. } => inner.array_len(),
            _ => None,
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
pub struct EnumInfo {
    pub name: String,
    pub variants: Vec<VariantInfo>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: String,
    pub tag: u32,
    pub fields: Vec<(String, Type)>,
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
    pub enums: HashMap<String, EnumInfo>,
    pub functions: HashMap<String, FnInfo>,
    pub order: Vec<String>,
    pub has_structs: bool,
    pub has_enums: bool,
    pub has_arenas: bool,
    pub has_refs: bool,
    pub has_strings: bool,
    pub has_advanced_ffi: bool,
    pub has_arrays: bool,
}

impl TypedProgram {
    pub fn needs_borrow_check(&self) -> bool {
        // Arena-backed pointer-like values (for example `c_string`) also need
        // escape analysis even when the source program contains no `&T`.
        self.has_refs || self.has_arenas
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
        enums: HashMap::new(),
        functions: HashMap::new(),
        order: Vec::new(),
        has_structs: false,
        has_enums: false,
        has_arenas: false,
        has_refs: false,
        has_strings: false,
        has_advanced_ffi: false,
        has_arrays: false,
        scopes: Vec::new(),
        current_return: Type::Unit,
        in_unsafe: false,
        arena_depth: 0,
        expr_types: HashMap::new(),
    };

    // Register type names first (structs + enums) so fields can refer to later types.
    for item in &program.items {
        match item {
            Item::Struct(def) => checker.register_struct_name(def),
            Item::Enum(def) => checker.register_enum_name(def),
            _ => {}
        }
    }
    for item in &program.items {
        match item {
            Item::Struct(def) => checker.define_struct_fields(def),
            Item::Enum(def) => checker.define_enum_variants(def),
            _ => {}
        }
    }

    // Collect functions / externs.
    for item in &program.items {
        match item {
            Item::Function(f) => checker.define_function(f),
            Item::Extern(e) => checker.define_extern(e),
            Item::Struct(_) | Item::Enum(_) => {}
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

    Some(TypedProgram {
        structs: checker.structs,
        enums: checker.enums,
        functions: checker.functions,
        order: checker.order,
        has_structs: checker.has_structs,
        has_enums: checker.has_enums,
        has_arenas: checker.has_arenas,
        has_refs: checker.has_refs,
        has_strings: checker.has_strings,
        has_advanced_ffi: checker.has_advanced_ffi,
        has_arrays: checker.has_arrays,
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
    enums: HashMap<String, EnumInfo>,
    functions: HashMap<String, FnInfo>,
    order: Vec<String>,
    has_structs: bool,
    has_enums: bool,
    has_arenas: bool,
    has_refs: bool,
    has_strings: bool,
    has_advanced_ffi: bool,
    has_arrays: bool,
    scopes: Vec<HashMap<String, Local>>,
    current_return: Type,
    in_unsafe: bool,
    arena_depth: u32,
    #[allow(dead_code)]
    expr_types: HashMap<usize, Type>,
}

impl<'a> Typechecker<'a> {
    fn type_name_taken(&self, name: &str) -> bool {
        self.structs.contains_key(name) || self.enums.contains_key(name)
    }

    fn register_struct_name(&mut self, def: &StructDef) {
        if self.type_name_taken(&def.name) {
            self.diagnostics
                .error_at(def.span, format!("type `{}` is already defined", def.name));
            return;
        }
        self.has_structs = true;
        if def.is_c_repr {
            self.has_advanced_ffi = true;
        }
        self.structs.insert(
            def.name.clone(),
            StructInfo {
                name: def.name.clone(),
                fields: Vec::new(),
                is_c_repr: def.is_c_repr,
                span: def.span,
            },
        );
    }

    fn define_struct_fields(&mut self, def: &StructDef) {
        let Some(info) = self.structs.get(&def.name).cloned() else {
            return;
        };
        let mut fields = Vec::new();
        for field in &def.fields {
            if let Some(ty) = self.resolve_type(&field.ty) {
                fields.push((field.name.clone(), ty))
            }
        }
        self.structs
            .insert(def.name.clone(), StructInfo { fields, ..info });
    }

    fn register_enum_name(&mut self, def: &EnumDef) {
        if self.type_name_taken(&def.name) {
            self.diagnostics
                .error_at(def.span, format!("type `{}` is already defined", def.name));
            return;
        }
        self.has_enums = true;
        self.enums.insert(
            def.name.clone(),
            EnumInfo {
                name: def.name.clone(),
                variants: Vec::new(),
                span: def.span,
            },
        );
    }

    fn define_enum_variants(&mut self, def: &EnumDef) {
        let Some(info) = self.enums.get(&def.name).cloned() else {
            return;
        };
        let mut variants = Vec::new();
        let mut seen = HashMap::new();
        for (tag, variant) in def.variants.iter().enumerate() {
            if seen.insert(variant.name.clone(), ()).is_some() {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        variant.span,
                        DiagnosticCode::E0309,
                        format!(
                            "duplicate variant `{}` in enum `{}`",
                            variant.name, def.name
                        ),
                    )
                    .help("variant names must be unique within an enum"),
                );
            }
            let mut fields = Vec::new();
            let mut field_names = HashMap::new();
            for field in &variant.fields {
                if field_names.insert(field.name.clone(), ()).is_some() {
                    self.diagnostics.error_at(
                        field.span,
                        format!(
                            "duplicate field `{}` in variant `{}::{}`",
                            field.name, def.name, variant.name
                        ),
                    );
                }
                if let Some(ty) = self.resolve_type(&field.ty) {
                    // Reject direct recursive enum-by-value cycles for this MVP.
                    if matches!(&ty, Type::Enum(n) if n == &def.name) {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                field.span,
                                DiagnosticCode::E0312,
                                format!(
                                    "recursive enum `{0}` by value is not supported; payloads cannot contain `{0}` directly",
                                    def.name
                                ),
                            )
                            .help("use a non-recursive payload type in this release"),
                        );
                    }
                    fields.push((field.name.clone(), ty));
                }
            }
            variants.push(VariantInfo {
                name: variant.name.clone(),
                tag: tag as u32,
                fields,
                span: variant.span,
            });
        }
        self.enums
            .insert(def.name.clone(), EnumInfo { variants, ..info });
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
            Type::Array { .. } | Type::Slice(_) => {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        span,
                        DiagnosticCode::E0500,
                        "arrays and slices are not allowed in `extern \"C\"` signatures yet",
                    )
                    .help(
                        "pass scalars or C-layout structs across FFI until an array ABI is defined",
                    ),
                );
            }
            Type::Enum(name) => {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        span,
                        DiagnosticCode::E0500,
                        format!("enum `{name}` is not allowed in `extern \"C\"` signatures yet"),
                    )
                    .help(
                        "pass scalars or C-layout structs across FFI until an enum ABI is defined",
                    ),
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
        self.resolve_type_inner(ty, false)
    }

    fn resolve_type_inner(&mut self, ty: &TypeExpr, allow_slice: bool) -> Option<Type> {
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
                    } else if self.enums.contains_key(other) {
                        self.has_enums = true;
                        Some(Type::Enum(other.to_string()))
                    } else {
                        self.diagnostics
                            .error_at(ty.span, format!("unknown type `{other}`"));
                        None
                    }
                }
            },
            TypeExprKind::Ref { mutable, inner } => {
                self.has_refs = true;
                let allow_inner_slice = matches!(inner.kind, TypeExprKind::Slice { .. });
                let inner = self.resolve_type_inner(inner, allow_inner_slice)?;
                if matches!(inner, Type::Slice(_)) {
                    self.has_arrays = true;
                }
                Some(Type::Ref {
                    mutable: *mutable,
                    inner: Box::new(inner),
                })
            }
            TypeExprKind::Array { elem, len } => {
                self.has_arrays = true;
                let element = self.resolve_type_inner(elem, false)?;
                if matches!(element, Type::Slice(_)) {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            ty.span,
                            DiagnosticCode::E0305,
                            "array elements cannot be unsized slices; use a fixed array or a reference",
                        )
                        .help("write `[T; N]` for a fixed array, or store `&[T]` / `&mut [T]`"),
                    );
                }
                Some(Type::Array {
                    element: Box::new(element),
                    len: *len as usize,
                })
            }
            TypeExprKind::Slice { elem } => {
                self.has_arrays = true;
                let element = self.resolve_type_inner(elem, false)?;
                if !allow_slice {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            ty.span,
                            DiagnosticCode::E0305,
                            "unsized slice type `[T]` must appear behind `&` or `&mut`",
                        )
                        .help("write `&[T]` or `&mut [T]` for a slice reference"),
                    );
                }
                Some(Type::Slice(Box::new(element)))
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
                let final_ty = if let Some(ann) = ty {
                    let ann_ty = self.resolve_type(ann).unwrap_or(Type::Unit);
                    let value_ty = self.check_expr_expected(value, Some(&ann_ty));
                    if ann_ty != value_ty && !self.can_coerce(&value_ty, &ann_ty) {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                *span,
                                DiagnosticCode::E0301,
                                format!(
                                    "type mismatch: expected `{}`, found `{}`",
                                    ann_ty.display(),
                                    value_ty.display()
                                ),
                            )
                            .help("change the assigned expression type, or cast with `as` when converting numbers"),
                        );
                    }
                    ann_ty
                } else {
                    self.check_expr(value)
                };
                // Move non-copy values out of the RHS name if applicable.
                self.maybe_move_expr(value);
                if matches!(final_ty, Type::Slice(_)) {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            *span,
                            DiagnosticCode::E0305,
                            "cannot store an unsized slice by value; use `&[T]` or `&mut [T]`",
                        )
                        .help("write `let s = &array[start..end]` to borrow a slice"),
                    );
                }
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
                        if let ExprKind::Index { base, .. } = &target.kind {
                            self.check_index_assignable(base, *span);
                        }
                        let target_ty = self.check_expr(target);
                        if target_ty != value_ty {
                            self.diagnostics.push(
                                Diagnostic::error_at_code(
                                    *span,
                                    DiagnosticCode::E0301,
                                    format!(
                                        "type mismatch: expected `{}`, found `{}`",
                                        target_ty.display(),
                                        value_ty.display()
                                    ),
                                )
                                .help("change the assigned expression type, or cast with `as` when converting numbers"),
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
                self.arena_depth = self.arena_depth.saturating_add(1);
                self.check_block(body);
                self.arena_depth = self.arena_depth.saturating_sub(1);
            }
            Stmt::Unsafe { body, .. } => {
                let prev = self.in_unsafe;
                self.in_unsafe = true;
                self.check_block(body);
                self.in_unsafe = prev;
            }
            Stmt::Match {
                scrutinee,
                arms,
                span,
            } => {
                self.check_match(scrutinee, arms, *span);
            }
        }
    }

    fn check_match(&mut self, scrutinee: &Expr, arms: &[MatchArm], span: Span) {
        let scrut_ty = self.check_expr(scrutinee);
        self.maybe_move_expr(scrutinee);

        let mut covered_variants: Vec<String> = Vec::new();
        let mut covered_bools = (false, false);
        let mut saw_wildcard = false;
        let mut saw_binding = false;

        for (i, arm) in arms.iter().enumerate() {
            if saw_wildcard || saw_binding {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        arm.span,
                        DiagnosticCode::E0311,
                        "unreachable match arm",
                    )
                    .note("a previous `_` or binding pattern already covers every remaining value")
                    .help("remove this arm or place it before the catch-all"),
                );
            }

            let bindings = self.check_pattern(&arm.pattern, &scrut_ty);
            match &arm.pattern.kind {
                PatternKind::Wildcard => saw_wildcard = true,
                PatternKind::Binding(_) => saw_binding = true,
                PatternKind::Bool(v) => {
                    if *v {
                        if covered_bools.0 {
                            self.diagnostics.push(
                                Diagnostic::error_at_code(
                                    arm.pattern.span,
                                    DiagnosticCode::E0311,
                                    "unreachable pattern: `true` already covered",
                                )
                                .help("remove the duplicate arm"),
                            );
                        }
                        covered_bools.0 = true;
                    } else {
                        if covered_bools.1 {
                            self.diagnostics.push(
                                Diagnostic::error_at_code(
                                    arm.pattern.span,
                                    DiagnosticCode::E0311,
                                    "unreachable pattern: `false` already covered",
                                )
                                .help("remove the duplicate arm"),
                            );
                        }
                        covered_bools.1 = true;
                    }
                }
                PatternKind::Variant {
                    enum_name, variant, ..
                } => {
                    let key = format!("{enum_name}::{variant}");
                    if covered_variants.iter().any(|v| v == &key) {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                arm.pattern.span,
                                DiagnosticCode::E0311,
                                format!("unreachable pattern: `{key}` already covered"),
                            )
                            .help("remove the duplicate arm"),
                        );
                    } else {
                        covered_variants.push(key);
                    }
                }
            }

            self.push_scope();
            for (name, ty, bspan) in bindings {
                self.declare(&name, ty, false, bspan);
            }
            self.check_block(&arm.body);
            self.pop_scope();
            let _ = i;
        }

        // Exhaustiveness
        match &scrut_ty {
            Type::Enum(ename) => {
                if saw_wildcard || saw_binding {
                    return;
                }
                if let Some(info) = self.enums.get(ename) {
                    let missing: Vec<_> = info
                        .variants
                        .iter()
                        .filter(|v| {
                            let key = format!("{ename}::{}", v.name);
                            !covered_variants.iter().any(|c| c == &key)
                        })
                        .map(|v| format!("{ename}::{}", v.name))
                        .collect();
                    if !missing.is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                span,
                                DiagnosticCode::E0310,
                                format!("non-exhaustive match on `{ename}`"),
                            )
                            .note(format!("missing pattern(s): {}", missing.join(", ")))
                            .help("add the missing cases or a final `case _:`"),
                        );
                    }
                }
            }
            Type::Bool => {
                if saw_wildcard || saw_binding {
                    return;
                }
                let mut missing = Vec::new();
                if !covered_bools.0 {
                    missing.push("`true`");
                }
                if !covered_bools.1 {
                    missing.push("`false`");
                }
                if !missing.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            span,
                            DiagnosticCode::E0310,
                            "non-exhaustive match on `bool`",
                        )
                        .note(format!("missing pattern(s): {}", missing.join(", ")))
                        .help("cover both `true` and `false`, or add `case _:`"),
                    );
                }
            }
            _ => {
                if !(saw_wildcard || saw_binding) {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            span,
                            DiagnosticCode::E0310,
                            format!("non-exhaustive match on `{}`", scrut_ty.display()),
                        )
                        .note("only enums and `bool` support structured patterns in this release")
                        .help("add a catch-all `case _:` or `case name:`"),
                    );
                }
            }
        }
    }

    fn check_pattern(&mut self, pattern: &Pattern, expected: &Type) -> Vec<(String, Type, Span)> {
        let mut bindings = Vec::new();
        match &pattern.kind {
            PatternKind::Wildcard => {}
            PatternKind::Binding(name) => {
                bindings.push((name.clone(), expected.clone(), pattern.span));
            }
            PatternKind::Bool(_) => {
                if *expected != Type::Bool {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            pattern.span,
                            DiagnosticCode::E0312,
                            format!(
                                "pattern type mismatch: expected `{}`, found `bool` pattern",
                                expected.display()
                            ),
                        )
                        .help("use a pattern that matches the scrutinee type"),
                    );
                }
            }
            PatternKind::Variant {
                enum_name,
                variant,
                fields,
            } => {
                let Some(info) = self.enums.get(enum_name).cloned() else {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            pattern.span,
                            DiagnosticCode::E0309,
                            format!("unknown enum `{enum_name}`"),
                        )
                        .help("declare the enum before matching on it"),
                    );
                    return bindings;
                };
                if let Type::Enum(ename) = expected {
                    if ename != enum_name {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                pattern.span,
                                DiagnosticCode::E0312,
                                format!(
                                    "pattern type mismatch: expected `{ename}`, found `{enum_name}::{variant}`"
                                ),
                            )
                            .help("match variants of the scrutinee enum only"),
                        );
                    }
                } else {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            pattern.span,
                            DiagnosticCode::E0312,
                            format!("cannot match enum pattern on `{}`", expected.display()),
                        )
                        .help("the scrutinee must be an enum value"),
                    );
                }
                let Some(vinfo) = info.variants.iter().find(|v| v.name == *variant).cloned() else {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            pattern.span,
                            DiagnosticCode::E0309,
                            format!("enum `{enum_name}` has no variant `{variant}`"),
                        )
                        .help("check the variant name spelling"),
                    );
                    return bindings;
                };
                if fields.len() != vinfo.fields.len() {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            pattern.span,
                            DiagnosticCode::E0312,
                            format!(
                                "variant `{enum_name}::{variant}` expects {} field pattern(s), found {}",
                                vinfo.fields.len(),
                                fields.len()
                            ),
                        )
                        .help("provide one subpattern per payload field, in declaration order"),
                    );
                }
                let mut seen_names = HashMap::new();
                for (sub, (_, fty)) in fields.iter().zip(vinfo.fields.iter()) {
                    let nested = self.check_pattern(sub, fty);
                    for (n, t, s) in nested {
                        if seen_names.insert(n.clone(), ()).is_some()
                            || bindings.iter().any(|(bn, _, _)| bn == &n)
                        {
                            self.diagnostics.push(
                                Diagnostic::error_at_code(
                                    s,
                                    DiagnosticCode::E0312,
                                    format!(
                                        "identifier `{n}` is bound more than once in this pattern"
                                    ),
                                )
                                .help("use unique binding names within a pattern"),
                            );
                        }
                        bindings.push((n, t, s));
                    }
                }
            }
        }
        bindings
    }

    fn check_expr(&mut self, expr: &Expr) -> Type {
        self.check_expr_expected(expr, None)
    }

    fn check_expr_expected(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
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
            ExprKind::EnumConstruct {
                enum_name,
                variant,
                args,
            } => {
                self.has_enums = true;
                let Some(info) = self.enums.get(enum_name).cloned() else {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0309,
                            format!("unknown enum `{enum_name}`"),
                        )
                        .help("declare the enum before constructing a variant"),
                    );
                    return Type::Unit;
                };
                let Some(vinfo) = info.variants.iter().find(|v| v.name == *variant).cloned() else {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0309,
                            format!("enum `{enum_name}` has no variant `{variant}`"),
                        )
                        .help("check the variant name spelling"),
                    );
                    return Type::Enum(enum_name.clone());
                };
                if args.len() != vinfo.fields.len() {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0312,
                            format!(
                                "variant `{enum_name}::{variant}` expects {} argument(s), found {}",
                                vinfo.fields.len(),
                                args.len()
                            ),
                        )
                        .help("pass one argument per payload field, in declaration order"),
                    );
                }
                for (arg, (_, expected)) in args.iter().zip(vinfo.fields.iter()) {
                    let at = self.check_expr_expected(arg, Some(expected));
                    if at != *expected && !self.can_coerce(&at, expected) {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                arg.span,
                                DiagnosticCode::E0312,
                                format!(
                                    "argument type mismatch: expected `{}`, found `{}`",
                                    expected.display(),
                                    at.display()
                                ),
                            )
                            .help("adjust the argument type to match the variant payload"),
                        );
                    }
                    self.maybe_move_expr(arg);
                }
                Type::Enum(enum_name.clone())
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
            ExprKind::Index { base, index } => self.check_index(expr, base, index),
            ExprKind::ArrayLit { elems } => self.check_array_lit(expr, elems, expected),
            ExprKind::Slice { base, start, end } => {
                self.check_slice(expr, base, start.as_deref(), end.as_deref())
            }
        };
        ty
    }

    fn check_array_lit(&mut self, expr: &Expr, elems: &[Expr], expected: Option<&Type>) -> Type {
        self.has_arrays = true;
        if elems.is_empty() {
            match expected {
                Some(Type::Array { element, len }) if *len == 0 => {
                    return Type::Array {
                        element: element.clone(),
                        len: 0,
                    };
                }
                Some(other) => {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0308,
                            format!(
                                "empty array literal requires a fixed array type annotation such as `[T; 0]`, found expected `{}`",
                                other.display()
                            ),
                        )
                        .help("annotate the binding: `let xs: [i64; 0] = []`"),
                    );
                    return other.clone();
                }
                None => {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0308,
                            "empty array literal requires a type annotation",
                        )
                        .help("annotate the binding: `let xs: [i64; 0] = []`"),
                    );
                    return Type::Array {
                        element: Box::new(Type::Unit),
                        len: 0,
                    };
                }
            }
        }

        let expected_elem = match expected {
            Some(Type::Array { element, .. }) => Some(element.as_ref()),
            _ => None,
        };
        let mut elem_ty = expected_elem.cloned().unwrap_or(Type::Unit);
        for (i, e) in elems.iter().enumerate() {
            let t = self.check_expr_expected(e, expected_elem);
            if i == 0 && expected_elem.is_none() {
                elem_ty = t;
            } else if t != elem_ty && !self.can_coerce(&t, &elem_ty) {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        e.span,
                        DiagnosticCode::E0308,
                        format!(
                            "array element type mismatch: expected `{}`, found `{}`",
                            elem_ty.display(),
                            t.display()
                        ),
                    )
                    .help("all elements of an array literal must have the same type"),
                );
            }
        }
        let lit_len = elems.len();
        if let Some(Type::Array { len, .. }) = expected {
            if *len != lit_len {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        expr.span,
                        DiagnosticCode::E0308,
                        format!(
                            "array literal length mismatch: expected {len} element(s), found {lit_len}"
                        ),
                    )
                    .help("adjust the literal or the `[T; N]` annotation so lengths match"),
                );
            }
        }
        Type::Array {
            element: Box::new(elem_ty),
            len: lit_len,
        }
    }

    fn check_index(&mut self, expr: &Expr, base: &Expr, index: &Expr) -> Type {
        self.has_arrays = true;
        let base_ty = self.check_expr(base);
        let it = self.check_expr(index);
        if it != Type::I64 {
            self.diagnostics.push(
                Diagnostic::error_at_code(
                    index.span,
                    DiagnosticCode::E0305,
                    format!("index must be `i64`, found `{}`", it.display()),
                )
                .help("use an `i64` index expression"),
            );
        }
        if let ExprKind::Int(n) = index.kind {
            if let Some(len) = base_ty.array_len() {
                if n < 0 || n as usize >= len {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            index.span,
                            DiagnosticCode::E0306,
                            format!("index `{n}` is out of bounds for array of length {len}"),
                        )
                        .help("use an index in `0..len`"),
                    );
                }
            }
        }
        match &base_ty {
            Type::Array { element, .. } => {
                if !element.is_copy() {
                    // Reading an element by value would move; reject and suggest a reference.
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0307,
                            format!(
                                "cannot move element of type `{}` out of an array",
                                element.display()
                            ),
                        )
                        .help("borrow the element with `&array[i]` or `&mut array[i]`"),
                    );
                }
                *element.clone()
            }
            Type::Slice(element) => {
                if !element.is_copy() {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0307,
                            format!(
                                "cannot move element of type `{}` out of a slice",
                                element.display()
                            ),
                        )
                        .help("borrow the element with `&slice[i]` or `&mut slice[i]`"),
                    );
                }
                *element.clone()
            }
            Type::Ref { inner, .. } => match inner.as_ref() {
                Type::Array { element, .. } | Type::Slice(element) => {
                    if !element.is_copy() {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                expr.span,
                                DiagnosticCode::E0307,
                                format!(
                                    "cannot move element of type `{}` out of a borrowed array or slice",
                                    element.display()
                                ),
                            )
                            .help("borrow the element instead of moving it"),
                        );
                    }
                    *element.clone()
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0305,
                            format!("type `{}` cannot be indexed", base_ty.display()),
                        )
                        .help("index `[T; N]`, `&[T]`, or `&mut [T]` values"),
                    );
                    Type::Unit
                }
            },
            _ => {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        expr.span,
                        DiagnosticCode::E0305,
                        format!("type `{}` cannot be indexed", base_ty.display()),
                    )
                    .note("slicing `str` is not supported in this release")
                    .help("index `[T; N]`, `&[T]`, or `&mut [T]` values"),
                );
                Type::Unit
            }
        }
    }

    fn check_slice(
        &mut self,
        expr: &Expr,
        base: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
    ) -> Type {
        self.has_arrays = true;
        let base_ty = self.check_expr(base);
        let (element, len_opt) = match &base_ty {
            Type::Array { element, len } => (element.as_ref().clone(), Some(*len)),
            Type::Slice(element) => (element.as_ref().clone(), None),
            Type::Ref { inner, .. } => match inner.as_ref() {
                Type::Array { element, len } => (element.as_ref().clone(), Some(*len)),
                Type::Slice(element) => (element.as_ref().clone(), None),
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0305,
                            format!("type `{}` cannot be sliced", base_ty.display()),
                        )
                        .note("slicing `str` is not supported in this release")
                        .help("slice `[T; N]`, `&[T]`, or `&mut [T]` values"),
                    );
                    return Type::Slice(Box::new(Type::Unit));
                }
            },
            _ => {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        expr.span,
                        DiagnosticCode::E0305,
                        format!("type `{}` cannot be sliced", base_ty.display()),
                    )
                    .note("slicing `str` is not supported in this release")
                    .help("slice `[T; N]`, `&[T]`, or `&mut [T]` values"),
                );
                return Type::Slice(Box::new(Type::Unit));
            }
        };

        let mut start_lit: Option<i64> = Some(0);
        let mut end_lit: Option<i64> = len_opt.map(|l| l as i64);

        if let Some(s) = start {
            let st = self.check_expr(s);
            if st != Type::I64 {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        s.span,
                        DiagnosticCode::E0305,
                        format!("slice start must be `i64`, found `{}`", st.display()),
                    )
                    .help("use an `i64` start index"),
                );
            }
            start_lit = match &s.kind {
                ExprKind::Int(n) => Some(*n),
                _ => None,
            };
        }
        if let Some(e) = end {
            let et = self.check_expr(e);
            if et != Type::I64 {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        e.span,
                        DiagnosticCode::E0305,
                        format!("slice end must be `i64`, found `{}`", et.display()),
                    )
                    .help("use an `i64` end index"),
                );
            }
            end_lit = match &e.kind {
                ExprKind::Int(n) => Some(*n),
                _ => None,
            };
        }

        if let (Some(s), Some(e)) = (start_lit, end_lit) {
            if s < 0 || e < 0 || s > e {
                self.diagnostics.push(
                    Diagnostic::error_at_code(
                        expr.span,
                        DiagnosticCode::E0306,
                        format!("invalid slice range `{s}..{e}`"),
                    )
                    .help("require `0 <= start <= end`"),
                );
            } else if let Some(len) = len_opt {
                if e as usize > len {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            expr.span,
                            DiagnosticCode::E0306,
                            format!("slice end `{e}` is out of bounds for array of length {len}"),
                        )
                        .help("require `end <= len`"),
                    );
                }
            }
        }

        Type::Slice(Box::new(element))
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
            ExprKind::Index { base, .. } | ExprKind::Slice { base, .. } => {
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

    fn check_index_assignable(&mut self, base: &Expr, span: Span) {
        match &base.kind {
            ExprKind::Name(name) => {
                if let Some(local) = self.lookup(name) {
                    if !local.mutable {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                span,
                                DiagnosticCode::E0302,
                                format!("cannot assign to element of immutable array `{name}`"),
                            )
                            .help(format!(
                                "declare it as `let mut {name} = ...` if mutation is intended"
                            )),
                        );
                    }
                }
            }
            ExprKind::Deref { expr } => {
                let t = self.check_expr(expr);
                if let Type::Ref { mutable: false, .. } = t {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            span,
                            DiagnosticCode::E0302,
                            "cannot assign through a shared slice or array reference",
                        )
                        .help("use `&mut [T]` when mutation is intended"),
                    );
                }
            }
            ExprKind::Index { base, .. } => self.check_index_assignable(base, span),
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
                self.has_arenas = true;
                self.expect_arity(name, args, 1, span);
                if self.arena_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            span,
                            DiagnosticCode::E0500,
                            "`to_c_string` requires an active `arena:` block",
                        )
                        .help("wrap the conversion in `arena:` so the resulting `c_string` has a known lifetime"),
                    );
                }
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
            "len" => {
                self.has_arrays = true;
                self.expect_arity(name, args, 1, span);
                if let Some(a) = args.first() {
                    let t = self.check_expr(a);
                    let ok = match &t {
                        Type::Array { .. } | Type::Slice(_) => true,
                        Type::Ref { inner, .. } => {
                            matches!(inner.as_ref(), Type::Array { .. } | Type::Slice(_))
                        }
                        _ => false,
                    };
                    if !ok {
                        self.diagnostics.push(
                            Diagnostic::error_at_code(
                                a.span,
                                DiagnosticCode::E0305,
                                format!("`len` expects an array or slice, found `{}`", t.display()),
                            )
                            .help("pass `[T; N]`, `&[T]`, or `&mut [T]`"),
                        );
                    }
                }
                Some(Type::I64)
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
                if matches!(lt, Type::Enum(_)) || matches!(rt, Type::Enum(_)) {
                    self.diagnostics.push(
                        Diagnostic::error_at_code(
                            span,
                            DiagnosticCode::E0301,
                            "comparing enums with `==` / `!=` is not supported yet; use `match`",
                        )
                        .help("destructure with `match` instead of comparing enum values"),
                    );
                    Type::Bool
                } else if lt == rt {
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
        match &expr.kind {
            ExprKind::Name(name) => {
                if let Some(local) = self.lookup_mut(name) {
                    if !local.ty.is_copy() {
                        local.moved = true;
                    }
                }
            }
            ExprKind::Index { .. } => {
                // Indexing of non-copy elements is already rejected in check_index.
            }
            _ => {}
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
