use kroa::{compile_source, CompileOptions};

fn err(src: &str) -> Vec<kroa::diagnostics::Diagnostic> {
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    match compile_source("agent.kroa", src, &options) {
        Ok(_) => panic!("expected errors"),
        Err(d) => d,
    }
}

#[test]
fn rejects_c_style_and_with_canonical_help() {
    let diags = err("fn main() -> i64:\n    return 1 && 0\n");
    let text: String = diags.iter().map(|d| d.to_json_line()).collect();
    assert!(text.contains("E0201"), "{text}");
    assert!(text.contains("and"), "{text}");
    assert!(diags.iter().any(|d| d.help.is_some()));
}

#[test]
fn rejects_c_style_or_and_not() {
    let or_err = err("fn main() -> i64:\n    let x = true || false\n    return 0\n");
    assert!(or_err.iter().any(|d| d.code.as_str() == "E0201"));

    let not_err = err("fn main() -> i64:\n    let x = !false\n    return 0\n");
    assert!(not_err.iter().any(|d| d.code.as_str() == "E0201"));
}

#[test]
fn json_diagnostics_include_file_line_column() {
    let diags = err("fn main() -> i64:\n    return true\n");
    let d = diags
        .iter()
        .find(|d| d.code.as_str() == "E0304")
        .expect("expected return mismatch");
    assert_eq!(d.file.as_deref(), Some("agent.kroa"));
    assert!(d.line.is_some());
    assert!(d.column.is_some());
    let json = d.to_json_line();
    assert!(json.contains("\"file\":\"agent.kroa\""), "{json}");
    assert!(json.contains("\"line\":"), "{json}");
    assert!(json.contains("\"column\":"), "{json}");
}

#[test]
fn borrow_conflict_has_root_cause_code() {
    let diags = err(r#"
fn main() -> i64:
    let mut x = 1
    let a = &mut x
    let b = &mut x
    print_i64(*a + *b)
    return 0
"#);
    assert!(
        diags.iter().any(|d| d.code.as_str() == "E0400"),
        "{:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    assert!(diags.iter().any(|d| d.help.is_some()));
}

#[test]
fn array_static_oob_has_stable_code() {
    let diags = err(r#"
fn main() -> i64:
    let a: [i64; 1] = [7]
    return a[1]
"#);
    assert!(
        diags.iter().any(|d| d.code.as_str() == "E0306"),
        "{:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn non_exhaustive_match_has_stable_code() {
    let diags = err(r#"
enum Opt:
    Some(value: i64)
    None

fn main() -> i64:
    let x = Opt::None
    match x:
        case Opt::Some(v):
            print_i64(v)
    return 0
"#);
    assert!(
        diags.iter().any(|d| d.code.as_str() == "E0310"),
        "{:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}
