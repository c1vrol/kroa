# Kroa Project Status

> Living project summary. Update this file with every Kroa release.

**Current version:** Alpha-2.0.0 (`A-2.0.0`)  
**Cargo version:** `2.0.0-alpha`  
**Release date:** July 30, 2026  
**Status:** Alpha — functional compiler MVP, not production-ready

- [Resumen en español](#resumen-en-español)
- [English summary](#english-summary)

---

# Resumen en español

## Convención de versiones

Kroa usa el formato:

```text
A-MAJOR.MINOR.PATCH
```

Para la versión actual:

```text
A-2.0.0
```

- `A` significa **Alpha**: el lenguaje funciona, pero su sintaxis, ABI y herramientas todavía pueden cambiar.
- `MAJOR` (`2`) aumenta cuando hay una evolución grande del lenguaje, su arquitectura o compatibilidad.
- `MINOR` (`.0`) aumenta al añadir características pequeñas o compatibles dentro de la misma versión mayor.
- `PATCH` (`.0`) aumenta al corregir bugs sin introducir características importantes.

Cargo requiere una versión compatible con SemVer, por lo que `A-2.0.0` se representa internamente como `2.0.0-alpha`.

## Objetivo de Kroa

Kroa busca combinar:

- legibilidad e indentación sencilla inspiradas en Python;
- compilación AOT a ejecutables nativos;
- tipos estáticos e inferencia local;
- control de memoria seguro mediante movimientos, referencias y arenas;
- interoperabilidad directa con C;
- una gramática y diagnósticos fáciles de procesar por agentes de IA.

No se garantiza que todos los programas alcancen exactamente el rendimiento de C/C++. El rendimiento depende del programa, de las comprobaciones necesarias y de las optimizaciones que LLVM pueda aplicar.

## Arquitectura implementada

El pipeline actual es:

```text
.kroa source
  -> strict lexer
  -> indentation-aware parser
  -> AST
  -> local type checker
  -> Kroa IR + CFG
  -> borrow/memory analysis
  -> LLVM IR
  -> Clang
  -> native executable
```

El compilador está escrito en Rust. Genera LLVM IR textual y usa Clang para producir el ejecutable final. El runtime mínimo está escrito en C.

### Componentes principales

- `src/lexer.rs`: tokens, espacios significativos, `Indent`/`Dedent` y rechazo de tabs.
- `src/parser.rs`: parser predictivo y expresiones con precedencia definida.
- `src/ast.rs`: representación estructurada del código fuente.
- `src/typecheck.rs`: tipos, ámbitos, mutabilidad, movimientos e inferencia local.
- `src/ir.rs`: Kroa IR con bloques básicos, valores tipados y terminadores.
- `src/lower.rs`: transformación de AST tipado a Kroa IR.
- `src/borrowcheck.rs`: análisis de préstamos sobre el CFG.
- `src/codegen.rs`: generación de LLVM IR.
- `src/backend.rs`: invocación de Clang y enlace de bibliotecas.
- `src/ffi.rs`: reglas de interoperabilidad con C.
- `src/memory.rs`: detección y soporte de operaciones de arena.
- `src/diagnostics.rs`: diagnósticos humanos y NDJSON para herramientas.
- `runtime/runtime.c`: impresión, arenas y conversión segura a cadenas C.

## Lenguaje disponible

### Sintaxis y gramática

- Archivos con extensión `.kroa`.
- Bloques definidos por indentación.
- Solo espacios; los tabs se rechazan.
- Palabras clave, sintaxis, CLI y errores exclusivamente en inglés.
- Forma lógica canónica única: `and`, `or`, `not`.
- `&&`, `||` y `!` se rechazan para evitar sintaxis redundante. `!=` sí es válido.
- Comentarios de línea con `#`.

### Tipos

- `i64`
- `f64`
- `bool`
- `unit`
- `str` UTF-8 como puntero y longitud
- `c_char`
- `c_string`
- structs definidos por el usuario
- referencias `&T` y `&mut T`

La inferencia es local a cada función y bloque. Las conversiones numéricas son explícitas mediante `as`.

### Declaraciones y control de flujo

- funciones con `fn`;
- variables inmutables con `let`;
- variables mutables con `let mut`;
- asignación;
- `if` / `else`;
- `while`;
- `return`;
- llamadas de función;
- recursión;
- structs y acceso a campos;
- operaciones aritméticas, comparaciones y lógica booleana.

### Funciones integradas

- `print_i64`
- `print_f64`
- `print_bool`
- `print_str`
- `to_c_string`

## Memoria y seguridad

### Semántica de copia y movimiento

Los tipos pequeños y simples se copian. Los valores con recursos usan movimientos lógicos; el type checker detecta usos posteriores a un movimiento.

### Arenas léxicas

```kroa
arena:
    let message = "temporary"
    print_str(message)
```

- La arena agrupa asignaciones.
- Toda la memoria se libera al cerrar el bloque.
- Las salidas anticipadas insertan la liberación correspondiente.
- El runtime usa una pila de bump allocators con alineación.

### Préstamos

- `&T` crea una referencia compartida.
- `&mut T` crea una referencia mutable exclusiva.
- No puede coexistir un préstamo mutable con otro préstamo del mismo lugar.
- No se puede mover un valor mientras está prestado.
- No se puede devolver una referencia a almacenamiento local ni un puntero
  respaldado por una arena local.

El borrow checker usa análisis NLL-lite sobre el CFG: préstamos múltiples por lugar, muerte en el último uso de la referencia (incluidos locales que la almacenan), join en ramas/bucles y provenance de arena. Todavía no es el modelo completo de Rust; falta cobertura fina de escapes y aliasing avanzado antes de producción.

## FFI con C

Kroa admite:

- declaraciones `extern "C"`;
- escalares compatibles con C;
- enlace mediante `--library`, `--library-path` y `--link`;
- structs con layout C mediante `struct c Name`;
- `c_char` y `c_string`;
- conversión explícita de `str` a `c_string`;
- bloques `unsafe` para fronteras avanzadas.

`str` nunca se pasa implícitamente como `char*`. `to_c_string` crea una cadena terminada en NUL dentro de una arena y rechaza NUL internos.

Las bibliotecas C++ necesitan una envoltura `extern "C"`.

## Experiencia para agentes de IA

Kroa incluye una capa AI-Friendly:

- gramática canónica sin formas lógicas duplicadas;
- coordenadas exactas de archivo, línea, columna y rango;
- códigos de diagnóstico estables, por ejemplo `E0301` y `E0400`;
- mensajes con causa raíz;
- campos opcionales `notes` y `help`;
- salida NDJSON parseable.

Comando recomendado para agentes:

```bash
kroa build file.kroa --message-format json
```

Flujo de reparación:

1. compilar;
2. parsear cada línea JSON;
3. localizar `file:line:column`;
4. aplicar el cambio mínimo indicado por `message` y `help`;
5. repetir hasta obtener código de salida 0.

Contexto estático:

- `AGENTS.md`
- `docs/en/agent-spec.md`
- `docs/es/agent-spec.md`

## CLI disponible

```bash
kroa build file.kroa -o output
kroa run file.kroa
kroa emit-ir file.kroa
kroa emit-kir file.kroa
kroa build file.kroa --link native.c
kroa build file.kroa -l library -L ./libs
kroa build file.kroa --message-format json
```

## Ejemplos incluidos

- `examples/hello.kroa`
- `examples/factorial.kroa`
- `examples/loop_sum.kroa`
- `examples/struct_point.kroa`
- `examples/arena_string.kroa`
- `examples/borrow_mut.kroa`
- `examples/ffi_labs.kroa`
- `examples/ffi_add.kroa`
- `examples/ffi_struct.kroa`
- `examples/ffi_string.kroa`
- `examples/ffi/native_lib.c`

## Pruebas

La suite actual cubre:

- lexer y rechazo de tabs;
- parser;
- type checker;
- diagnósticos y coordenadas JSON;
- rechazo de sintaxis lógica no canónica;
- conflictos de préstamos;
- generación de Kroa IR;
- structs y arenas;
- generación LLVM;
- compilación y ejecución end-to-end;
- FFI con una biblioteca C mínima.

Comandos de verificación:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build --release
```

## Documentación

La documentación educativa está disponible en inglés y español:

- `README.md`
- `docs/en/getting-started.md`
- `docs/es/getting-started.md`
- `docs/en/language-guide.md`
- `docs/es/language-guide.md`
- `docs/en/reference.md`
- `docs/es/reference.md`
- `docs/en/troubleshooting.md`
- `docs/es/troubleshooting.md`
- `docs/en/ffi.md`
- `docs/es/ffi.md`
- especificaciones para agentes en ambos idiomas.

## Requisitos actuales

- Rust y Cargo.
- LLVM/Clang 18 o superior.
- `clang` accesible desde `PATH`.
- En Windows, normalmente `C:\Program Files\LLVM\bin`.

## Limitaciones conocidas de Alpha-1.0.0

- Es un MVP Alpha, no un compilador listo para producción.
- La sintaxis y la ABI todavía pueden cambiar.
- El borrow checker es NLL-lite (último uso + join CFG); aún no cubre todos los casos de escape/aliasing de un checker completo.
- Las arenas usan un runtime mínimo y aún no ofrecen una API pública completa de asignación tipada.
- Los errores externos de Clang no se traducen todavía a todos los códigos Kroa específicos.
- No hay módulos, imports, gestor de paquetes, formatter ni LSP.
- No hay arreglos, slicing, pattern matching, enums con datos ni genéricos.
- No existe todavía una biblioteca estándar general.
- La configuración LLVM/triple y la compatibilidad multiplataforma necesitan más trabajo.
- La FFI no puede garantizar la seguridad del código C externo.

## Roadmap posterior

1. Arreglos y slicing seguro.
2. Pattern matching y enums con datos.
3. Sobrecarga controlada de operadores.
4. Genéricos monomorfizados.
5. Borrow checker más completo.
6. Optimizaciones sobre Kroa IR.
7. Módulos e imports.
8. Formatter, LSP y gestor de paquetes.
9. Biblioteca estándar.
10. Compatibilidad multiplataforma estable.

## Regla de actualización de este documento

En cada update:

1. cambiar la versión y fecha al inicio;
2. actualizar la versión de Cargo;
3. añadir una entrada al historial;
4. actualizar capacidades, pruebas y limitaciones;
5. documentar cualquier cambio de sintaxis o compatibilidad;
6. mantener sincronizados `README.md`, `AGENTS.md` y las guías;
7. ejecutar las verificaciones antes de publicar.

## Historial

### Alpha-2.0.0 — July 30, 2026

Evolución mayor del lenguaje:

- arrays fijos `[T; N]` y slices seguros `&[T]` / `&mut [T]`;
- enums con `match` / `case` y comprobación de exhaustividad;
- borrow checker NLL-lite (último uso, joins CFG, multi-loan, reborrows);
- análisis de escape para referencias locales y `c_string` de arena;
- documentación técnica del borrow checker y proceso profesional con entornos `development` / `production`.

### Alpha-1.0.0 — July 30, 2026

Primera versión Alpha documentada (reconstruida; ver `RECONSTRUCTION.md`):

- pipeline completo hasta ejecutable nativo;
- lexer y parser por indentación;
- tipos estáticos e inferencia local;
- Kroa IR y CFG;
- structs, movimientos y arenas;
- referencias y borrow checking inicial;
- FFI con C, strings y structs C;
- CLI de build/run/emisión de IR;
- diagnósticos humanos y NDJSON;
- gramática optimizada para agentes;
- documentación bilingüe, ejemplos y pruebas automatizadas.

---

# English summary

## Version

Current release: **Alpha-2.0.0 (`A-2.0.0`)**, represented as `2.0.0-alpha` in Cargo.

Version format:

```text
A-MAJOR.MINOR.PATCH
```

- `A`: Alpha development stage.
- `MAJOR`: large language, architecture, or compatibility change.
- `MINOR`: smaller compatible feature addition.
- `PATCH`: bug fix without a major feature.

## Implemented

- Rust compiler with a strict indentation lexer and predictive parser.
- Static primitive, string, struct, C, and reference types.
- Function/block-local inference and scope resolution.
- Functions, recursion, variables, mutation, conditions, loops, returns, structs, and casts.
- Kroa IR with typed values, basic blocks, terminators, and CFG.
- Native compilation through textual LLVM IR and Clang.
- Copy/move semantics, lexical arenas, and initial borrow checking.
- C FFI with scalars, C-layout structs, strings, unsafe boundaries, and linker options.
- Human diagnostics plus stable-code NDJSON diagnostics for AI agents.
- Canonical `and` / `or` / `not` grammar; tabs and redundant C-style logical forms are rejected.
- English/Spanish documentation, examples, and automated tests.

## Main limitations

Kroa is not production-ready. The arena API, standard library, platform support, tooling, modules, generics, package management, and editor integration remain incomplete. The borrow checker is NLL-lite rather than a full region system.

## Update rule

Every release must update this file’s version, date, history, capabilities, tests, limitations, Cargo version, README, agent specification, and bilingual documentation.

## Release history

### Alpha-2.0.0 — July 30, 2026

Major language evolution adding fixed arrays and safe slices, enums with `match`/`case`, NLL-lite borrow checking, local/arena escape analysis, borrow-checker documentation, and a professional development/production release workflow.

### Alpha-1.0.0 — July 30, 2026

First documented Alpha release (reconstructed; see `RECONSTRUCTION.md`) containing the native compiler pipeline, Kroa IR, static types, structs, arenas, initial borrowing, C FFI, agent-oriented diagnostics, examples, tests, and bilingual documentation.
