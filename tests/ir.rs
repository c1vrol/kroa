use kroa::{compile_source, CompileOptions};

fn compile_kir(src: &str) -> String {
    let options = CompileOptions {
        emit_kir: true,
        ..Default::default()
    };
    compile_source("ir.kroa", src, &options)
        .unwrap_or_else(|d| panic!("{}", d.iter().map(|x| x.to_string()).collect::<String>()))
        .kroa_ir
        .unwrap()
}

#[test]
fn lowers_struct_and_fields() {
    let kir = compile_kir(
        r#"
struct Point:
    x: i64
    y: i64

fn main() -> i64:
    let p = Point { x: 1, y: 2 }
    return p.x
"#,
    );
    assert!(kir.contains("struct Point"), "{kir}");
    assert!(kir.contains("extract"), "{kir}");
}

#[test]
fn lowers_arena_enter_exit() {
    let kir = compile_kir(
        r#"
fn main() -> i64:
    arena:
        let x = 1
        print_i64(x)
    return 0
"#,
    );
    assert!(kir.contains("arena.enter"), "{kir}");
    assert!(kir.contains("arena.exit"), "{kir}");
}
