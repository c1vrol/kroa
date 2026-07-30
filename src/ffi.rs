//! Foreign Function Interface helpers and validation notes.
//!
//! Phase 1: only scalar extern signatures (`i64`, `f64`, `bool`, `unit`).
//! Phase 4: `c_char`, `c_string`, and `struct c Name` layouts.
//!
//! Kroa `str` is UTF-8 `(pointer, length)` and is never passed to C as `char*`
//! implicitly. Use `to_c_string(s)` / `s as c_string` inside an `arena` so the
//! NUL-terminated buffer is freed with the arena. Strings containing an interior
//! NUL are rejected by the runtime.

use crate::typecheck::Type;

/// Returns true if a type is allowed in Phase-1 scalar-only FFI.
pub fn is_phase1_ffi_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I64 | Type::F64 | Type::Bool | Type::Unit | Type::CChar
    )
}

/// Returns true if converting `str` → `c_string` is required.
pub fn requires_string_bridge(ty: &Type) -> bool {
    matches!(ty, Type::Str | Type::CString)
}
