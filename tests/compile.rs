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

#[test]
fn arrays_and_slices_compile() {
    let r = compile_ok(
        r#"
fn main() -> i64:
    let mut xs: [i64; 3] = [1, 2, 3]
    xs[0] = 9
    let s = &xs[1..3]
    print_i64(xs[0])
    print_i64(len(s))
    return s[0]
"#,
    );
    assert!(r.llvm_ir.contains("kroa_bounds_panic"), "{}", r.llvm_ir);
    assert!(r.llvm_ir.contains("getelementptr"), "{}", r.llvm_ir);
}

#[test]
fn enums_and_match_compile() {
    let r = compile_ok(
        r#"
enum Opt:
    Some(value: i64)
    None

fn main() -> i64:
    let x = Opt::Some(7)
    match x:
        case Opt::Some(v):
            print_i64(v)
        case Opt::None:
            print_i64(0)
    return 0
"#,
    );
    assert!(r.llvm_ir.contains("%enum.Opt"), "{}", r.llvm_ir);
    assert!(r.llvm_ir.contains("switch i64"), "{}", r.llvm_ir);
}

#[test]
fn rejects_non_exhaustive_match() {
    let msg = compile_err(
        r#"
enum Opt:
    Some(value: i64)
    None

fn main() -> i64:
    let x = Opt::Some(1)
    match x:
        case Opt::Some(v):
            print_i64(v)
    return 0
"#,
    );
    assert!(
        msg.contains("non-exhaustive") || msg.contains("E0310"),
        "{msg}"
    );
}

#[test]
fn rejects_static_oob_index() {
    let msg = compile_err(
        r#"
fn main() -> i64:
    let a: [i64; 2] = [1, 2]
    return a[2]
"#,
    );
    assert!(
        msg.contains("out of bounds") || msg.contains("E0306"),
        "{msg}"
    );
}

#[test]
fn rejects_bare_slice_type() {
    let msg = compile_err(
        r#"
fn main() -> i64:
    let s: [i64] = [1, 2]
    return 0
"#,
    );
    assert!(msg.contains("behind `&`") || msg.contains("E0305"), "{msg}");
}

#[test]
fn end_to_end_run_array_slice() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("skipping: clang not available");
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let exe = out
        .path()
        .join(if cfg!(windows) { "arr.exe" } else { "arr" });
    let options = CompileOptions {
        output: Some(exe.clone()),
        ..Default::default()
    };
    compile_source(
        "array_slice.kroa",
        r#"
fn main() -> i64:
    let mut xs: [i64; 4] = [10, 20, 30, 40]
    print_i64(xs[0])
    print_i64(xs[3])
    xs[1] = 21
    print_i64(xs[1])
    print_i64(len(xs))
    let s = &xs[1..3]
    print_i64(len(s))
    print_i64(s[0])
    return 0
"#,
        &options,
    )
    .unwrap_or_else(|d| panic!("{}", d.iter().map(|x| x.to_string()).collect::<String>()));
    let output = Command::new(&exe).output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("10"), "stdout={stdout}");
    assert!(stdout.contains("40"), "stdout={stdout}");
    assert!(stdout.contains("21"), "stdout={stdout}");
    assert!(stdout.contains("4"), "stdout={stdout}");
    assert!(stdout.contains("2"), "stdout={stdout}");
}

#[test]
fn end_to_end_run_enum_match() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("skipping: clang not available");
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let exe = out
        .path()
        .join(if cfg!(windows) { "enum.exe" } else { "enum" });
    let options = CompileOptions {
        output: Some(exe.clone()),
        ..Default::default()
    };
    compile_source(
        "enum_result.kroa",
        r#"
enum Result:
    Ok(value: i64)
    Err(code: i64)

fn main() -> i64:
    let r = Result::Ok(42)
    match r:
        case Result::Ok(v):
            print_i64(v)
        case Result::Err(c):
            print_i64(c)
    return 0
"#,
        &options,
    )
    .unwrap_or_else(|d| panic!("{}", d.iter().map(|x| x.to_string()).collect::<String>()));
    let output = Command::new(&exe).output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("42"), "stdout={stdout}");
}
