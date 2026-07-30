# Language guide

This guide explains Kroa step by step. Every example uses English syntax only.

## Variables

Create a value with `let`:

```kroa
fn main() -> i64:
    let x = 10
    print_i64(x)
    return 0
```

Use `let mut` when you need to change the value later:

```kroa
fn main() -> i64:
    let mut total = 0
    total = total + 5
    print_i64(total)
    return 0
```

## Types

Kroa checks types before your program runs.

Common types today:

| Type | Meaning |
|------|---------|
| `i64` | 64-bit integer |
| `f64` | 64-bit floating point |
| `bool` | `true` or `false` |
| `unit` | “no useful value” (like void) |
| `str` | UTF-8 text (pointer + length) |

You can annotate types, or let Kroa infer them locally:

```kroa
fn main() -> i64:
    let a: i64 = 1
    let b = 2
    return a + b
```

Convert numbers explicitly with `as`:

```kroa
fn main() -> i64:
    let x: f64 = 3.5
    let y = x as i64
    print_i64(y)
    return 0
```

## Operators

Arithmetic: `+ - * / %`  
Compare: `== != < <= > >=`  
Logic (canonical only): `and`, `or`, `not`

Do not write `&&`, `||`, or bare `!`. The compiler rejects those forms so tools and agents have one unambiguous spelling.

```kroa
fn main() -> i64:
    print_bool(3 < 10 and not false)
    return 0
```

Expected output:

```text
true
```
## Conditions

```kroa
fn main() -> i64:
    let n = 7
    if n > 5:
        print_i64(1)
    else:
        print_i64(0)
    return 0
```

## Loops

```kroa
fn main() -> i64:
    let mut i = 0
    while i < 3:
        print_i64(i)
        i = i + 1
    return 0
```

## Functions

```kroa
fn add(a: i64, b: i64) -> i64:
    return a + b

fn main() -> i64:
    print_i64(add(2, 3))
    return 0
```

## Structs

A struct groups named fields:

```kroa
struct Point:
    x: i64
    y: i64

fn main() -> i64:
    let p = Point { x: 3, y: 4 }
    print_i64(p.x + p.y)
    return 0
```

For C-compatible layout, declare `struct c Name`.

## Arenas

An arena is a memory block that frees everything at once when the block ends:

```kroa
fn main() -> i64:
    arena:
        let s = "hello"
        print_str(s)
    return 0
```

This is useful for temporary data and for converting strings for C.

## References

`&T` borrows a value without copying it. `&mut T` borrows it for modification.

```kroa
fn add_one(x: &mut i64) -> unit:
    *x = *x + 1
    return

fn main() -> i64:
    let mut n = 41
    add_one(&mut n)
    print_i64(n)
    return 0
```

Rules in short:

- A reference cannot outlive the value it points to.
- You cannot have a mutable borrow together with other borrows of the same value.
- Loans end at the last use of the reference (NLL-lite), so sequential non-overlapping `&mut` borrows are allowed.

See the [borrow checker technical guide](borrow-checker.md) for the complete
CFG, liveness, join, place, carrier, and arena-provenance algorithm.

## Strings and C

Kroa `str` is not a C `char*`. Convert explicitly:

```kroa
extern "C" fn puts(s: c_string) -> i64

fn main() -> i64:
    arena:
        unsafe:
            let s = "hi"
            let c = to_c_string(s)
            # call C with c here
            print_i64(1)
    return 0
```

`to_c_string` rejects strings that contain an interior NUL byte.

## Calling C

Declare C functions with `extern "C"`:

```kroa
extern "C" fn kroa_add(a: i64, b: i64) -> i64
```

Link the C code when building:

```bash
kroa run app.kroa --link mylib.c
```

Calls that pass advanced C types (strings/structs) belong inside `unsafe`.
