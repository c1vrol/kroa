use kroa::{compile_source, CompileOptions};

fn err(src: &str) -> String {
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    match compile_source("b.kroa", src, &options) {
        Ok(_) => panic!("expected borrow error"),
        Err(diagnostics) => diagnostics.iter().map(|d| d.to_string()).collect(),
    }
}

fn ok(src: &str) {
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    compile_source("b.kroa", src, &options).unwrap_or_else(|diagnostics| {
        panic!(
            "expected success, got: {}",
            diagnostics
                .iter()
                .map(|d| d.to_string())
                .collect::<String>()
        )
    });
}

#[test]
fn detects_conflicting_mutable_borrows() {
    let message = err(r#"
fn main() -> i64:
    let mut x = 1
    let a = &mut x
    let b = &mut x
    print_i64(*a + *b)
    return 0
"#);
    assert!(message.contains("E0400") || message.contains("already borrowed"));
}

#[test]
fn rejects_mutable_while_shared_borrowed() {
    let message = err(r#"
fn main() -> i64:
    let mut x = 1
    let a = &x
    let b = &mut x
    print_i64(*a + *b)
    return 0
"#);
    assert!(message.contains("E0400") || message.contains("already borrowed"));
}

#[test]
fn rejects_assign_while_borrowed() {
    let message = err(r#"
fn main() -> i64:
    let mut x = 1
    let a = &x
    x = 2
    print_i64(*a)
    return 0
"#);
    assert!(message.contains("E0401") || message.contains("cannot assign"));
}

#[test]
fn allows_two_shared_borrows() {
    ok(r#"
fn main() -> i64:
    let x = 1
    let a = &x
    let b = &x
    print_i64(*a + *b)
    return 0
"#);
}

#[test]
fn keeps_borrows_lexical_without_last_use_shortening() {
    let message = err(r#"
fn main() -> i64:
    let mut x = 1
    let first = &mut x
    *first = 2
    let second = &mut x
    *second = 3
    return x
"#);
    assert!(message.contains("E0400") || message.contains("already borrowed"));
}

#[test]
fn rejects_move_while_borrowed() {
    let message = err(r#"
struct Box:
    value: i64

fn main() -> i64:
    let item = Box { value: 7 }
    let borrowed = &item
    let moved = item
    print_i64((*borrowed).value)
    return moved.value
"#);
    assert!(message.contains("E0402") || message.contains("cannot move"));
}

#[test]
fn rejects_return_reference_to_local_storage() {
    let message = err(r#"
fn bad() -> &i64:
    let x = 1
    return &x

fn main() -> i64:
    return 0
"#);
    assert!(message.contains("E0403") || message.contains("local"));
}

#[test]
fn allows_returning_reference_received_from_caller() {
    ok(r#"
fn identity(x: &i64) -> &i64:
    return x

fn main() -> i64:
    let x = 7
    let r = identity(&x)
    return *r
"#);
}
