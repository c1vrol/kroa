# FFI guide

This guide explains how Kroa talks to C.

## Why FFI exists

C libraries are everywhere: operating systems, codecs, databases, drivers.  
FFI (Foreign Function Interface) lets Kroa call those libraries.

## Phase rules

- Scalar FFI (`i64`, `f64`, `bool`) is available early and is the simplest path.
- Strings and C structs are advanced: convert carefully and call them from `unsafe`.

## Declare a C function

```kroa
extern "C" fn kroa_add(a: i64, b: i64) -> i64
```

Then link the implementation:

```bash
kroa run app.kroa --link native_lib.c
```

## Structs with C layout

```kroa
struct c CPoint:
    x: i64
    y: i64
```

The `c` marker means “use a C-compatible memory layout”.

## Strings

Kroa `str` stores pointer + length (UTF-8).  
C usually wants a NUL-terminated `char*`.

Convert explicitly inside an arena:

```kroa
arena:
    unsafe:
        let s = "hello"
        let c = to_c_string(s)
        # pass `c` to an extern function that expects c_string
```

The converted buffer lives in the arena and is freed when the arena ends.

## Safety model

- Kroa checks Kroa code.
- Kroa cannot prove that a C library is correct.
- Therefore advanced extern calls are `unsafe`, and you should wrap them in small safe helpers when possible.
