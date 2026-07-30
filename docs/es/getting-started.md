# Primeros pasos

Esta guía te lleva desde la instalación hasta tu primer programa Kroa.

## 1. Instalar herramientas

Necesitas dos herramientas:

1. **Rust** — para construir el compilador de Kroa  
2. **Clang (LLVM)** — para convertir LLVM IR en un ejecutable nativo

### Instalar Rust

Sigue https://rustup.rs/ y comprueba:

```bash
rustc --version
cargo --version
```

### Instalar Clang

Instala LLVM de forma que `clang` esté disponible en la terminal:

```bash
clang --version
```

En Windows, añade `C:\Program Files\LLVM\bin` a tu `PATH` si hace falta.

## 2. Compilar Kroa

Desde la raíz del proyecto:

```bash
cargo build --release
```

Ejecuta el compilador con:

```bash
./target/release/kroa run examples/hello.kroa
```

## 3. Escribir un programa pequeño

Crea `hello.kroa`:

```kroa
fn main() -> i64:
    let message_number = 40
    print_i64(message_number + 2)
    return 0
```

Qué significa:

- `fn main() -> i64` define el punto de entrada. Devuelve un entero.
- `let` crea una variable inmutable (no puedes cambiarla después, salvo que uses `let mut`).
- `print_i64(...)` imprime un entero.
- La indentación (solo espacios) define el cuerpo. Los tabuladores se rechazan.

La sintaxis del lenguaje es siempre en inglés.

## 4. Compilar y ejecutar

```bash
kroa run hello.kroa
```

Deberías ver:

```text
42
```

## 5. Siguientes pasos

- Lee la [Guía del lenguaje](language-guide.md).
- Consulta la [Referencia](reference.md).
- Si algo falla, abre [Solución de problemas](troubleshooting.md).

## Nota sobre rendimiento

Kroa compila por adelantado (AOT) a un ejecutable nativo.  
La velocidad exacta depende del programa: el código numérico puede ser muy rápido, y algunas comprobaciones de seguridad pueden tener coste si LLVM no puede eliminarlas.
