# Guía FFI

Esta guía explica cómo Kroa habla con C.

## Para qué sirve la FFI

Las bibliotecas C están en todas partes.  
La FFI (interfaz de funciones externas) permite llamarlas desde Kroa.

## Reglas por fase

- La FFI escalar (`i64`, `f64`, `bool`) es la más simple.
- Cadenas y structs C son avanzadas: convierte con cuidado y llámalas desde `unsafe`.

## Declarar una función C

```kroa
extern "C" fn kroa_add(a: i64, b: i64) -> i64
```

Enlaza la implementación:

```bash
kroa run app.kroa --link native_lib.c
```

## Structs con layout C

```kroa
struct c CPoint:
    x: i64
    y: i64
```

La marca `c` significa “usar un layout compatible con C”.

## Cadenas

El `str` de Kroa guarda puntero + longitud (UTF-8).  
C suele querer un `char*` terminado en NUL.

Convierte de forma explícita dentro de una arena:

```kroa
arena:
    unsafe:
        let s = "hello"
        let c = to_c_string(s)
```

El búfer convertido vive en la arena y se libera al cerrarla.

## Modelo de seguridad

- Kroa comprueba el código Kroa.
- Kroa no puede demostrar que una biblioteca C sea correcta.
- Por eso las llamadas `extern` avanzadas son `unsafe`; conviene envolverlas en helpers pequeños y seguros cuando sea posible.
