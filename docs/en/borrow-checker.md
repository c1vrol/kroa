# NLL-lite borrow checker: an understandable technical guide

This guide explains how Kroa's borrow checker works internally. It is written so that a twelve-year-old can build a correct mental model while also learning the technical terms used by compiler engineers.

The main implementation lives in:

- `src/borrowcheck.rs`: loan analysis, dataflow, and diagnostics.
- `src/ir.rs`: KIR, basic blocks, and instruction definitions.
- `src/lower.rs`: conversion to KIR and correct arena-exit ordering.
- `src/typecheck.rs`: decides when borrow checking is required.
- `tests/borrow_check.rs`: programs that must be accepted or rejected.

## 1. The problem

Imagine that `x` is a notebook.

- `&x` is permission to read it.
- `&mut x` is the only key that permits changing it.
- A **loan** records who has permission.
- A **place** is the object being borrowed.

Kroa permits many readers or one exclusive writer, but never a writer while another reader or writer exists. The technical name is **shared XOR mutable**.

```kroa
let mut x = 1
let a = &x
let b = &x
print_i64(*a + *b)  # two readers: valid
```

```kroa
let mut x = 1
let a = &x
let b = &mut x      # E0400: reader and writer overlap
print_i64(*a + *b)
```

## 2. NLL-lite

**NLL** means *Non-Lexical Lifetimes*.

A lexical lifetime would last until the enclosing indented block ends, even when the reference is never used again. NLL ends the loan at its **actual last use**.

```kroa
let mut x = 1
let first = &mut x
*first = 2          # last use of first
let second = &mut x # valid with NLL-lite
*second = 3
```

It is called **NLL-lite** because it implements the essential behavior with liveness and control-flow analysis without attempting to reproduce Rust's complete region and lifetime system.

A technically precise description is:

> Kroa runs backward liveness analysis over the CFG. A loan remains active while a live SSA value or local slot carries it. Forward loan analysis uses that information to kill the loan after its last use.

## 3. The road map: CFG

**CFG** means *Control-Flow Graph*.

Programs do not always run in a straight line:

- `if` splits the route;
- `match` can split it many ways;
- `while` creates a route back to an earlier point;
- `return` ends the route.

The compiler divides a function into **basic blocks**. A basic block is an ordered instruction list ending in:

- `Jump`: continue in another block;
- `Branch`: choose between two blocks;
- `Switch`: choose among several blocks;
- `Return`: leave the function;
- `Unreachable`: there is no valid continuation.

`terminator_targets` finds each block's successors. Those edges are also reversed to find predecessors.

In simple words, the checker draws the road map before tracking which permissions travel along each road.

## 4. KIR, SSA, values, and slots

Kroa analyzes **KIR**, the *Kroa Intermediate Representation*.

In KIR:

- `ValueId` identifies a temporary value;
- `BlockId` identifies a basic block;
- `Alloca` reserves a local **slot**;
- `Store` writes a slot;
- `Load` reads a slot;
- `Ref` creates `&T` or `&mut T`;
- `Move` transfers a value;
- `ArenaEnter` and `ArenaExit` delimit an arena.

KIR is **SSA-like**. SSA means *Static Single Assignment*: each temporary receives a fresh identity and is not changed. Mutable source locals are represented by slots plus `Store` and `Load`.

```text
%1 = alloca       // x's slot
%2 = const 1
store %1, %2
%3 = ref %1       // borrow x
```

The value/slot distinction matters. A reference can start as `%3`, be stored in `r`, later be loaded as `%8`, and still carry the same logical loan.

## 5. Loan data structures

### `LoanId`

This is a loan's stable identity. It is derived from the `ValueId` of the creating `Ref` instruction. Revisiting a block in a loop therefore does not mint endless analysis identities.

### `Loan`

It stores:

- `id`: loan identity;
- `place`: borrowed root;
- `mutable`: `true` for `&mut`, `false` for `&`;
- `span`: source location for diagnostics;
- `born_arena`: arena depth at creation.

### `LoanKey`

This comparable summary is used to determine whether fixed-point state changed.

### `LoanSet`

This is the active permission ledger. It has four indexes:

- `by_place`: loans grouped by borrowed place;
- `by_id`: direct identity lookup;
- `value_carrier`: loan sets carried by SSA values;
- `slot_carrier`: loan sets stored in local slots.

Carriers hold **sets** of `LoanId`, not one identity. This is required at a branch join:

```kroa
let mut r = &x
if condition:
    r = &y
```

After the join, `r` may carry either loan. Keeping both is a conservative **may-analysis**: if a loan may exist on any incoming path, the merged state retains it.

## 6. Places and alias roots

Different expressions can point to the same memory. This is **aliasing**.

```kroa
let mut values: [i64; 4] = [1, 2, 3, 4]
let left = &mut values[0..2]
let right = &mut values[2..4]
```

Kroa currently uses a deliberately conservative policy: every slice of one array has the same **alias root**, `values`. The loans are therefore considered overlapping even when the written ranges look separate.

`resolve_root` follows `place_root` links to the root place. `ElemPtr` inherits its base's root.

Useful terms:

- **place**: memory location that can be read, written, or borrowed;
- **projection**: a part of a place, such as an element;
- **alias root**: common root used to test possible overlap;
- **conservative aliasing**: rejecting some safe cases to avoid accepting an unsafe one.

## 7. Liveness and last use

**Liveness** answers:

> Can this value still be used from this program point?

It is calculated backward:

1. start at the end of a block;
2. uses make a value live;
3. a new definition kills the old version;
4. successor information is joined;
5. repeat until nothing changes.

That stable result is a **fixed point**.

### Slot-aware liveness

`Load slot` reads a slot's contents, so those contents become live.

`Store slot, new_value` replaces the old contents. Writing the box is not reading the old reference. The store therefore kills the old slot-content liveness.

This permits:

```kroa
let mut r = &mut x
*r = 2
r = &mut x  # the Store replaces the previous reference
```

`compute_liveness` computes block-entry liveness. `compute_live_after` computes liveness at every point between instructions. `loan_operands` distinguishes real reads from slot replacement.

## 8. Worklist and forward analysis

After liveness, `check_function` runs forward analysis.

A **worklist** is a queue of blocks waiting to be processed:

1. start at the entry block;
2. apply every instruction to the state;
3. send state copies to successors;
4. join states when roads meet;
5. revisit a block if the join added information;
6. stop at a fixed point.

`FlowState` carries:

- current arena depth;
- active loans;
- arena provenance stored in local slots.

The join is conservative. At a branch, “may be active” is treated as active. This prevents an unsafe path from being hidden by a safe path.

## 9. Important instruction transfer functions

### `Ref`

It resolves the place root, checks active loans, applies shared-versus-mutable compatibility, and records the new loan and SSA carrier. Conflicts emit `E0400`.

### `Store`

It rejects direct writes to a place with any active loan (`E0401`). A write
through the carrier of an active `&mut` is valid; writing the original place
behind that exclusive loan is not. It then replaces the destination slot's
carrier set and arena provenance.

### `Load`

It rejects direct reads of the original place while an exclusive `&mut` is
active. It then remembers the source root and copies the slot's loans and
provenance to the new SSA value.

### `Move`

It finds the actual source place. A live loan causes `E0402`, because moving the value would invalidate existing references. Reference and arena provenance is copied to the result.

### `ElemPtr`

It links an element pointer to the root of its array or slice.

### `ArenaEnter` and `ArenaExit`

Entering increments arena depth. Exiting decrements it and ends loans born in that arena. An unmatched exit emits `E0404`.

### `ToCString` and `ArenaAlloc`

Their results receive the current arena's **provenance**: information describing where a pointer came from and which storage keeps it valid.

A `c_string` is not `&T`, but it is still backed by arena memory. `needs_borrow_check` therefore runs for programs containing references or arenas.

### `Return`

The return expression is evaluated before `ArenaExit` is emitted, preserving its true provenance.

Kroa rejects:

- returning a newly created reference to local storage;
- returning a pointer that may depend on a local arena.

It permits returning a caller-provided reference:

```kroa
fn identity(x: &i64) -> &i64:
    return x
```

Escapes emit `E0403`.

## 10. Killing loans

After each instruction, `retain_live` keeps a loan only when some live carrier still contains it.

This does not free runtime memory. It only removes a permission from the compiler's static model.

The technical wording is:

> The transfer function applies the instruction effect and then restricts the loan set to carriers present in the program point's live-after set.

## 11. Diagnostics

- `E0400`: borrow conflict.
- `E0401`: write while shared-borrowed.
- `E0402`: move while borrowed.
- `E0403`: local reference or arena-backed pointer escapes.
- `E0404`: unbalanced arena entry/exit.

`report_once` suppresses duplicate reports caused by fixed-point revisits. Diagnostics include a root cause, source span, conceptual note, and actionable help.

## 12. Test coverage

`tests/borrow_check.rs` covers:

- truly overlapping mutable loans;
- slices sharing an array root;
- multiple shared loans;
- shared then mutable after last use;
- conflicts preserved through joins;
- branch-local loans dead at the join;
- replacing a reference stored in a slot;
- loop convergence;
- writes during a shared loan;
- local-reference escapes;
- valid return of a caller-provided reference;
- arena-backed `c_string` escapes.

## 13. Intentional limits

NLL-lite is not a complete Rust-style region system:

- slices from one array always overlap;
- calls that return references conservatively inherit loans from every borrowed argument;
- there are no explicit lifetime annotations;
- interprocedural analysis remains limited to that conservative call summary;
- joins prefer safety and may reject difficult valid cases;
- dynamic indexes are not proven mathematically distinct.

Mutable references are not `Copy`: `let b = a` moves an `&mut T`. Reborrows of the form `&mut *p` transfer the exclusive loan instead of inventing a temporary place.

This is not guessing. It is a deliberately defined conservative approximation.

## 14. Short explanation

A precise answer to “How does Kroa's borrow checker work?” is:

> The compiler lowers source code to SSA-like KIR, builds its CFG, and computes backward liveness to a fixed point. It then propagates loans forward over places normalized to alias roots. SSA values and local slots carry LoanId sets, joins perform conservative union, and each loan dies after its final live carrier. Arena provenance is propagated separately to prevent escapes. Conflicts emit E0400–E0404.

In simple words:

> The compiler draws every possible road through the program, tracks who owns each reading or writing permission, and removes permission just after its last use. If dangerous permissions could meet on any road, compilation stops.
