# Borrow checker NLL-lite: guía técnica entendible

Esta guía explica cómo funciona el borrow checker de Kroa por dentro. Está escrita para que una persona de unos 12 años pueda construir una imagen mental correcta y, al mismo tiempo, aprender los términos técnicos usados por quienes diseñan compiladores.

El código principal está en:

- `src/borrowcheck.rs`: análisis de préstamos, flujo de datos y diagnósticos.
- `src/ir.rs`: definición de KIR, bloques básicos e instrucciones.
- `src/lower.rs`: conversión del programa a KIR y orden correcto de salida de arenas.
- `src/typecheck.rs`: decide cuándo debe ejecutarse el borrow checker.
- `tests/borrow_check.rs`: ejemplos que deben aceptarse o rechazarse.

## 1. El problema que resuelve

Imagina que `x` es un cuaderno.

- `&x` es un permiso para leerlo.
- `&mut x` es la única llave que permite modificarlo.
- Un **préstamo** (*loan*) es el registro que dice quién tiene un permiso.
- El **lugar** (*place*) es el objeto prestado; en este caso, el cuaderno `x`.

Kroa permite:

- muchos lectores al mismo tiempo;
- o un único escritor;
- pero nunca un escritor mientras exista otro lector o escritor.

La regla técnica se llama **shared XOR mutable**: préstamos compartidos o un préstamo mutable exclusivo.

```kroa
let mut x = 1
let a = &x
let b = &x
print_i64(*a + *b)  # correcto: dos lectores
```

```kroa
let mut x = 1
let a = &x
let b = &mut x      # E0400: lector y escritor se solapan
print_i64(*a + *b)
```

## 2. Qué significa NLL-lite

**NLL** significa *Non-Lexical Lifetimes*, o “tiempos de vida no léxicos”.

Un tiempo de vida **léxico** terminaría al cerrar el bloque indentado, aunque la referencia ya no se utilice. NLL termina el préstamo en su **último uso real**.

```kroa
let mut x = 1
let first = &mut x
*first = 2          # último uso de first
let second = &mut x # correcto con NLL-lite
*second = 3
```

Se llama **NLL-lite** porque implementa la parte esencial mediante vivacidad y flujo de control, pero no pretende reproducir todo el sistema de regiones y tiempos de vida de Rust.

Una respuesta técnica correcta sería:

> Kroa usa un análisis backward de liveness sobre el CFG. Un préstamo permanece activo mientras algún valor SSA o slot local que lo transporta está vivo. El análisis forward de préstamos consume esa información para matar el loan después de su último uso.

## 3. El mapa de carreteras: CFG

**CFG** significa *Control-Flow Graph* o grafo de flujo de control.

Un programa no siempre avanza en línea recta:

- `if` divide el camino;
- `match` puede dividirlo en muchos caminos;
- `while` crea un camino que vuelve hacia atrás;
- `return` termina el camino.

El compilador divide cada función en **bloques básicos**. Un bloque básico es una lista de instrucciones que se ejecuta en orden y termina con una decisión:

- `Jump`: continúa en otro bloque;
- `Branch`: elige entre dos;
- `Switch`: elige entre varios;
- `Return`: sale de la función;
- `Unreachable`: no existe una continuación válida.

`terminator_targets` obtiene los sucesores de cada bloque. A partir de ellos también se calculan sus predecesores.

En términos sencillos: el checker construye el mapa de carreteras antes de comprobar qué permisos viajan por cada carretera.

## 4. KIR, SSA, valores y slots

Kroa analiza una representación intermedia llamada **KIR** (*Kroa Intermediate Representation*).

KIR es una versión muy precisa y pequeña del programa. En ella:

- `ValueId` identifica un valor temporal;
- `BlockId` identifica un bloque básico;
- `Alloca` reserva un **slot** local;
- `Store` escribe en un slot;
- `Load` lee de un slot;
- `Ref` crea `&T` o `&mut T`;
- `Move` transfiere un valor;
- `ArenaEnter` y `ArenaExit` delimitan una arena.

KIR es **SSA-like**. SSA significa *Static Single Assignment*: cada valor temporal recibe un identificador nuevo y no cambia. Los locales que sí cambian se representan como slots con `Store` y `Load`.

Ejemplo mental:

```text
%1 = alloca       // casillero de x
%2 = const 1
store %1, %2
%3 = ref %1       // préstamo de x
```

La separación entre valor SSA y slot importa: una referencia puede nacer como `%3`, guardarse en el slot de `r`, cargarse después como `%8` y seguir siendo el mismo préstamo lógico.

## 5. Las piezas de un préstamo

### `LoanId`

Es la identidad estable de un préstamo. Usa el `ValueId` de la instrucción `Ref` que lo creó.

Esto evita generar identidades nuevas cuando el algoritmo vuelve a visitar un bloque dentro de un bucle.

### `Loan`

Guarda:

- `id`: identidad del préstamo;
- `place`: raíz prestada;
- `mutable`: `true` para `&mut`, `false` para `&`;
- `span`: posición del código fuente, usada por el diagnóstico;
- `born_arena`: profundidad de arena en la que nació.

### `LoanKey`

Es una versión comparable del préstamo usada para saber si el estado cambió durante el punto fijo.

### `LoanSet`

Es el libro de permisos activos. Mantiene cuatro índices:

- `by_place`: préstamos agrupados por el lugar prestado;
- `by_id`: búsqueda directa por identidad;
- `value_carrier`: préstamos transportados por cada valor SSA;
- `slot_carrier`: préstamos almacenados en cada slot local.

Los carriers contienen **conjuntos** de `LoanId`, no uno solo. Esto es necesario después de un `if`:

```kroa
let mut r = &x
if condition:
    r = &y
```

Después del join, `r` puede transportar el préstamo de `x` o el de `y`. Conservar ambos es un análisis **may-analysis** conservador: si un préstamo puede existir en algún camino, se mantiene.

## 6. Places y raíces de alias

Dos expresiones diferentes pueden señalar la misma memoria. Eso se llama **aliasing**.

```kroa
let mut values: [i64; 4] = [1, 2, 3, 4]
let left = &mut values[0..2]
let right = &mut values[2..4]
```

Aunque los rangos parecen separados, la política actual de Kroa es deliberadamente conservadora: todo slice del mismo array usa la misma **alias root**, la raíz `values`. Por eso ambos préstamos se consideran solapados.

`resolve_root` sigue el mapa `place_root` hasta encontrar el lugar raíz. `ElemPtr` también hereda la raíz de su base.

Términos importantes:

- **place**: ubicación que puede ser leída, escrita o prestada;
- **projection**: parte de un place, como un elemento;
- **alias root**: raíz común usada para decidir si dos projections pueden solaparse;
- **aliasing conservador**: rechazar algún caso seguro para no aceptar uno peligroso.

## 7. Liveness: descubrir el último uso

**Liveness** o vivacidad responde:

> ¿Este valor todavía puede ser usado desde este punto?

Se calcula hacia atrás:

1. se empieza al final del bloque;
2. los usos hacen que un valor esté vivo;
3. una definición nueva mata la versión anterior;
4. la información de los sucesores se une;
5. se repite hasta que nada cambia.

Ese último estado estable se llama **punto fijo** (*fixed point*).

### Por qué hay vivacidad especial para slots

`Load slot` lee el contenido del slot, así que ese contenido debe estar vivo.

`Store slot, new_value` reemplaza el contenido anterior. Escribir el casillero no significa leer la referencia vieja. Por eso el `Store` mata la vivacidad del contenido anterior.

Esta diferencia permite:

```kroa
let mut r = &mut x
*r = 2
r = &mut x  # el Store reemplaza la referencia anterior
```

Sin esa regla, el simple acto de reemplazar `r` mantendría vivo para siempre el préstamo viejo.

`compute_liveness` calcula la vivacidad al entrar a cada bloque. `compute_live_after` obtiene la vivacidad en cada punto entre instrucciones. `loan_operands` distingue lecturas reales de reemplazos de slots.

## 8. El worklist y el análisis forward

Después de calcular liveness, `check_function` ejecuta un análisis hacia delante.

Una **worklist** es una cola de bloques pendientes. El algoritmo:

1. comienza en el bloque de entrada;
2. aplica cada instrucción al estado;
3. envía una copia del estado a cada sucesor;
4. une estados cuando varias carreteras llegan al mismo bloque;
5. vuelve a procesar el bloque si el join añadió información;
6. termina al alcanzar un punto fijo.

`FlowState` transporta:

- profundidad actual de arena;
- conjunto de préstamos;
- provenance de valores de arena guardados en slots.

El `join` hace una unión conservadora. En ramas, “puede estar activo” se trata como “activo”. Esta decisión evita aceptar un programa peligroso solo porque otra rama era segura.

## 9. Cómo se procesa cada instrucción importante

### `Ref`

1. resuelve la raíz del place;
2. consulta los préstamos activos de esa raíz;
3. permite otro `&` si todos son compartidos;
4. rechaza `&mut` si existe cualquier préstamo;
5. rechaza `&` si existe un `&mut`;
6. registra el nuevo préstamo y su carrier SSA.

El error es `E0400`.

### `Store`

Comprueba si se escribe directamente en un place con cualquier préstamo activo.
Una escritura mediante el carrier de un `&mut` sí es válida; escribir el place
original “por detrás” del préstamo no lo es. Un conflicto emite `E0401`.

También reemplaza el conjunto de préstamos transportado por el slot de destino y actualiza su provenance de arena.

### `Load`

Rechaza una lectura directa del place original si existe un `&mut` activo,
porque el préstamo es exclusivo. Después recuerda de qué raíz salió el valor y
copia al nuevo valor SSA los préstamos o provenance almacenados en el slot.

### `Move`

Busca el place real del valor. Si el place aún tiene préstamos activos, emite `E0402`, porque moverlo invalidaría referencias existentes.

Si lo movido es una referencia o un puntero de arena, su provenance se copia al resultado.

### `ElemPtr`

Asocia el puntero del elemento con la raíz de su array o slice.

### `ArenaEnter` y `ArenaExit`

`ArenaEnter` incrementa la profundidad. `ArenaExit` la reduce y termina préstamos nacidos dentro de la arena que sale.

Una salida sin entrada correspondiente produce `E0404`.

### `ToCString` y `ArenaAlloc`

Marcan el resultado con provenance de la arena actual. **Provenance** significa “de dónde viene este puntero y qué almacenamiento lo mantiene válido”.

Aunque el tipo sea `c_string` y no `&T`, sigue siendo un puntero respaldado por memoria de arena. Por eso `needs_borrow_check` se activa tanto para referencias como para arenas.

### `Return`

El valor de retorno se evalúa antes de emitir `ArenaExit`. Así el checker conserva su provenance real.

Se rechaza:

- devolver una referencia creada hacia almacenamiento local;
- devolver un puntero que puede depender de una arena local.

Se permite devolver una referencia recibida del llamador:

```kroa
fn identity(x: &i64) -> &i64:
    return x
```

El diagnóstico estable es `E0403`.

## 10. Muerte de préstamos

Después de cada instrucción, `retain_live` conserva un préstamo solo si algún carrier vivo todavía lo contiene.

Esto no “libera memoria”. Solo elimina un permiso del modelo estático del compilador.

La frase técnica es:

> El transfer function aplica el efecto de la instrucción y después restringe el conjunto de loans a los carriers presentes en el live-after del program point.

## 11. Diagnósticos

- `E0400`: conflicto entre préstamos.
- `E0401`: escritura mientras existe un préstamo compartido.
- `E0402`: movimiento mientras el place está prestado.
- `E0403`: referencia o puntero respaldado por almacenamiento local que escapa.
- `E0404`: entradas y salidas de arena desbalanceadas.

`report_once` evita repetir el mismo error cuando el punto fijo visita varias veces una instrucción.

Cada diagnóstico incluye:

- causa principal;
- posición fuente;
- nota conceptual;
- ayuda concreta para corregirlo.

## 12. Qué comprueban las pruebas

`tests/borrow_check.rs` cubre:

- dos `&mut` realmente solapados;
- slices con la misma raíz de array;
- dos préstamos compartidos;
- préstamo compartido seguido de mutable después del último uso;
- conflicto conservado después de un join;
- préstamo local muerto antes del join;
- reemplazo de una referencia almacenada en un slot;
- convergencia con préstamos dentro de un bucle;
- escritura durante un préstamo compartido;
- referencia a almacenamiento local que intenta escapar;
- retorno válido de una referencia recibida;
- escape de `c_string` respaldado por arena.

## 13. Límites intencionales

NLL-lite todavía no es un sistema completo de regiones como el de Rust:

- los slices del mismo array siempre se consideran solapados;
- las llamadas que devuelven referencias heredan de forma conservadora los préstamos de todos los argumentos prestados;
- no hay anotaciones explícitas de lifetime;
- el análisis interprocedural se limita a ese resumen conservador de llamadas;
- los joins prefieren seguridad y pueden rechazar casos válidos difíciles;
- no se demuestra aritméticamente que dos índices dinámicos sean distintos.

Las referencias mutables no son `Copy`: `let b = a` mueve un `&mut T`. Un reborrow de la forma `&mut *p` transfiere el préstamo exclusivo en lugar de inventar un lugar temporal.

Esto no significa que el checker “adivine”. Significa que usa una aproximación conservadora claramente definida.

## 14. Respuesta corta para explicar el sistema

Si alguien pregunta “¿cómo funciona el borrow checker de Kroa?”, una respuesta precisa sería:

> El compilador baja el programa a KIR SSA-like, construye su CFG y calcula liveness backward hasta un punto fijo. Luego propaga forward un conjunto de loans por places normalizados a alias roots. Los valores SSA y slots locales transportan conjuntos de LoanId, los joins hacen unión conservadora y cada préstamo muere después del último carrier vivo. Además se propaga provenance de arena para impedir escapes. Los conflictos producen diagnósticos E0400–E0404.

Y en palabras sencillas:

> El compilador dibuja todas las carreteras posibles del programa, sigue quién tiene cada permiso de lectura o escritura y retira el permiso justo después de su último uso. Si dos permisos peligrosos podrían encontrarse en alguna carretera, detiene la compilación.
