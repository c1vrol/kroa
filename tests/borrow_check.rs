use kroa::{compile_source, CompileOptions};

fn err(src: &str) -> String {
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    match compile_source("b.kroa", src, &options) {
        Ok(_) => panic!("expected borrow error"),
        Err(d) => d.iter().map(|x| x.to_string()).collect(),
    }
}

fn ok(src: &str) {
    let options = CompileOptions {
        emit_ir: true,
        ..Default::default()
    };
    compile_source("b.kroa", src, &options).unwrap_or_else(|d| {
        panic!(
            "expected success, got: {}",
            d.iter().map(|x| x.to_string()).collect::<String>()
        )
    });
}

#[test]
fn detects_conflicting_mutable_borrows() {
    // Two mutable borrows of the same place at once.
    let msg = err(r#"
fn main() -> i64:
    let mut x = 1
    let a = &mut x
    let b = &mut x
    print_i64(*a + *b)
    return 0
"#);
    assert!(
        msg.contains("cannot borrow as mutable") || msg.contains("already borrowed"),
        "{msg}"
    );
}

#[test]
fn detects_overlapping_array_slice_borrows() {
    let msg = err(r#"
fn main() -> i64:
    let mut a: [i64; 4] = [1, 2, 3, 4]
    let s = &mut a[0..2]
    let t = &mut a[2..4]
    print_i64(s[0])
    print_i64(t[0])
    return 0
"#);
    assert!(
        msg.contains("already borrowed") || msg.contains("cannot create"),
        "{msg}"
    );
}

#[test]
fn allows_sequential_mutable_borrows_after_last_use() {
    // NLL-lite: first `&mut` dies after its last use, so a second is allowed.
    ok(r#"
fn main() -> i64:
    let mut x = 1
    let a = &mut x
    *a = 2
    let b = &mut x
    *b = 3
    print_i64(x)
    return 0
"#);
}

#[test]
fn allows_shared_then_mutable_after_last_use() {
    ok(r#"
fn main() -> i64:
    let mut x = 1
    let a = &x
    print_i64(*a)
    let b = &mut x
    *b = 5
    print_i64(x)
    return 0
"#);
}

#[test]
fn rejects_mutable_while_shared_still_live() {
    let msg = err(r#"
fn main() -> i64:
    let mut x = 1
    let a = &x
    let b = &mut x
    print_i64(*a + *b)
    return 0
"#);
    assert!(
        msg.contains("already borrowed") || msg.contains("cannot create"),
        "{msg}"
    );
}

#[test]
fn rejects_assign_while_shared_borrowed() {
    let msg = err(r#"
fn main() -> i64:
    let mut x = 1
    let a = &x
    x = 2
    print_i64(*a)
    return 0
"#);
    assert!(
        msg.contains("shared-borrowed") || msg.contains("cannot assign"),
        "{msg}"
    );
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
fn rejects_conflict_after_branch_join() {
    // `r` may carry a loan of `x` on one incoming CFG edge. The join must
    // preserve both alternatives instead of choosing one HashMap entry.
    let msg = err(r#"
fn main() -> i64:
    let mut x = 1
    let mut y = 2
    let mut r = &x
    if x == 0:
        r = &y
    let m = &mut x
    print_i64(*r + *m)
    return 0
"#);
    assert!(
        msg.contains("already borrowed") || msg.contains("cannot create"),
        "{msg}"
    );
}

#[test]
fn allows_branch_local_loan_dead_at_join() {
    ok(r#"
fn main() -> i64:
    let mut x = 1
    if x == 0:
        let r = &mut x
        *r = 2
    let next = &mut x
    *next = 3
    return x
"#);
}

#[test]
fn rejects_return_reference_to_local_storage() {
    let msg = err(r#"
fn bad() -> &i64:
    let x = 1
    return &x

fn main() -> i64:
    return 0
"#);
    assert!(
        msg.contains("E0403") || msg.contains("local storage"),
        "{msg}"
    );
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

#[test]
fn rejects_arena_backed_c_string_return() {
    let msg = err(r#"
fn bad() -> c_string:
    arena:
        return to_c_string("temporary")

fn main() -> i64:
    return 0
"#);
    assert!(
        msg.contains("E0403") || msg.contains("local arena"),
        "{msg}"
    );
}

#[test]
fn allows_replacing_reference_after_its_last_read() {
    // Writing a new value into `r` kills the old slot contents. The write to
    // the slot itself must not artificially keep the previous loan alive.
    ok(r#"
fn main() -> i64:
    let mut x = 1
    let mut r = &mut x
    *r = 2
    r = &mut x
    *r = 3
    return x
"#);
}

#[test]
fn converges_for_a_dead_loan_inside_a_loop() {
    ok(r#"
fn main() -> i64:
    let mut x = 0
    let mut i = 0
    while i < 3:
        let r = &mut x
        *r = *r + 1
        i = i + 1
    return x
"#);
}

#[test]
fn rejects_direct_read_while_mutably_borrowed() {
    let msg = err(r#"
fn main() -> i64:
    let mut x = 1
    let r = &mut x
    print_i64(x)
    print_i64(*r)
    return 0
"#);
    assert!(
        msg.contains("mutably borrowed") || msg.contains("cannot read"),
        "{msg}"
    );
}

#[test]
fn rejects_direct_write_while_mutably_borrowed() {
    let msg = err(r#"
fn main() -> i64:
    let mut x = 1
    let r = &mut x
    x = 2
    print_i64(*r)
    return 0
"#);
    assert!(msg.contains("cannot assign"), "{msg}");
}

#[test]
fn allows_access_to_original_after_mutable_last_use() {
    ok(r#"
fn main() -> i64:
    let mut x = 1
    let r = &mut x
    *r = 2
    print_i64(x)
    return 0
"#);
}

#[test]
fn rejects_move_from_the_place_while_borrowed() {
    let msg = err(r#"
struct Box:
    value: i64

fn main() -> i64:
    let item = Box { value: 7 }
    let borrowed = &item
    let moved = item
    print_i64((*borrowed).value)
    return moved.value
"#);
    assert!(
        msg.contains("cannot move") || msg.contains("E0402"),
        "{msg}"
    );
}

#[test]
fn rejects_copying_a_mutable_reference() {
    let msg = err(r#"
fn main() -> i64:
    let mut x = 0
    let a = &mut x
    let b = a
    *a = 1
    *b = 2
    return x
"#);
    assert!(
        msg.contains("moved") || msg.contains("already borrowed") || msg.contains("E0303"),
        "{msg}"
    );
}

#[test]
fn rejects_local_ref_escaping_inside_struct() {
    let msg = err(r#"
struct Holder:
    r: &i64

fn bad() -> Holder:
    let x = 1
    return Holder { r: &x }

fn main() -> i64:
    return 0
"#);
    assert!(
        msg.contains("E0403") || msg.contains("local storage"),
        "{msg}"
    );
}

#[test]
fn rejects_conflict_through_returned_ref_arg() {
    let msg = err(r#"
fn second(a: &i64, b: &i64) -> &i64:
    return b

fn main() -> i64:
    let mut x = 1
    let mut y = 2
    let out = second(&x, &y)
    let m = &mut y
    print_i64(*out + *m)
    return 0
"#);
    assert!(
        msg.contains("already borrowed") || msg.contains("cannot create"),
        "{msg}"
    );
}

#[test]
fn allows_reborrow_through_deref() {
    ok(r#"
fn main() -> i64:
    let mut x = 1
    let p = &mut x
    let q = &mut *p
    *q = 9
    return x
"#);
}

#[test]
fn rejects_to_c_string_outside_arena() {
    let msg = err(r#"
fn bad() -> c_string:
    return to_c_string("temporary")

fn main() -> i64:
    return 0
"#);
    assert!(msg.contains("arena") || msg.contains("E0500"), "{msg}");
}

#[test]
fn rejects_use_of_c_string_after_arena_exit() {
    let msg = err(r#"
extern "C" fn consume(c: c_string) -> unit

fn bad(seed: c_string) -> i64:
    let mut c = seed
    arena:
        c = to_c_string("temporary")
    unsafe:
        consume(c)
    return 0

fn main() -> i64:
    return 0
"#);
    assert!(msg.contains("E0403") || msg.contains("arena"), "{msg}");
}

#[test]
fn rejects_slice_reborrow_aliasing_original_array() {
    let msg = err(r#"
fn main() -> i64:
    let mut a: [i64; 2] = [1, 2]
    let mut s = &mut a[0..2]
    let x = &mut s[0]
    let y = &mut a[0]
    print_i64(*x + *y)
    return 0
"#);
    assert!(
        msg.contains("already borrowed") || msg.contains("cannot create"),
        "{msg}"
    );
}
