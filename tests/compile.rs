use kroa::{compile_source, CompileOptions};
use std::process::Command;

fn compile_ok(src: &str) -> kroa::CompileResult {
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    compile_source("test.kroa", src, &options).unwrap_or_else(|d| {
        let msg: String = d.iter().map(|x| x.to_string()).collect();
        panic!("compile failed:\n{msg}");
    })
}

fn compile_err(src: &str) -> String {
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    match compile_source("test.kroa", src, &options) {
        Ok(_) => panic!("expected errors"),
        Err(d) => d.iter().map(|x| x.to_string()).collect(),
    }
}

#[test]
fn rejects_tabs() {
    let msg = compile_err("fn main() -> i64:\n\treturn 1\n");
    assert!(msg.contains("tabs are not allowed"), "{msg}");
}

#[test]
fn emits_add() {
    let r = compile_ok("fn main() -> i64:\n    return 1 + 2\n");
    assert!(r.llvm_ir.contains("add i64"));
}

#[test]
fn type_mismatch() {
    let msg = compile_err("fn main() -> i64:\n    return true\n");
    assert!(msg.contains("return type mismatch"), "{msg}");
}

#[test]
fn emits_kir_on_request() {
    let options = CompileOptions {
        emit_kir: true,
        ..Default::default()
    };
    let r = compile_source("t.kroa", "fn main() -> i64:\n    return 7\n", &options).unwrap();
    let kir = r.kroa_ir.unwrap();
    assert!(kir.contains("fn main"), "{kir}");
    assert!(kir.contains("const.i64 7"), "{kir}");
}

#[test]
fn end_to_end_run_hello() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("skipping: clang not available");
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let exe = out.path().join(if cfg!(windows) { "t.exe" } else { "t" });
    let options = CompileOptions {
        output: Some(exe.clone()),
        ..Default::default()
    };
    compile_source(
        "hello.kroa",
        "fn main() -> i64:\n    print_i64(21 + 21)\n    return 0\n",
        &options,
    )
    .unwrap_or_else(|d| panic!("{}", d.iter().map(|x| x.to_string()).collect::<String>()));
    let output = Command::new(&exe).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("42"), "stdout={stdout}");
}
