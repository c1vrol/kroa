# Guía del lenguaje

Esta guía explica Kroa paso a paso. Los ejemplos usan siempre sintaxis en inglés.

## Variables

Crea un valor con `let`:

```kroa
fn main() -> i64:
    let x = 10
    print_i64(x)
    return 0
```

Usa `let mut` cuando necesites cambiar el valor después:

```kroa
fn main() -> i64:
    let mut total = 0
    total = total + 5
    print_i64(total)
    return 0
```

## Tipos

Kroa comprueba los tipos antes de ejecutar el programa.

Tipos comunes hoy:

| Tipo | Significado |
|------|-------------|
| `i64` | Entero de 64 bits |
| `f64` | Decimal de 64 bits |
| `bool` | `true` o `false` |
| `unit` | “sin valor útil” (parecido a void) |
| `str` | Texto UTF-8 (puntero + longitud) |

Puedes anotar tipos o dejar que Kroa los infiera en local:

```kroa
fn main() -> i64:
    let a: i64 = 1
    let b = 2
    return a + b
```

Convierte números de forma explícita con `as`:

```kroa
fn main() -> i64:
    let x: f64 = 3.5
    let y = x as i64
    print_i64(y)
    return 0
```

## Operadores

Aritméticos: `+ - * / %`  
Comparación: `== != < <= > >=`  
Lógicos (solo forma canónica): `and`, `or`, `not`

No uses `&&`, `||` ni `!` suelto. El compilador los rechaza para que herramientas y agentes tengan una sola ortografía.

## Condiciones y ciclos

```kroa
fn main() -> i64:
    let n = 7
    if n > 5:
        print_i64(1)
    else:
        print_i64(0)
    return 0
```

```kroa
fn main() -> i64:
    let mut i = 0
    while i < 3:
        print_i64(i)
        i = i + 1
    return 0
```

## Funciones

```kroa
fn add(a: i64, b: i64) -> i64:
    return a + b

fn main() -> i64:
    print_i64(add(2, 3))
    return 0
```

## Structs

```kroa
struct Point:
    x: i64
    y: i64

fn main() -> i64:
    let p = Point { x: 3, y: 4 }
    print_i64(p.x + p.y)
    return 0
```

Para layout compatible con C: `struct c Name`.

## Arenas

Una arena libera toda su memoria al cerrar el bloque:

```kroa
fn main() -> i64:
    arena:
        let s = "hello"
        print_str(s)
    return 0
```

## Referencias

`&T` presta un valor. `&mut T` lo presta para modificarlo.

```kroa
fn add_one(x: &mut i64) -> unit:
    *x = *x + 1
    return

fn main() -> i64:
    let mut n = 41
    add_one(&mut n)
    print_i64(n)
    return 0
```

Reglas breves:

- Una referencia no puede vivir más que el valor.
- No puede coexistir un préstamo mutable con otros préstamos del mismo valor.
- Los préstamos terminan en el último uso de la referencia (NLL-lite), así que se permiten `&mut` secuenciales no solapados.

Para entender el algoritmo completo —CFG, liveness, joins, places, carriers y
provenance de arena— consulta la
[guía técnica del borrow checker](borrow-checker.md).

## Cadenas y C

`str` de Kroa no es un `char*` de C. Convierte de forma explícita con `to_c_string` dentro de una `arena`.

## Llamar a C

```kroa
extern "C" fn kroa_add(a: i64, b: i64) -> i64
```

```bash
kroa run app.kroa --link mylib.c
```

Las llamadas con tipos C avanzados van dentro de `unsafe`.
