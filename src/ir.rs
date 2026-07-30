//! Kroa IR: a simple typed SSA-like instruction stream with basic blocks.

use crate::span::Span;
use crate::typecheck::{FnInfo, StructInfo, Type};
use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone)]
pub struct Module {
    pub structs: HashMap<String, StructInfo>,
    pub functions: Vec<Function>,
    pub externs: Vec<ExternFn>,
}

#[derive(Debug, Clone)]
pub struct ExternFn {
    pub name: String,
    pub params: Vec<Type>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, Type, ValueId)>,
    pub return_type: Type,
    pub blocks: Vec<BasicBlock>,
    pub next_value: u32,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub name: String,
    pub insts: Vec<Inst>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Option<ValueId>),
    Jump(BlockId),
    Branch {
        cond: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct Inst {
    pub id: Option<ValueId>,
    pub ty: Type,
    pub kind: InstKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum InstKind {
    Nop,
    ConstI64(i64),
    ConstF64(f64),
    ConstBool(bool),
    ConstStr(String),
    /// Alloca-like slot for a local (pointer to value).
    Alloca,
    Load {
        ptr: ValueId,
    },
    Store {
        ptr: ValueId,
        value: ValueId,
    },
    Binary {
        op: BinOp,
        left: ValueId,
        right: ValueId,
    },
    Unary {
        op: UnOp,
        value: ValueId,
    },
    Call {
        name: String,
        args: Vec<ValueId>,
        is_extern: bool,
    },
    StructAgg {
        name: String,
        fields: Vec<ValueId>,
    },
    ExtractField {
        agg: ValueId,
        index: usize,
    },
    InsertField {
        agg: ValueId,
        index: usize,
        value: ValueId,
    },
    Cast {
        value: ValueId,
        to: Type,
    },
    /// Move: logical transfer; lowers like a copy for POD, tracks ownership in analyses.
    Move {
        value: ValueId,
    },
    Ref {
        mutable: bool,
        place: ValueId,
    },
    Deref {
        ptr: ValueId,
    },
    ArenaEnter,
    ArenaExit,
    /// Allocate `nbytes` inside the current arena; returns pointer (i64 as opaque for MVP).
    ArenaAlloc {
        nbytes: ValueId,
    },
    ToCString {
        value: ValueId,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy)]
pub enum UnOp {
    Neg,
    Not,
}

pub fn format_module(module: &Module) -> String {
    let mut out = String::new();
    for ext in &module.externs {
        let params: Vec<_> = ext.params.iter().map(|t| t.display()).collect();
        let _ = writeln!(
            out,
            "extern {}({}) -> {}",
            ext.name,
            params.join(", "),
            ext.return_type.display()
        );
    }
    for func in &module.functions {
        let params: Vec<_> = func
            .params
            .iter()
            .map(|(n, t, v)| format!("{n}: {} = %{}", t.display(), v.0))
            .collect();
        let _ = writeln!(
            out,
            "fn {}({}) -> {} {{",
            func.name,
            params.join(", "),
            func.return_type.display()
        );
        for block in &func.blocks {
            let _ = writeln!(out, "  {}(%{}):", block.name, block.id.0);
            for inst in &block.insts {
                let _ = writeln!(out, "    {}", format_inst(inst));
            }
            let _ = writeln!(out, "    {}", format_term(&block.terminator));
        }
        let _ = writeln!(out, "}}");
    }
    out
}

fn format_inst(inst: &Inst) -> String {
    let dest = inst
        .id
        .map(|v| format!("%{}: {} = ", v.0, inst.ty.display()))
        .unwrap_or_default();
    let body = match &inst.kind {
        InstKind::Nop => "nop".into(),
        InstKind::ConstI64(v) => format!("const.i64 {v}"),
        InstKind::ConstF64(v) => format!("const.f64 {v}"),
        InstKind::ConstBool(v) => format!("const.bool {v}"),
        InstKind::ConstStr(s) => format!("const.str \"{s}\""),
        InstKind::Alloca => "alloca".into(),
        InstKind::Load { ptr } => format!("load %{}", ptr.0),
        InstKind::Store { ptr, value } => format!("store %{} -> %{}", value.0, ptr.0),
        InstKind::Binary { op, left, right } => {
            format!("{op:?} %{} %{}", left.0, right.0)
        }
        InstKind::Unary { op, value } => format!("{op:?} %{}", value.0),
        InstKind::Call { name, args, .. } => {
            let a: Vec<_> = args.iter().map(|v| format!("%{}", v.0)).collect();
            format!("call {name}({})", a.join(", "))
        }
        InstKind::StructAgg { name, fields } => {
            let a: Vec<_> = fields.iter().map(|v| format!("%{}", v.0)).collect();
            format!("struct {name} {{ {} }}", a.join(", "))
        }
        InstKind::ExtractField { agg, index } => format!("extract %{}[{index}]", agg.0),
        InstKind::InsertField { agg, index, value } => {
            format!("insert %{}[{index}] = %{}", agg.0, value.0)
        }
        InstKind::Cast { value, to } => format!("cast %{} to {}", value.0, to.display()),
        InstKind::Move { value } => format!("move %{}", value.0),
        InstKind::Ref { mutable, place } => {
            if *mutable {
                format!("ref.mut %{}", place.0)
            } else {
                format!("ref %{}", place.0)
            }
        }
        InstKind::Deref { ptr } => format!("deref %{}", ptr.0),
        InstKind::ArenaEnter => "arena.enter".into(),
        InstKind::ArenaExit => "arena.exit".into(),
        InstKind::ArenaAlloc { nbytes } => format!("arena.alloc %{}", nbytes.0),
        InstKind::ToCString { value } => format!("to_c_string %{}", value.0),
    };
    format!("{dest}{body}")
}

fn format_term(term: &Terminator) -> String {
    match term {
        Terminator::Return(None) => "return".into(),
        Terminator::Return(Some(v)) => format!("return %{}", v.0),
        Terminator::Jump(b) => format!("jump block_{}", b.0),
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => format!(
            "branch %{} block_{} block_{}",
            cond.0, then_block.0, else_block.0
        ),
        Terminator::Unreachable => "unreachable".into(),
    }
}

impl From<&FnInfo> for ExternFn {
    fn from(info: &FnInfo) -> Self {
        Self {
            name: info.name.clone(),
            params: info.params.iter().map(|(_, t)| t.clone()).collect(),
            return_type: info.return_type.clone(),
        }
    }
}
