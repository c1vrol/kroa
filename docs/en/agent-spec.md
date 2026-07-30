# Kroa Agent Specification

> **Purpose:** paste this file into an AI agent system prompt or tool context when the agent must read, write, review, or auto-fix Kroa code.
>
> Language, keywords, diagnostics, and CLI output are English-only.

## 1. What Kroa is

Kroa is an ahead-of-time compiled language:

- Python-like indentation (spaces only; tabs are illegal)
- Static types with **local** inference inside functions
- Native codegen through LLVM
- Optional arenas and borrow checking for memory safety
- `extern "C"` for calling C

## 2. Canonical grammar (one form only)

Do **not** invent alternate spellings. Use exactly these forms:

| Intent | Canonical syntax | Forbidden |
|--------|------------------|-----------|
| Logical and | `and` | `&&` |
| Logical or | `or` | `\|\|` |
| Logical not | `not` | `!` (except `!=`) |
| Indentation | spaces | tabs (`\t`) |
| Mutation | `let mut x = ...` then `x = ...` | mutating a plain `let` |
| Borrow | `&x` / `&mut x` | raw pointers in safe Kroa |
| C string | `to_c_string(s)` inside `arena:` | passing `str` as `char*` |

### Minimal program

```kroa
fn main() -> i64:
    print_i64(40 + 2)
    return 0
```

### Functions, control flow, locals

```kroa
fn add(a: i64, b: i64) -> i64:
    return a + b

fn main() -> i64:
    let mut i = 0
    while i < 3:
        if i == 1 or i == 2:
            print_i64(add(i, 10))
        i = i + 1
    return 0
```

### Structs and arenas

```kroa
struct Point:
    x: i64
    y: i64

fn main() -> i64:
    let p = Point { x: 3, y: 4 }
    arena:
        let s = "hi"
        print_str(s)
    return p.x + p.y
```

### References

```kroa
fn bump(x: &mut i64) -> unit:
    *x = *x + 1
    return

fn main() -> i64:
    let mut n = 1
    bump(&mut n)
    return n
```

## 3. Types

Primitives: `i64`, `f64`, `bool`, `unit`, `str`  
FFI: `c_char`, `c_string`, `struct c Name`  
References: `&T`, `&mut T`

Inference is **local**:

- Resolve names from the current function’s scope stack only.
- Do not invent globals or implicit imports.
- Numeric conversions are explicit: `x as i64`, `y as f64`.

## 4. Memory and borrows (root causes)

1. **Shared XOR mutable:** many `&T`, or one `&mut T`, never both on the same place.
2. **No move while borrowed.**
3. **No assign through a place while it is shared-borrowed.**
4. **Local and arena lifetimes:** local storage dies when its function ends, and memory from `arena:` dies when the block ends (including early `return`). Return owned values or caller-provided references, never references to local storage or arena-backed pointers.
5. **NLL-lite:** a loan ends at the last use of the reference (including the local that stores it), not at the end of the enclosing lexical block. Sequential non-overlapping `&mut` borrows of the same place are allowed.

## 5. Diagnostics for auto-fix loops

Always compile with structured diagnostics when fixing code automatically:

```bash
kroa build file.kroa --message-format json
```

Each diagnostic is one JSON object (NDJSON) with fields:

| Field | Meaning |
|-------|---------|
| `severity` | `error` or `warning` |
| `code` | stable id, e.g. `E0301`, `E0400` |
| `message` | root-cause text |
| `file` | source path |
| `line`, `column` | 1-based start |
| `end_line`, `end_column` | 1-based end |
| `notes` | optional extra facts |
| `help` | optional concrete fix |

### Important codes

| Code | Meaning |
|------|---------|
| `E0100` | tab rejected |
| `E0101` | inconsistent indentation |
| `E0201` | non-canonical syntax (`&&`, `||`, `!`) |
| `E0300` | undefined name |
| `E0301` | type mismatch |
| `E0302` | assign to immutable |
| `E0303` | use/move after move |
| `E0304` | return type mismatch |
| `E0400` | borrow conflict |
| `E0401` | assign while borrowed |
| `E0402` | move while borrowed |
| `E0403` | local reference or arena-backed pointer escapes |
| `E0404` | arena enter/exit mismatch |
| `E0500` | FFI/unsafe boundary |

### Agent repair loop

1. Write or edit a `.kroa` file.
2. Run `kroa build path.kroa --message-format json`.
3. Parse each JSON line.
4. Apply the `help` / `message` at `file:line:column`.
5. Rebuild until exit code 0.

Human-readable errors use the same codes:

```text
error[E0304]: demo.kroa:2:5: return type mismatch: expected `i64`, found `bool`
```

## 6. Commands

```bash
kroa build file.kroa -o out
kroa run file.kroa
kroa emit-ir file.kroa
kroa emit-kir file.kroa
kroa build file.kroa --link lib.c
kroa build file.kroa --message-format json
```

Built-ins: `print_i64`, `print_f64`, `print_bool`, `print_str`, `to_c_string`.

## 7. Hard rules for agents

1. Prefer the smallest edit that clears diagnostics.
2. Never introduce `&&`, `||`, bare `!`, or tabs.
3. Keep changes inside one function when possible (local scopes).
4. Treat advanced `extern` calls as `unsafe` and prefer thin wrappers.
5. Do not claim GC or full Python dynamics — Kroa is statically typed and AOT-compiled.
