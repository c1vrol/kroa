# Troubleshooting

Error messages from the compiler are always in English. This page explains common ones.

## Structured diagnostics for agents

```bash
kroa build file.kroa --message-format json
```

Each line is a JSON object with `severity`, `code`, `message`, `file`, `line`, `column`, optional `notes` and `help`.  
See [Agent specification](agent-spec.md).

## `tabs are not allowed; indent with spaces only`

Kroa rejects tab characters.

**Fix:** configure your editor to insert spaces, then re-indent the file.

## `expected indent` / `inconsistent indentation`

Your spaces do not line up with a previous indentation level.

**Fix:** use a consistent width (for example 4 spaces) for each nested block.

## `undefined variable`

You used a name that was not declared, or it is out of scope.

**Fix:** declare it with `let` before use, or check spelling.

## `cannot assign to immutable variable`

You tried to change a value created with `let` (without `mut`).

**Fix:** write `let mut name = ...` if mutation is intended.

## `type mismatch` / `return type mismatch`

A value has a different type than expected.

**Fix:** cast with `as` when converting numbers, or change the declared type.

## `extern function ... must be called inside unsafe`

Advanced FFI (strings/structs) is treated as unsafe because Kroa cannot verify C code.

**Fix:** wrap the call in:

```kroa
unsafe:
    ...
```

## `clang failed while linking native binary`

The LLVM IR was generated, but Clang could not produce an executable.

Common causes:

- `clang` is not on `PATH`
- a linked C file/library is missing
- an FFI signature does not match the C definition

**Fix:** verify `clang --version`, check `--link` paths, and compare `extern` signatures with your C headers.

## `string contains interior NUL; cannot convert to c_string`

`to_c_string` found a zero byte inside the text. C strings cannot represent that safely.

**Fix:** remove interior `\0` characters, or keep the data as Kroa `str`.

## Still stuck?

1. Run `kroa emit-ir file.kroa` and inspect the generated IR.
2. Run `kroa emit-kir file.kroa` to see Kroa IR before LLVM.
3. Compare with the examples in the `examples/` folder.
