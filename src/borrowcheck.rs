//! Initial Phase 3 borrow checker.
//!
//! This pass deliberately uses lexical, forward dataflow. A place has at most
//! one active loan record, and loans are not shortened using value liveness.

use crate::diagnostics::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::ir::{BlockId, Function, Inst, InstKind, Module, Terminator, ValueId};
use crate::span::{SourceFile, Span};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone)]
struct Loan {
    mutable: bool,
    span: Span,
    arena_depth: u32,
}

#[derive(Debug, Clone, Default)]
struct FlowState {
    loans: HashMap<ValueId, Loan>,
    arena_depth: u32,
}

pub fn borrow_check(_file: &SourceFile, module: &Module, diagnostics: &mut Diagnostics) {
    for function in &module.functions {
        check_function(function, diagnostics);
    }
}

fn check_function(function: &Function, diagnostics: &mut Diagnostics) {
    let Some(entry) = function.blocks.first().map(|block| block.id) else {
        return;
    };
    let blocks: HashMap<BlockId, _> = function.blocks.iter().map(|b| (b.id, b)).collect();
    let mut incoming: HashMap<BlockId, FlowState> = HashMap::new();
    let mut work = VecDeque::from([(entry, FlowState::default())]);
    let mut loaded_from: HashMap<ValueId, ValueId> = HashMap::new();
    let mut local_refs = HashSet::new();
    let mut arena_values: HashMap<ValueId, u32> = HashMap::new();
    let mut reported = HashSet::new();

    while let Some((block_id, candidate)) = work.pop_front() {
        let state = match incoming.get(&block_id) {
            Some(previous) => {
                let joined = join_states(previous, &candidate);
                if same_state(previous, &joined) {
                    continue;
                }
                joined
            }
            None => candidate,
        };
        incoming.insert(block_id, state.clone());

        let Some(block) = blocks.get(&block_id) else {
            continue;
        };
        let mut state = state;
        for inst in &block.insts {
            apply_inst(
                inst,
                &mut state,
                &mut loaded_from,
                &mut local_refs,
                &mut arena_values,
                &mut reported,
                diagnostics,
            );
        }

        if let Terminator::Return(Some(value)) = block.terminator {
            if local_refs.contains(&value)
                || arena_values.get(&value).is_some_and(|depth| *depth > 0)
            {
                report_once(
                    &mut reported,
                    function.span,
                    "E0403",
                    diagnostics,
                    Diagnostic::error_at_code(
                        function.span,
                        DiagnosticCode::E0403,
                        "cannot return a reference to local or arena storage",
                    )
                    .help("return an owned value or a reference received from the caller"),
                );
            }
        }

        for successor in terminator_targets(&block.terminator) {
            work.push_back((successor, state.clone()));
        }
    }
}

fn apply_inst(
    inst: &Inst,
    state: &mut FlowState,
    loaded_from: &mut HashMap<ValueId, ValueId>,
    local_refs: &mut HashSet<ValueId>,
    arena_values: &mut HashMap<ValueId, u32>,
    reported: &mut HashSet<(usize, usize, &'static str)>,
    diagnostics: &mut Diagnostics,
) {
    match &inst.kind {
        InstKind::ArenaEnter => state.arena_depth = state.arena_depth.saturating_add(1),
        InstKind::ArenaExit => {
            if state.arena_depth == 0 {
                report_once(
                    reported,
                    inst.span,
                    "E0404",
                    diagnostics,
                    Diagnostic::error_at_code(
                        inst.span,
                        DiagnosticCode::E0404,
                        "arena exit without a matching arena enter",
                    )
                    .help("balance every arena enter with exactly one exit"),
                );
            } else {
                let exiting = state.arena_depth;
                state.loans.retain(|_, loan| loan.arena_depth < exiting);
                state.arena_depth -= 1;
            }
        }
        InstKind::Ref { mutable, place } => {
            if let Some(previous) = state.loans.get(place) {
                if *mutable || previous.mutable {
                    let kind = if *mutable { "mutable" } else { "shared" };
                    report_once(
                        reported,
                        inst.span,
                        "E0400",
                        diagnostics,
                        Diagnostic::error_at_code(
                            inst.span,
                            DiagnosticCode::E0400,
                            format!(
                                "cannot create a {kind} borrow because the place is already borrowed"
                            ),
                        )
                        .note("Kroa permits shared borrows or one mutable borrow, never both")
                        .help("narrow the lexical scope of the earlier borrow"),
                    );
                }
            } else {
                state.loans.insert(
                    *place,
                    Loan {
                        mutable: *mutable,
                        span: inst.span,
                        arena_depth: state.arena_depth,
                    },
                );
            }
            if let Some(id) = inst.id {
                local_refs.insert(id);
                if state.arena_depth > 0 {
                    arena_values.insert(id, state.arena_depth);
                }
            }
        }
        InstKind::Store { ptr, value } => {
            if state.loans.contains_key(ptr) {
                report_once(
                    reported,
                    inst.span,
                    "E0401",
                    diagnostics,
                    Diagnostic::error_at_code(
                        inst.span,
                        DiagnosticCode::E0401,
                        "cannot assign to a place while it is borrowed",
                    )
                    .help("finish the lexical borrow scope before assigning"),
                );
            }
            if let Some(depth) = arena_values.get(value).copied() {
                arena_values.insert(*ptr, depth);
            }
        }
        InstKind::Load { ptr } => {
            if let Some(id) = inst.id {
                loaded_from.insert(id, *ptr);
                if let Some(depth) = arena_values.get(ptr).copied() {
                    arena_values.insert(id, depth);
                }
            }
        }
        InstKind::Move { value } => {
            let place = loaded_from.get(value).copied().unwrap_or(*value);
            if let Some(loan) = state.loans.get(&place) {
                report_once(
                    reported,
                    inst.span,
                    "E0402",
                    diagnostics,
                    Diagnostic::error_at_code(
                        inst.span,
                        DiagnosticCode::E0402,
                        "cannot move a value while it is borrowed",
                    )
                    .note(format!(
                        "the active borrow began at byte {}",
                        loan.span.start
                    ))
                    .help("move the value before borrowing it"),
                );
            }
            if let Some(id) = inst.id {
                if local_refs.contains(value) {
                    local_refs.insert(id);
                }
                if let Some(depth) = arena_values.get(value).copied() {
                    arena_values.insert(id, depth);
                }
            }
        }
        InstKind::ToCString { value } => {
            if let Some(id) = inst.id {
                let depth = arena_values
                    .get(value)
                    .copied()
                    .unwrap_or(state.arena_depth);
                if depth > 0 {
                    arena_values.insert(id, depth);
                }
            }
        }
        InstKind::ArenaAlloc { .. } => {
            if let Some(id) = inst.id {
                arena_values.insert(id, state.arena_depth);
            }
        }
        _ => {}
    }
}

fn join_states(left: &FlowState, right: &FlowState) -> FlowState {
    let mut loans = left.loans.clone();
    for (place, loan) in &right.loans {
        loans
            .entry(*place)
            .and_modify(|known| {
                known.mutable |= loan.mutable;
                known.arena_depth = known.arena_depth.max(loan.arena_depth);
            })
            .or_insert_with(|| loan.clone());
    }
    FlowState {
        loans,
        arena_depth: left.arena_depth.max(right.arena_depth),
    }
}

fn same_state(left: &FlowState, right: &FlowState) -> bool {
    left.arena_depth == right.arena_depth
        && left.loans.len() == right.loans.len()
        && left.loans.iter().all(|(place, loan)| {
            right.loans.get(place).is_some_and(|other| {
                loan.mutable == other.mutable && loan.arena_depth == other.arena_depth
            })
        })
}

fn terminator_targets(terminator: &Terminator) -> Vec<BlockId> {
    match terminator {
        Terminator::Jump(block) => vec![*block],
        Terminator::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        Terminator::Return(_) | Terminator::Unreachable => Vec::new(),
    }
}

fn report_once(
    reported: &mut HashSet<(usize, usize, &'static str)>,
    span: Span,
    tag: &'static str,
    diagnostics: &mut Diagnostics,
    diagnostic: Diagnostic,
) {
    if reported.insert((span.start, span.end, tag)) {
        diagnostics.push(diagnostic);
    }
}
