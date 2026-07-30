# Reference

Short reference for Kroa syntax and tooling.

## Commands

| Command | Purpose |
|---------|---------|
| `kroa build <file> -o <out>` | Compile to a native binary |
| `kroa run <file>` | Compile and run |
| `kroa emit-ir <file>` | Print LLVM IR |
| `kroa emit-kir <file>` | Print Kroa IR |
| `kroa build file.kroa --message-format json` | Emit NDJSON diagnostics for agents |
| `--library` / `-l` | Link a library |
| `--library-path` / `-L` | Add a library search path |
| `--link <file>` | Link an extra object/C file |
| `--keep-temps` | Keep temporary files |

## Keywords

`fn`, `let`, `mut`, `if`, `else`, `while`, `return`, `true`, `false`, `extern`, `struct`, `arena`, `unsafe`, `as`, `and`, `or`, `not`

## Types

`i64`, `f64`, `bool`, `unit`, `str`, `c_char`, `c_string`, named structs, `&T`, `&mut T`

## Built-in functions

| Name | Description |
|------|-------------|
| `print_i64(x)` | Print an integer |
| `print_f64(x)` | Print a float |
| `print_bool(x)` | Print a boolean |
| `print_str(s)` | Print a Kroa string |
| `to_c_string(s)` | Convert `str` → `c_string` (arena-backed) |

## Indentation rules

- Use spaces only
- Tabs are an error
- Blocks start after `:` and an indented section

## File extension

`.kroa`
