# Especificación para agentes Kroa

> **Propósito:** usa este archivo (o la versión en inglés) como contexto estático / system prompt cuando un agente deba leer, escribir, revisar o auto-corregir código Kroa.
>
> El lenguaje, las palabras clave, los diagnósticos y la CLI son solo en inglés. Esta guía explica esas reglas en español.

## 1. Qué es Kroa

Kroa es un lenguaje compilado AOT:

- Indentación estilo Python (solo espacios; tabuladores ilegales)
- Tipos estáticos con inferencia **local** dentro de funciones
- Código nativo vía LLVM
- Arenas y borrow checking opcionales
- `extern "C"` para llamar a C

## 2. Gramática canónica (una sola forma)

No inventes ortografías alternativas. Usa exactamente:

| Intención | Sintaxis canónica | Prohibido |
|-----------|-------------------|-----------|
| Y lógico | `and` | `&&` |
| O lógico | `or` | `\|\|` |
| Negación | `not` | `!` (excepto `!=`) |
| Indentación | espacios | tabs (`\t`) |
| Mutación | `let mut x = ...` y luego `x = ...` | mutar un `let` plano |
| Préstamo | `&x` / `&mut x` | punteros crudos en Kroa seguro |
| Cadena C | `to_c_string(s)` dentro de `arena:` | pasar `str` como `char*` |

### Programa mínimo

```kroa
fn main() -> i64:
    print_i64(40 + 2)
    return 0
```

## 3. Tipos y razonamiento local

Primitivos: `i64`, `f64`, `bool`, `unit`, `str`  
FFI: `c_char`, `c_string`, `struct c Name`  
Referencias: `&T`, `&mut T`

La inferencia es **local**:

- Los nombres se resuelven solo con la pila de ámbitos de la función actual.
- No inventes globales ni imports implícitos.
- Las conversiones numéricas son explícitas: `as i64`, `as f64`.

Un agente puede reescribir una sola función con precisión si conserva:

1. la firma,
2. los `let` / `let mut` en orden,
3. los tipos de las funciones llamadas (solo desde sus firmas).

## 4. Memoria y préstamos (causas raíz)

1. **Compartido XOR mutable:** muchos `&T`, o un solo `&mut T`, nunca ambos.
2. **No mover mientras hay préstamos.**
3. **No asignar** a un lugar con préstamo compartido activo.
4. **Arenas:** la memoria de `arena:` muere al cerrar el bloque (también con `return`). No devuelvas referencias a la arena.

## 5. Diagnósticos para bucles de auto-corrección

```bash
kroa build file.kroa --message-format json
```

Cada línea es un objeto JSON (NDJSON) con:

| Campo | Significado |
|-------|-------------|
| `severity` | `error` o `warning` |
| `code` | id estable, p. ej. `E0301`, `E0400` |
| `message` | causa raíz |
| `file` | ruta |
| `line`, `column` | inicio (base 1) |
| `end_line`, `end_column` | fin (base 1) |
| `notes` | hechos extra |
| `help` | arreglo concreto sugerido |

### Códigos importantes

| Código | Significado |
|--------|-------------|
| `E0100` | tab rechazado |
| `E0101` | indentación inconsistente |
| `E0201` | sintaxis no canónica (`&&`, `||`, `!`) |
| `E0300` | nombre indefinido |
| `E0301` | desajuste de tipos |
| `E0302` | asignación a inmutable |
| `E0303` | uso/movimiento inválido |
| `E0304` | retorno incorrecto |
| `E0400` | conflicto de préstamos |
| `E0401` | asignación mientras está prestado |
| `E0402` | movimiento mientras está prestado |
| `E0403` | referencia escapa de arena |
| `E0404` | enter/exit de arena desbalanceado |
| `E0500` | frontera FFI/unsafe |

### Bucle de reparación

1. Editar el `.kroa`.
2. Ejecutar `kroa build path.kroa --message-format json`.
3. Parsear cada línea JSON.
4. Aplicar `help` / `message` en `file:line:column`.
5. Recompilar hasta código de salida 0.

## 6. Reglas duras para agentes

1. Prefiere el cambio mínimo que limpie diagnósticos.
2. Nunca introduzcas `&&`, `||`, `!` suelto ni tabs.
3. Mantén los cambios dentro de una función cuando sea posible.
4. Trata `extern` avanzado como `unsafe`.
5. No asumas GC ni tipado dinámico estilo Python: Kroa es estático y AOT.

La versión canónica pensada para pegar en system prompts en inglés está en [`../en/agent-spec.md`](../en/agent-spec.md).
