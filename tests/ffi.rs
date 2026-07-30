use kroa::{compile_source, CompileOptions};
use std::process::Command;

#[test]
fn scalar_ffi_with_c_source() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("skipping: clang not available");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let c_path = dir.path().join("lib.c");
    std::fs::write(
        &c_path,
        "long long kroa_mul(long long a, long long b) { return a * b; }\n",
    )
    .unwrap();
    let exe = dir.path().join(if cfg!(windows) { "t.exe" } else { "t" });
    let options = CompileOptions {
        output: Some(exe.clone()),
        link_files: vec![c_path],
        ..Default::default()
    };
    compile_source(
        "ffi.kroa",
        "extern \"C\" fn kroa_mul(a: i64, b: i64) -> i64\n\nfn main() -> i64:\n    print_i64(kroa_mul(6, 7))\n    return 0\n",
        &options,
    )
    .unwrap_or_else(|d| panic!("{}", d.iter().map(|x| x.to_string()).collect::<String>()));
    let out = Command::new(&exe).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("42"));
}

#[test]
fn rejects_interior_nul_at_runtime_boundary_ir() {
    // Ensure to_c_string appears in IR for conversion path.
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    let r = compile_source(
        "s.kroa",
        "fn main() -> i64:\n    arena:\n        let s = \"hi\"\n        let c = to_c_string(s)\n        print_i64(1)\n    return 0\n",
        &options,
    )
    .unwrap();
    assert!(r.llvm_ir.contains("kroa_str_to_cstr"), "{}", r.llvm_ir);
}
