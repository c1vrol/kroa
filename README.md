# Kroa

Kroa is a compiled programming language with Python-like indentation and native performance through LLVM.

**Current version:** Alpha-2.0.0 (`A-2.0.0`)  
**Cargo version:** `2.0.0-alpha`  
**Living project summary:** [`PROJECT_STATUS.md`](PROJECT_STATUS.md)  
**Changelog:** [`CHANGELOG.md`](CHANGELOG.md)

**Language, syntax, keywords, and error messages are English-only.**  
Documentation is available in English and Spanish.

- [English docs](docs/en/getting-started.md)
- [Documentación en español](docs/es/getting-started.md)
- [AI agent specification](docs/en/agent-spec.md) ([español](docs/es/agent-spec.md)) · [`AGENTS.md`](AGENTS.md)
- [NLL-lite borrow checker internals](docs/en/borrow-checker.md) ([explicación técnica en español](docs/es/borrow-checker.md))
- [Versioning and release process](docs/en/versioning.md) ([español](docs/es/versioning.md))

## What Kroa is

Kroa aims to feel easy to read, while compiling ahead-of-time to a native executable.

| Goal | How Kroa approaches it |
|------|-------------------------|
| Easy to read | Indentation-based syntax, clear keywords |
| Fast | Compiles to LLVM IR, then to native code |
| Safer memory | Arenas and NLL-lite borrow checking |
| Works with C | `extern "C"` foreign function interface |

## Requirements

- [Rust](https://rustup.rs/) (to build the compiler)
- [LLVM/Clang](https://llvm.org/) 18+ with `clang` on your `PATH`

## Build the compiler

```bash
cargo build --release
```

The binary is `target/release/kroa` (or `kroa.exe` on Windows).

## First program

Create `hello.kroa`:

```kroa
fn main() -> i64:
    print_i64(40 + 2)
    return 0
```

Run it:

```bash
kroa run hello.kroa
```

Or compile to an executable:

```bash
kroa build hello.kroa -o hello
./hello
```

Expected output:

```text
42
```

## Essential commands

```bash
kroa build file.kroa -o out          # compile to native binary
kroa run file.kroa                   # compile and run
kroa emit-ir file.kroa               # print LLVM IR
kroa emit-kir file.kroa              # print Kroa IR
kroa build file.kroa --link lib.c    # link extra C/object files
kroa build file.kroa -l m -L ./libs  # link system libraries
kroa build file.kroa --message-format json  # NDJSON diagnostics for agents
```

## AI-friendly workflow

Kroa is designed so agents can auto-fix code in a loop:

1. Prefer the canonical grammar (`and` / `or` / `not`, spaces only).
2. Compile with `--message-format json` to get parseable diagnostics (`file`, `line`, `column`, `code`, `help`).
3. Apply the smallest edit suggested by `help` / `message`, then rebuild.

See [`docs/en/agent-spec.md`](docs/en/agent-spec.md) for the full static agent context.

## Development vs production

| Environment | Branch | Purpose |
|-------------|--------|---------|
| Development | `develop` | Integration, CI checks, temporary artifacts |
| Production | `main` + tags `A-*` | Verified releases only |

See [`docs/en/versioning.md`](docs/en/versioning.md) for the full branching and release model.

## Project status

Kroa is an Alpha-stage language under active development. Alpha-2.0.0 adds:

1. Fixed arrays and safe slicing
2. Enums with `match` / `case`
3. NLL-lite borrow checking

See the language guide and project status for what works today.

## License

MIT
