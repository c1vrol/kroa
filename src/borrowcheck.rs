//! Borrow checker over Kroa IR control-flow graphs (NLL-lite).
//!
//! Rules:
//! - A reference must not outlive the place it borrows.
//! - `&mut T` is exclusive: no other `&T` or `&mut T` to the same place at once.
//! - Loans end at the last use of any value (or local slot) that still carries the
//!   reference — non-lexical lifetimes along each CFG path.
//! - Values allocated in an arena must not be referenced after `arena.exit`.
//!
//! Diagnostics are written for agents: stable codes, root cause, and actionable help.

use crate::diagnostics::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::ir::{BlockId, Function, Inst, InstKind, Module, Terminator, ValueId};
use crate::span::{SourceFile, Span};
use crate::typecheck::Type;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct LoanId(u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LoanKey {
    id: LoanId,
    place: ValueId,
    mutable: bool,
    born_arena: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Loan {
    id: LoanId,
    place: ValueId,
    mutable: bool,
    span: Span,
    born_arena: u32,
}

impl Loan {
    fn key(&self) -> LoanKey {
        LoanKey {
            id: self.id,
            place: self.place,
            mutable: self.mutable,
            born_arena: self.born_arena,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LoanSet {
    by_place: HashMap<ValueId, Vec<Loan>>,
    by_id: HashMap<LoanId, Loan>,
    /// SSA values that currently carry a loaned reference.
    value_carrier: HashMap<ValueId, HashSet<LoanId>>,
    /// Alloca/place slots that currently store a loaned reference.
    slot_carrier: HashMap<ValueId, HashSet<LoanId>>,
}

impl LoanSet {
    fn loans_on(&self, place: ValueId) -> &[Loan] {
        self.by_place
            .get(&place)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn insert(&mut self, loan: Loan, carrier: ValueId) {
        let id = loan.id;
        self.value_carrier.entry(carrier).or_default().insert(id);
        if self.by_id.contains_key(&id) {
            return;
        }
        self.by_place
            .entry(loan.place)
            .or_default()
            .push(loan.clone());
        self.by_id.insert(id, loan);
    }

    fn bind_value(&mut self, value: ValueId, loan: LoanId) {
        if self.by_id.contains_key(&loan) {
            self.value_carrier.entry(value).or_default().insert(loan);
        }
    }

    fn bind_value_set(&mut self, value: ValueId, loans: &HashSet<LoanId>) {
        for loan in loans {
            self.bind_value(value, *loan);
        }
    }

    fn replace_slot(&mut self, slot: ValueId, loans: HashSet<LoanId>) {
        if loans.is_empty() {
            self.slot_carrier.remove(&slot);
        } else {
            self.slot_carrier.insert(slot, loans);
        }
    }

    fn carrier_loans(&self, value: ValueId) -> HashSet<LoanId> {
        self.value_carrier.get(&value).cloned().unwrap_or_default()
    }

    fn retain_live(&mut self, live: &HashSet<ValueId>) {
        let mut live_loans: HashSet<LoanId> = HashSet::new();
        for v in live {
            if let Some(ids) = self.value_carrier.get(v) {
                live_loans.extend(ids.iter().copied());
            }
            if let Some(ids) = self.slot_carrier.get(v) {
                live_loans.extend(ids.iter().copied());
            }
        }
        self.by_place.retain(|_, loans| {
            loans.retain(|l| live_loans.contains(&l.id));
            !loans.is_empty()
        });
        self.by_id.retain(|id, _| live_loans.contains(id));
        self.value_carrier.retain(|_, ids| {
            ids.retain(|id| live_loans.contains(id));
            !ids.is_empty()
        });
        self.slot_carrier.retain(|_, ids| {
            ids.retain(|id| live_loans.contains(id));
            !ids.is_empty()
        });
    }

    fn kill_arena(&mut self, exiting_depth: u32) {
        let dead: HashSet<LoanId> = self
            .by_id
            .values()
            .filter(|l| l.born_arena >= exiting_depth)
            .map(|l| l.id)
            .collect();
        self.by_place.retain(|_, loans| {
            loans.retain(|l| !dead.contains(&l.id));
            !loans.is_empty()
        });
        self.by_id.retain(|id, _| !dead.contains(id));
        self.value_carrier.retain(|_, ids| {
            ids.retain(|id| !dead.contains(id));
            !ids.is_empty()
        });
        self.slot_carrier.retain(|_, ids| {
            ids.retain(|id| !dead.contains(id));
            !ids.is_empty()
        });
    }

    fn contains_place(&self, place: ValueId) -> bool {
        self.by_place.get(&place).is_some_and(|v| !v.is_empty())
    }

    fn join(a: &LoanSet, b: &LoanSet) -> LoanSet {
        let mut out = a.clone();
        for loan in b.by_id.values() {
            if out.by_id.contains_key(&loan.id) {
                continue;
            }
            out.by_place
                .entry(loan.place)
                .or_default()
                .push(loan.clone());
            out.by_id.insert(loan.id, loan.clone());
        }
        for (v, ids) in &b.value_carrier {
            out.value_carrier
                .entry(*v)
                .or_default()
                .extend(ids.iter().copied());
        }
        for (v, ids) in &b.slot_carrier {
            out.slot_carrier
                .entry(*v)
                .or_default()
                .extend(ids.iter().copied());
        }
        out
    }

    fn fingerprint(&self) -> HashSet<LoanKey> {
        self.by_id.values().map(|l| l.key()).collect()
    }
}

#[derive(Debug, Clone)]
struct FlowState {
    arena_depth: u32,
    loans: LoanSet,
    /// Arena provenance of pointer-like values stored in local slots.
    arena_slots: HashMap<ValueId, u32>,
    /// Values whose arena backing storage has already been exited.
    expired: HashSet<ValueId>,
}

impl FlowState {
    fn fingerprint(
        &self,
    ) -> (
        u32,
        HashSet<LoanKey>,
        HashMap<ValueId, u32>,
        HashSet<ValueId>,
    ) {
        (
            self.arena_depth,
            self.loans.fingerprint(),
            self.arena_slots.clone(),
            self.expired.clone(),
        )
    }
}

pub fn borrow_check(file: &SourceFile, module: &Module, diagnostics: &mut Diagnostics) {
    let _ = file;
    for func in &module.functions {
        check_function(func, diagnostics);
    }
}

fn check_function(func: &Function, diagnostics: &mut Diagnostics) {
    if func.blocks.is_empty() {
        return;
    }

    let mut succs: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    let mut preds: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    for block in &func.blocks {
        let targets = terminator_targets(&block.terminator);
        succs.insert(block.id, targets.clone());
        for t in targets {
            preds.entry(t).or_default().push(block.id);
        }
    }

    let allocas: HashSet<ValueId> = func
        .blocks
        .iter()
        .flat_map(|block| &block.insts)
        .filter_map(|inst| {
            matches!(inst.kind, InstKind::Alloca)
                .then_some(inst.id)
                .flatten()
        })
        .collect();
    let live_in = compute_liveness(func, &succs, &preds, &allocas);
    let live_after = compute_live_after(func, &live_in, &succs, &allocas);
    let block_map: HashMap<BlockId, &crate::ir::BasicBlock> =
        func.blocks.iter().map(|b| (b.id, b)).collect();

    let entry = func.blocks[0].id;
    let mut entry_state: HashMap<BlockId, FlowState> = HashMap::new();
    let mut work: VecDeque<(BlockId, FlowState)> = VecDeque::new();
    work.push_back((
        entry,
        FlowState {
            arena_depth: 0,
            loans: LoanSet::default(),
            arena_slots: HashMap::new(),
            expired: HashSet::new(),
        },
    ));

    let mut place_root: HashMap<ValueId, ValueId> = HashMap::new();
    let mut loaded_from: HashMap<ValueId, ValueId> = HashMap::new();
    let mut ref_born_arena: HashMap<ValueId, u32> = HashMap::new();
    let mut reported: HashSet<(u32, u32, &'static str)> = HashSet::new();

    while let Some((bid, incoming)) = work.pop_front() {
        let joined = match entry_state.get(&bid) {
            Some(prev) => {
                let merged = FlowState {
                    arena_depth: incoming.arena_depth.max(prev.arena_depth),
                    loans: LoanSet::join(&prev.loans, &incoming.loans),
                    arena_slots: join_depths(&prev.arena_slots, &incoming.arena_slots),
                    expired: prev.expired.union(&incoming.expired).copied().collect(),
                };
                if merged.fingerprint() == prev.fingerprint()
                    && merged.loans.value_carrier == prev.loans.value_carrier
                    && merged.loans.slot_carrier == prev.loans.slot_carrier
                {
                    continue;
                }
                merged
            }
            None => incoming,
        };
        entry_state.insert(bid, joined.clone());

        let Some(block) = block_map.get(&bid) else {
            continue;
        };
        let after = live_after.get(&bid).map(|v| v.as_slice()).unwrap_or(&[]);

        let mut state = joined;
        if let Some(before) = after.first() {
            state.loans.retain_live(before);
        } else if let Some(li) = live_in.get(&bid) {
            state.loans.retain_live(li);
        }

        for (i, inst) in block.insts.iter().enumerate() {
            apply_inst(
                inst,
                &mut state,
                &mut place_root,
                &mut loaded_from,
                &mut ref_born_arena,
                &mut reported,
                diagnostics,
            );
            let live_after_inst = after.get(i + 1).cloned().unwrap_or_default();
            state.loans.retain_live(&live_after_inst);
        }

        if let Terminator::Return(Some(v)) = &block.terminator {
            let returned_loans = state.loans.carrier_loans(*v);
            let escapes_local = !returned_loans.is_empty();
            let escapes_arena = ref_born_arena.get(v).copied().unwrap_or(0) > 0
                || returned_loans.iter().any(|id| {
                    state
                        .loans
                        .by_id
                        .get(id)
                        .is_some_and(|loan| loan.born_arena > 0)
                });
            if escapes_local || escapes_arena {
                let message = if escapes_arena {
                    "cannot return a reference that may point into a local arena"
                } else {
                    "cannot return a reference to local storage"
                };
                report_once(
                    &mut reported,
                    func.span,
                    "E0403",
                    diagnostics,
                    Diagnostic::error_at_code(
                        func.span,
                        DiagnosticCode::E0403,
                        message,
                    )
                    .note(
                        "local storage is destroyed when the function or its enclosing arena ends",
                    )
                    .help(
                        "return an owned value or a reference received from the caller instead",
                    ),
                );
            }
        }

        for s in succs.get(&bid).into_iter().flatten() {
            work.push_back((*s, state.clone()));
        }
    }
}

fn apply_inst(
    inst: &Inst,
    state: &mut FlowState,
    place_root: &mut HashMap<ValueId, ValueId>,
    loaded_from: &mut HashMap<ValueId, ValueId>,
    ref_born_arena: &mut HashMap<ValueId, u32>,
    reported: &mut HashSet<(u32, u32, &'static str)>,
    diagnostics: &mut Diagnostics,
) {
    match &inst.kind {
        InstKind::ArenaEnter => {
            state.arena_depth = state.arena_depth.saturating_add(1);
        }
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
                    .help(
                        "every `arena:` block inserts a matching exit; do not emit unbalanced arena exits",
                    ),
                );
            } else {
                let exiting = state.arena_depth;
                state.loans.kill_arena(exiting);
                let expired_slots: Vec<ValueId> = state
                    .arena_slots
                    .iter()
                    .filter_map(|(slot, depth)| (*depth >= exiting).then_some(*slot))
                    .collect();
                for slot in expired_slots {
                    state.expired.insert(slot);
                    state.arena_slots.remove(&slot);
                }
                let expired_vals: Vec<ValueId> = ref_born_arena
                    .iter()
                    .filter_map(|(value, depth)| (*depth >= exiting).then_some(*value))
                    .collect();
                for value in expired_vals {
                    state.expired.insert(value);
                }
                state.arena_depth -= 1;
            }
        }
        InstKind::Ref {
            mutable,
            place,
            alias_root,
        } => {
            check_expired_operands(&[*place], state, reported, diagnostics, inst.span);
            let parent_loans = state.loans.carrier_loans(*place);
            let loan_place = if !parent_loans.is_empty() {
                parent_loans
                    .iter()
                    .filter_map(|id| state.loans.by_id.get(id).map(|l| l.place))
                    .next()
                    .unwrap_or_else(|| resolve_root(place_root, alias_root.unwrap_or(*place)))
            } else {
                resolve_root(place_root, alias_root.unwrap_or(*place))
            };
            if let Some(id) = inst.id {
                if !parent_loans.is_empty() {
                    // Reborrow: transfer/share the parent loan instead of minting a
                    // second exclusive permission to a temporary.
                    if *mutable {
                        for parent in &parent_loans {
                            if let Some(prev) = state.loans.by_id.get(parent).cloned() {
                                if prev.mutable {
                                    state.loans.by_place.retain(|_, loans| {
                                        loans.retain(|l| l.id != prev.id);
                                        !loans.is_empty()
                                    });
                                    state.loans.by_id.remove(&prev.id);
                                    state.loans.value_carrier.retain(|_, ids| {
                                        ids.retain(|l| *l != prev.id);
                                        !ids.is_empty()
                                    });
                                    state.loans.slot_carrier.retain(|_, ids| {
                                        ids.retain(|l| *l != prev.id);
                                        !ids.is_empty()
                                    });
                                    state.loans.insert(
                                        Loan {
                                            id: LoanId(id.0),
                                            place: loan_place,
                                            mutable: true,
                                            span: inst.span,
                                            born_arena: prev.born_arena,
                                        },
                                        id,
                                    );
                                } else {
                                    report_once(
                                        reported,
                                        inst.span,
                                        "E0400",
                                        diagnostics,
                                        Diagnostic::error_at_code(
                                            inst.span,
                                            DiagnosticCode::E0400,
                                            "cannot reborrow as mutable (`&mut`) from a shared borrow",
                                        )
                                        .help("create a mutable borrow of the original place instead"),
                                    );
                                }
                            }
                        }
                    } else {
                        state.loans.bind_value_set(id, &parent_loans);
                    }
                } else {
                    let existing = state.loans.loans_on(loan_place);
                    let conflict = if *mutable {
                        existing.first()
                    } else {
                        existing.iter().find(|l| l.mutable)
                    };
                    if let Some(prev) = conflict.cloned() {
                        let kind = if *mutable {
                            "mutable (`&mut`)"
                        } else {
                            "shared (`&`)"
                        };
                        let prev_kind = if prev.mutable {
                            "mutable (`&mut`)"
                        } else {
                            "shared (`&`)"
                        };
                        report_once(
                            reported,
                            inst.span,
                            "E0400",
                            diagnostics,
                            Diagnostic::error_at_code(
                                inst.span,
                                DiagnosticCode::E0400,
                                format!(
                                    "cannot create a {kind} borrow because the value is already borrowed as {prev_kind}"
                                ),
                            )
                            .note(
                                "Kroa allows either many shared borrows, or exactly one mutable borrow, never both",
                            )
                            .help(
                                "end the previous borrow before creating a new conflicting one (narrow the borrow scope)",
                            ),
                        );
                        report_once(
                            reported,
                            prev.span,
                            "E0400-prev",
                            diagnostics,
                            Diagnostic::error_at_code(
                                prev.span,
                                DiagnosticCode::E0400,
                                "previous borrow occurs here",
                            )
                            .help("this borrow is still active at the conflicting site"),
                        );
                    } else {
                        state.loans.insert(
                            Loan {
                                id: LoanId(id.0),
                                place: loan_place,
                                mutable: *mutable,
                                span: inst.span,
                                born_arena: state.arena_depth,
                            },
                            id,
                        );
                    }
                }
                ref_born_arena.insert(id, state.arena_depth);
            }
        }
        InstKind::Store { ptr, value } => {
            check_expired_operands(&[*ptr, *value], state, reported, diagnostics, inst.span);
            let through = state.loans.carrier_loans(*ptr);
            if through.is_empty() {
                let root = resolve_root(place_root, *ptr);
                if !state.loans.loans_on(root).is_empty() {
                    report_once(
                        reported,
                        inst.span,
                        "E0401",
                        diagnostics,
                        Diagnostic::error_at_code(
                            inst.span,
                            DiagnosticCode::E0401,
                            "cannot assign directly to a value while it is borrowed",
                        )
                        .note(
                            "an active borrow requires access to go through its reference carrier",
                        )
                        .help(
                            "finish using the borrow first, or mutate through the active `&mut` reference",
                        ),
                    );
                }
                let carried = state.loans.carrier_loans(*value);
                state.loans.replace_slot(root, carried);
                if let Some(depth) = ref_born_arena.get(value).copied().filter(|d| *d > 0) {
                    state.arena_slots.insert(root, depth);
                } else {
                    state.arena_slots.remove(&root);
                }
            } else {
                // Store through a reference: update the pointee slot when the
                // pointee itself stores reference/provenance data.
                let carried = state.loans.carrier_loans(*value);
                for loan_id in &through {
                    if let Some(loan) = state.loans.by_id.get(loan_id).cloned() {
                        state.loans.replace_slot(loan.place, carried.clone());
                        if let Some(depth) = ref_born_arena.get(value).copied().filter(|d| *d > 0) {
                            state.arena_slots.insert(loan.place, depth);
                        } else {
                            state.arena_slots.remove(&loan.place);
                        }
                    }
                }
            }
        }
        InstKind::Load { ptr } => {
            check_expired_operands(&[*ptr], state, reported, diagnostics, inst.span);
            if let Some(id) = inst.id {
                let root = resolve_root(place_root, *ptr);
                if state.expired.contains(&root) {
                    state.expired.insert(id);
                }
                if state.loans.loans_on(root).iter().any(|loan| loan.mutable) {
                    report_once(
                        reported,
                        inst.span,
                        "E0401-read",
                        diagnostics,
                        Diagnostic::error_at_code(
                            inst.span,
                            DiagnosticCode::E0401,
                            "cannot read a value directly while it is mutably borrowed",
                        )
                        .note(
                            "an exclusive `&mut` loan requires access to go through that reference",
                        )
                        .help(
                            "read through the mutable reference, or finish using it before reading the original value",
                        ),
                    );
                }
                let carried = state
                    .loans
                    .slot_carrier
                    .get(&root)
                    .cloned()
                    .unwrap_or_default();
                state.loans.bind_value_set(id, &carried);
                if let Some(depth) = carried
                    .iter()
                    .filter_map(|loan| state.loans.by_id.get(loan).map(|l| l.born_arena))
                    .max()
                {
                    ref_born_arena.insert(id, depth);
                }
                if let Some(depth) = state.arena_slots.get(&root).copied() {
                    ref_born_arena.insert(id, depth);
                }
                // Only a load of the whole place can later count as moving that place.
                // Element/field projections are not moves of the root.
                if *ptr == root {
                    loaded_from.insert(id, root);
                }
            }
        }
        InstKind::Deref { ptr } => {
            check_expired_operands(&[*ptr], state, reported, diagnostics, inst.span);
        }
        InstKind::ElemPtr { base, .. } => {
            check_expired_operands(&[*base], state, reported, diagnostics, inst.span);
            if let Some(id) = inst.id {
                let through = state.loans.carrier_loans(*base);
                if let Some(place) = through
                    .iter()
                    .filter_map(|loan| state.loans.by_id.get(loan).map(|l| l.place))
                    .next()
                {
                    place_root.insert(id, place);
                } else {
                    place_root.insert(id, resolve_root(place_root, *base));
                }
            }
        }
        InstKind::Move { value } => {
            check_expired_operands(&[*value], state, reported, diagnostics, inst.span);
            reject_move_of_borrowed(
                *value,
                inst.span,
                state,
                place_root,
                loaded_from,
                reported,
                diagnostics,
            );
            if let Some(id) = inst.id {
                let carried = state.loans.carrier_loans(*value);
                state.loans.bind_value_set(id, &carried);
                if let Some(depth) = ref_born_arena.get(value).copied() {
                    ref_born_arena.insert(id, depth);
                }
                if state.expired.contains(value) {
                    state.expired.insert(id);
                }
            }
        }
        InstKind::Call { args, .. } => {
            check_expired_operands(args, state, reported, diagnostics, inst.span);
            for arg in args {
                reject_move_of_borrowed(
                    *arg,
                    inst.span,
                    state,
                    place_root,
                    loaded_from,
                    reported,
                    diagnostics,
                );
            }
            if let Some(id) = inst.id {
                propagate_all_carriers(id, args, state, ref_born_arena);
                if is_escape_capable(&inst.ty) {
                    if let Some(depth) = args
                        .iter()
                        .filter_map(|arg| ref_born_arena.get(arg).copied())
                        .max()
                    {
                        ref_born_arena.insert(id, depth);
                    }
                }
            }
        }
        InstKind::StructAgg { fields, .. }
        | InstKind::ArrayAgg { elems: fields }
        | InstKind::EnumConstruct { fields, .. } => {
            check_expired_operands(fields, state, reported, diagnostics, inst.span);
            if let Some(id) = inst.id {
                propagate_all_carriers(id, fields, state, ref_born_arena);
                if is_escape_capable(&inst.ty) {
                    if let Some(depth) = fields
                        .iter()
                        .filter_map(|field| ref_born_arena.get(field).copied())
                        .max()
                    {
                        ref_born_arena.insert(id, depth);
                    }
                }
            }
        }
        InstKind::ExtractField { agg, .. } | InstKind::EnumField { value: agg, .. } => {
            check_expired_operands(&[*agg], state, reported, diagnostics, inst.span);
            if let Some(id) = inst.id {
                let carried = state.loans.carrier_loans(*agg);
                state.loans.bind_value_set(id, &carried);
                if let Some(depth) = ref_born_arena.get(agg).copied() {
                    if is_escape_capable(&inst.ty) {
                        ref_born_arena.insert(id, depth);
                    }
                }
                if state.expired.contains(agg) {
                    state.expired.insert(id);
                }
            }
        }
        InstKind::Alloca => {
            if let Some(id) = inst.id {
                place_root.insert(id, id);
            }
        }
        InstKind::ToCString { value } => {
            check_expired_operands(&[*value], state, reported, diagnostics, inst.span);
            if let Some(id) = inst.id {
                ref_born_arena.insert(id, state.arena_depth);
            }
        }
        InstKind::ArenaAlloc { nbytes } => {
            check_expired_operands(&[*nbytes], state, reported, diagnostics, inst.span);
            if let Some(id) = inst.id {
                ref_born_arena.insert(id, state.arena_depth);
            }
        }
        _ => {
            let ops = inst_operands(inst);
            check_expired_operands(&ops, state, reported, diagnostics, inst.span);
            if let Some(id) = inst.id {
                if is_escape_capable(&inst.ty) {
                    propagate_all_carriers(id, &ops, state, ref_born_arena);
                    if let Some(depth) = ops
                        .iter()
                        .filter_map(|op| ref_born_arena.get(op).copied())
                        .max()
                    {
                        ref_born_arena.insert(id, depth);
                    }
                }
            }
        }
    }
}

fn is_escape_capable(ty: &Type) -> bool {
    match ty {
        Type::Ref { .. } | Type::CString | Type::Str | Type::Slice(_) => true,
        Type::Array { element, .. } => is_escape_capable(element),
        Type::Struct(_) | Type::Enum(_) | Type::Named(_) => true,
        _ => false,
    }
}

fn propagate_all_carriers(
    id: ValueId,
    ops: &[ValueId],
    state: &mut FlowState,
    ref_born_arena: &mut HashMap<ValueId, u32>,
) {
    let mut all = HashSet::new();
    for op in ops {
        all.extend(state.loans.carrier_loans(*op));
        if state.expired.contains(op) {
            state.expired.insert(id);
        }
    }
    state.loans.bind_value_set(id, &all);
    if let Some(depth) = ops
        .iter()
        .filter_map(|op| ref_born_arena.get(op).copied())
        .max()
    {
        let _ = depth;
    }
}

fn reject_move_of_borrowed(
    value: ValueId,
    span: Span,
    state: &FlowState,
    place_root: &HashMap<ValueId, ValueId>,
    loaded_from: &HashMap<ValueId, ValueId>,
    reported: &mut HashSet<(u32, u32, &'static str)>,
    diagnostics: &mut Diagnostics,
) {
    let place = loaded_from
        .get(&value)
        .copied()
        .unwrap_or_else(|| resolve_root(place_root, value));
    if state.loans.contains_place(place) {
        report_once(
            reported,
            span,
            "E0402",
            diagnostics,
            Diagnostic::error_at_code(
                span,
                DiagnosticCode::E0402,
                "cannot move a value while it is borrowed",
            )
            .note("moving would invalidate existing references to this place")
            .help("drop or stop using all borrows before moving the value"),
        );
    }
}

fn check_expired_operands(
    ops: &[ValueId],
    state: &FlowState,
    reported: &mut HashSet<(u32, u32, &'static str)>,
    diagnostics: &mut Diagnostics,
    span: Span,
) {
    if ops.iter().any(|op| state.expired.contains(op)) {
        report_once(
            reported,
            span,
            "E0403-use",
            diagnostics,
            Diagnostic::error_at_code(
                span,
                DiagnosticCode::E0403,
                "use of a value whose arena backing storage has ended",
            )
            .note(
                "arena memory is freed at `arena` exit, including paths that leave the block early",
            )
            .help("keep uses of arena-backed values inside the `arena:` block"),
        );
    }
}

fn join_depths(a: &HashMap<ValueId, u32>, b: &HashMap<ValueId, u32>) -> HashMap<ValueId, u32> {
    let mut out = a.clone();
    for (value, depth) in b {
        out.entry(*value)
            .and_modify(|known| *known = (*known).max(*depth))
            .or_insert(*depth);
    }
    out
}

fn report_once(
    reported: &mut HashSet<(u32, u32, &'static str)>,
    span: Span,
    tag: &'static str,
    diagnostics: &mut Diagnostics,
    diag: Diagnostic,
) {
    let key = (span.start as u32, span.end as u32, tag);
    if reported.insert(key) {
        diagnostics.push(diag);
    }
}

fn resolve_root(place_root: &HashMap<ValueId, ValueId>, v: ValueId) -> ValueId {
    let mut cur = v;
    let mut guard = 0;
    while let Some(p) = place_root.get(&cur) {
        if *p == cur {
            break;
        }
        cur = *p;
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    cur
}

fn terminator_targets(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Jump(b) => vec![*b],
        Terminator::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        Terminator::Switch { cases, default, .. } => {
            let mut t: Vec<_> = cases.iter().map(|(_, b)| *b).collect();
            t.push(*default);
            t
        }
        Terminator::Return(_) | Terminator::Unreachable => vec![],
    }
}

fn compute_liveness(
    func: &Function,
    succs: &HashMap<BlockId, Vec<BlockId>>,
    preds: &HashMap<BlockId, Vec<BlockId>>,
    allocas: &HashSet<ValueId>,
) -> HashMap<BlockId, HashSet<ValueId>> {
    let mut live_in: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
    let mut work: VecDeque<BlockId> = func.blocks.iter().map(|b| b.id).collect();

    while let Some(bid) = work.pop_front() {
        let Some(block) = func.blocks.iter().find(|b| b.id == bid) else {
            continue;
        };
        let mut live: HashSet<ValueId> = HashSet::new();
        for s in succs.get(&bid).into_iter().flatten() {
            if let Some(si) = live_in.get(s) {
                live.extend(si.iter().copied());
            }
        }
        for v in terminator_operands(&block.terminator) {
            live.insert(v);
        }
        for inst in block.insts.iter().rev() {
            if let Some(id) = inst.id {
                live.remove(&id);
            }
            if let InstKind::Store { ptr, .. } = &inst.kind {
                if allocas.contains(ptr) {
                    // A store defines the slot's new contents. It does not read
                    // the old reference that the slot used to carry.
                    live.remove(ptr);
                }
            }
            for op in loan_operands(inst, allocas) {
                live.insert(op);
            }
        }
        let changed = live_in.get(&bid).is_none_or(|prev| prev != &live);
        if changed {
            live_in.insert(bid, live);
            for p in preds.get(&bid).into_iter().flatten() {
                work.push_back(*p);
            }
        }
    }
    live_in
}

/// `live_after[block][i]` = values live immediately before instruction `i`
/// (index 0 = block entry). Length is `insts.len() + 1` (after last inst).
fn compute_live_after(
    func: &Function,
    live_in: &HashMap<BlockId, HashSet<ValueId>>,
    succs: &HashMap<BlockId, Vec<BlockId>>,
    allocas: &HashSet<ValueId>,
) -> HashMap<BlockId, Vec<HashSet<ValueId>>> {
    let mut out = HashMap::new();
    for block in &func.blocks {
        let mut live: HashSet<ValueId> = HashSet::new();
        for s in succs.get(&block.id).into_iter().flatten() {
            if let Some(si) = live_in.get(s) {
                live.extend(si.iter().copied());
            }
        }
        for v in terminator_operands(&block.terminator) {
            live.insert(v);
        }
        let mut points: Vec<HashSet<ValueId>> = Vec::with_capacity(block.insts.len() + 1);
        points.push(live.clone());
        for inst in block.insts.iter().rev() {
            if let Some(id) = inst.id {
                live.remove(&id);
            }
            if let InstKind::Store { ptr, .. } = &inst.kind {
                if allocas.contains(ptr) {
                    live.remove(ptr);
                }
            }
            for op in loan_operands(inst, allocas) {
                live.insert(op);
            }
            points.push(live.clone());
        }
        points.reverse();
        out.insert(block.id, points);
    }
    out
}

fn loan_operands(inst: &Inst, allocas: &HashSet<ValueId>) -> Vec<ValueId> {
    match &inst.kind {
        InstKind::Store { ptr, value } => {
            let mut operands = vec![*value];
            if !allocas.contains(ptr) {
                operands.push(*ptr);
            }
            operands
        }
        InstKind::Ref { .. } => vec![],
        _ => inst_operands(inst),
    }
}

fn inst_operands(inst: &Inst) -> Vec<ValueId> {
    match &inst.kind {
        InstKind::Nop
        | InstKind::ConstI64(_)
        | InstKind::ConstF64(_)
        | InstKind::ConstBool(_)
        | InstKind::ConstStr(_)
        | InstKind::Alloca
        | InstKind::ArenaEnter
        | InstKind::ArenaExit => vec![],
        InstKind::Load { ptr } => vec![*ptr],
        InstKind::Store { ptr, value } => vec![*ptr, *value],
        InstKind::Binary { left, right, .. } => vec![*left, *right],
        InstKind::Unary { value, .. } => vec![*value],
        InstKind::Call { args, .. } => args.clone(),
        InstKind::StructAgg { fields, .. } => fields.clone(),
        InstKind::ExtractField { agg, .. } => vec![*agg],
        InstKind::InsertField { agg, value, .. } => vec![*agg, *value],
        InstKind::Cast { value, .. } => vec![*value],
        InstKind::Move { value } => vec![*value],
        InstKind::Ref { place, .. } => vec![*place],
        InstKind::Deref { ptr } => vec![*ptr],
        InstKind::ArenaAlloc { nbytes } => vec![*nbytes],
        InstKind::ToCString { value } => vec![*value],
        InstKind::ArrayAgg { elems } => elems.clone(),
        InstKind::Len { value } => vec![*value],
        InstKind::BoundsCheck { index, len } => vec![*index, *len],
        InstKind::SliceBoundsCheck { start, end, len } => vec![*start, *end, *len],
        InstKind::ElemPtr { base, index } => vec![*base, *index],
        InstKind::SliceFrom { base, start, end } => vec![*base, *start, *end],
        InstKind::EnumConstruct { fields, .. } => fields.clone(),
        InstKind::EnumTag { value } => vec![*value],
        InstKind::EnumField { value, .. } => vec![*value],
    }
}

fn terminator_operands(term: &Terminator) -> Vec<ValueId> {
    match term {
        Terminator::Return(Some(v)) => vec![*v],
        Terminator::Return(None) | Terminator::Jump(_) | Terminator::Unreachable => vec![],
        Terminator::Branch { cond, .. } => vec![*cond],
        Terminator::Switch { discr, .. } => vec![*discr],
    }
}
