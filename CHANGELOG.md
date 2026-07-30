# Changelog

All notable changes to Kroa are documented in this file.

The version format is `A-MAJOR.MINOR.PATCH` (Cargo: `MAJOR.MINOR.PATCH-alpha`).

## [A-2.0.0] - 2026-07-30

### Added

- Fixed arrays `[T; N]` and safe slices `&[T]` / `&mut [T]` with runtime bounds checks
- Enums and `match` / `case` pattern matching with exhaustiveness checking
- NLL-lite borrow checker: last-use loan ending, CFG joins, multi-loan sets, reborrows
- Escape analysis for local references and arena-backed `c_string` values
- Pedagogical borrow-checker documentation (English and Spanish)
- Professional branching model: `main` (production), `develop` (integration)
- CI, development-artifact, and production-release GitHub Actions workflows

### Changed

- Cargo package version is now `2.0.0-alpha`
- Mutable references (`&mut T`) are no longer `Copy`
- `to_c_string` requires an active `arena:` block

### Notes

Alpha-2.0.0 is still an Alpha language. The borrow checker is NLL-lite, not a full
Rust-style region system. Arrays/slices remain conservative on alias roots.

## [A-1.0.0] - 2026-07-30

### Added

- Native AOT compiler pipeline (lexer, parser, types, Kroa IR, LLVM, Clang)
- Static types with local inference, structs, arenas, and initial borrow checking
- C FFI (`extern "C"`, scalars, C-layout structs, strings)
- Human diagnostics and NDJSON agent diagnostics
- Canonical `and` / `or` / `not` grammar; tabs rejected
- Bilingual documentation, examples, and automated tests

### Reconstruction notice

`A-1.0.0` was reconstructed from the historical specification because no
recoverable Git history or original release archive existed. See
[`RECONSTRUCTION.md`](RECONSTRUCTION.md).
