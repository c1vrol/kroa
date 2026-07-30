//! Lexical arena helpers and documentation of memory model.
//!
//! Arenas are represented in Kroa IR as `arena.enter` / `arena.exit` pairs.
//! The runtime keeps a stack of bump allocators. Leaving a scope (or returning
//! early) pops the arena and frees every allocation from that block at once.

use crate::ir::{InstKind, Module};

/// Returns true if the module uses arena instructions.
pub fn module_uses_arenas(module: &Module) -> bool {
    module.functions.iter().any(|f| {
        f.blocks.iter().any(|b| {
            b.insts.iter().any(|i| {
                matches!(
                    i.kind,
                    InstKind::ArenaEnter | InstKind::ArenaExit | InstKind::ArenaAlloc { .. }
                )
            })
        })
    })
}
