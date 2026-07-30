# Referencia

Referencia breve de sintaxis y herramientas de Kroa.

## Comandos

| Comando | Propósito |
|---------|-----------|
| `kroa build <file> -o <out>` | Compilar a binario nativo |
| `kroa run <file>` | Compilar y ejecutar |
| `kroa emit-ir <file>` | Mostrar LLVM IR |
| `kroa emit-kir <file>` | Mostrar Kroa IR |
| `kroa build file.kroa --message-format json` | Emitir diagnósticos NDJSON para agentes |
| `--library` / `-l` | Enlazar una biblioteca |
| `--library-path` / `-L` | Añadir ruta de bibliotecas |
| `--link <file>` | Enlazar un objeto/archivo C extra |
| `--keep-temps` | Conservar archivos temporales |

## Palabras clave

`fn`, `let`, `mut`, `if`, `else`, `while`, `return`, `true`, `false`, `extern`, `struct`, `arena`, `unsafe`, `as`, `and`, `or`, `not`

## Tipos

`i64`, `f64`, `bool`, `unit`, `str`, `c_char`, `c_string`, structs con nombre, `&T`, `&mut T`

## Funciones integradas

| Nombre | Descripción |
|--------|-------------|
| `print_i64(x)` | Imprime un entero |
| `print_f64(x)` | Imprime un decimal |
| `print_bool(x)` | Imprime un booleano |
| `print_str(s)` | Imprime un `str` de Kroa |
| `to_c_string(s)` | Convierte `str` → `c_string` (en arena) |

## Indentación

- Solo espacios
- Los tabuladores son un error
- Los bloques empiezan tras `:` y una sección indentada

## Extensión

`.kroa`
