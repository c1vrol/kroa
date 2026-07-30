# Solución de problemas

Los mensajes del compilador están siempre en inglés. Esta página los explica.

## Diagnósticos estructurados para agentes

```bash
kroa build file.kroa --message-format json
```

Cada línea es un objeto JSON con `severity`, `code`, `message`, `file`, `line`, `column`, y opcionalmente `notes` y `help`.  
Ver [Especificación para agentes](agent-spec.md).

## `tabs are not allowed; indent with spaces only`

Kroa rechaza tabuladores.

**Solución:** configura el editor para insertar espacios y vuelve a indentar.

## `expected indent` / `inconsistent indentation`

Los espacios no coinciden con un nivel anterior.

**Solución:** usa un ancho fijo (por ejemplo 4 espacios) en cada bloque anidado.

## `undefined variable`

Usaste un nombre no declarado o fuera de ámbito.

**Solución:** decláralo con `let` antes de usarlo, o revisa la ortografía.

## `cannot assign to immutable variable`

Intentaste cambiar un valor creado con `let` (sin `mut`).

**Solución:** escribe `let mut name = ...` si la mutación es intencionada.

## `type mismatch` / `return type mismatch`

Un valor tiene un tipo distinto al esperado.

**Solución:** convierte con `as` cuando cambies entre números, o ajusta el tipo declarado.

## `extern function ... must be called inside unsafe`

La FFI avanzada (cadenas/structs) se considera insegura porque Kroa no puede verificar el código C.

**Solución:** envuelve la llamada en:

```kroa
unsafe:
    ...
```

## `clang failed while linking native binary`

Se generó LLVM IR, pero Clang no pudo crear el ejecutable.

Causas frecuentes:

- `clang` no está en el `PATH`
- falta un archivo C o biblioteca enlazada
- la firma `extern` no coincide con la definición C

**Solución:** comprueba `clang --version`, las rutas de `--link` y las firmas.

## `string contains interior NUL; cannot convert to c_string`

`to_c_string` encontró un byte cero dentro del texto. Las cadenas C no pueden representarlo con seguridad.

**Solución:** elimina `\0` interiores o mantén el dato como `str` de Kroa.

## ¿Sigues atascado?

1. Ejecuta `kroa emit-ir file.kroa`.
2. Ejecuta `kroa emit-kir file.kroa`.
3. Compara con los ejemplos de la carpeta `examples/`.
