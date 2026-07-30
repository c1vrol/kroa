# Kroa examples

| File | Demonstrates |
|------|----------------|
| `hello.kroa` | First program |
| `factorial.kroa` | Recursion and `if` |
| `loop_sum.kroa` | `while` loops |
| `struct_point.kroa` | Structs |
| `arena_string.kroa` | Arenas and strings |
| `borrow_mut.kroa` | Mutable references |
| `array_slice.kroa` | Fixed arrays, indexing, and slices |
| `enum_result.kroa` | Enums with data and exhaustive `match` |
| `ffi_labs.kroa` | Scalar FFI |
| `ffi_add.kroa` | Linking a C file |
| `ffi_struct.kroa` | C-layout structs |
| `ffi_string.kroa` | `c_string` conversion |

Run:

```bash
kroa run examples/hello.kroa
kroa run examples/ffi_add.kroa --link examples/ffi/native_lib.c
```
