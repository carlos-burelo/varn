# Propagación de tipos desde los cimientos (living, 2026-10-10)

Objetivo: que lo que el checker sabe llegue intacto hasta el código
máquina, sin que ninguna capa lo pierda, lo invente o lo contradiga.
Cadena: checker `Type` → `BackendTy` (varn-tir, `varn-emit/src/ty/lower.rs`)
→ `HirType` (`varn-compiler/src/hir/mod.rs`, `from_tir/ty.rs`)
→ `SlotKind` = lo que ve el JIT.

Regla de trabajo (Ley 14): los cimientos primero. Si al avanzar aparece
una mentira de tipo, un hueco entre capas o un bug de lenguaje, se
arregla ahí, en su capa, antes de seguir. Se admiten cambios que rompen
código existente. Si este documento contradice al código, manda el
código (Ley 12).

## 0. Cómo se valida cada item

1. Matriz release de 4 cuadrantes de `tests/main.vn` (dev / `@embedded`
   × JIT / `VARN_NO_JIT=1`) + corpus `tests/errors/*.vn`. Caché nueva
   (`VARN_CACHE_DIR`) en cada corrida. No se corre `cargo test`.
2. Barrido de abandonos: `vn debug -p bails` sobre `tests/*.vn` y
   `tests/benchmarks/vn/*.vn` (ojo: `vn/`, no `tests/benchmarks/*.vn`).
   Debe quedar solo «generator/async not JIT-able in fase B».
3. Todo test nuevo se mira con `vn debug -p tiers`: una función que
   abandona pasa la suite igual, interpretada.
4. Rendimiento: A/B pareado contra el binario anterior (dos binarios,
   bloques alternos, mínimo de 3×10, descartar la primera corrida).
   `xtask compare` solo en frío: justo después de compilar dio 3, 6 y 2
   victorias en tres pasadas seguidas.
5. Un commit atómico por arreglo, en `main`. Sin stash ni ramas.

## 1. Estado

### Primera tanda (commits `0c3a2add`..`f58fc56e`, tests 185 y 186)

- Cero abandonos reales del JIT en corpus y benchmarks.
- `try/catch` compila (antes ninguna función con `try` lo hacía).
- Bucles versionados de cota constante compilan.
- Parámetro con valor por defecto recupera su tipo.
- Llamada directa a función libre entrega los argumentos con la forma
  de la firma (`int` → `int?`, argumento omitido → `null`).
- `||` y `&&` devuelven un operando; sus tipos se afinan.
- Literal de array con un elemento `dynamic` es `dynamic[]`.
- `e as Error` funciona sin imports.

Frontera de `dynamic` (sesión anterior): completa. Ver memoria
`varn-dynamic-boundary`.

### Segunda tanda (commits `b9744f27`..`6fd7a992`, tests 187 y 188)

Decisiones de lenguaje de la sección 3, y lo que destaparon:

- Índice fuera de rango lanza; leer por clave da `V?`; solo `null` y
  `false` son falsos; opcional y `null` son lo mismo.
- 2.1 hecho: toda llamada con firma conocida (método, constructor,
  valor función, `super`) entrega los argumentos con la forma
  declarada, desde un único punto del emisor (`guard_args` +
  `fill_omitted`). Se borró `lower_args_as_declared` del compilador.
- 2.5 hecho para clases importadas raíz: `ForeignClass` lleva el tipo
  del constructor.
- Leer un estático de clase o una variante de enum: 84–90 → 36–53 ns.

Bugs previos que aparecieron por el camino, cada uno arreglado en su
capa:

- El checker no validaba los argumentos de `new` (ni tipos ni aridad):
  `new K("x")` con `constructor(n: int)` compilaba y rompía en runtime.
- `m[k] ?? d` sobre `Map<str, int>` fallaba en runtime con «expected
  'int', got 'null'»: el emisor re-derivaba el tipo del elemento en
  tres sitios y decía `int`.
- Un estrechamiento restaba al tipo declarado, no al ya estrechado:
  tras `if (v == null) return`, el `else` de `Array.isArray(v)`
  recuperaba el `null`. Seis estrechadores leían el tipo cada uno por
  su cuenta.
- `function f(s?: str)` veía `s` como `str` en el cuerpo.
- Un miembro `tag?: T` no aceptaba `null` aunque se leía como `T?`.
- Un `int` literal hacia `float?` llegaba como int dentro del `float?`
  (`typeof` decía `int`).
- La rama `else` de `if (n)` con `n: int?` estrechaba a `null` aunque
  `n` fuera 0. Con la regla nueva es correcto por construcción.
- Cada lectura de propiedad por camino lento alocaba un `Arc<str>` para
  un nombre corto (SSO).

Sonda de 2.1 (ns por llamada, perfil `quick`, mínimo de 7 × 5M, mismo
estado térmico antes → después):

| Llamada | antes | después |
|---|---|---|
| método, `int` → `int?` | 64 | 20 |
| método, (`str`, `int` → `int?`) | 123 | 21 |
| método con argumento omitido | 107 | 12 |
| constructor con argumento omitido | 123 | 59 |
| valor función, `int` → `int?` | 108 | 28 |
| método con tipos exactos (referencia) | 10,6 | 10,6 |

La máquina termaliza: las sondas se repiten alternando binarios; una
sola pasada dio hasta 3× de diferencia en la misma medida.

### Tercera tanda (commits `52fbbef6`..`75074dcc`, tests 189 a 192)

- La variante de un enum es un slot global resuelto en compilación,
  local o importado: `Color.Red` 28 ns -> 1,4 ns; `LogLevel.Warn`
  importado 121 ns -> 1,2 ns (dos lecturas).
- `new` deduce los argumentos de tipo y valida siempre el constructor.
- Un constructor con valores por defecto se inlinea: 20,6 ns -> 0,4 ns
  (igual que sin defecto). El mensaje de `46ce2cab` dice «~59 ns»: esa
  cifra era de otra sonda; la medida con esta es 20,6.
- Un método de clase importada recibe los argumentos con su forma:
  `int` -> `int?` 44,6 ns -> 15,6 ns (el mismo método local: 14,8).
- El estrechamiento sigue a la asignación.

Bugs previos que aparecieron, cada uno arreglado en su capa:

- `new Box(1)` era `Box` sin instanciar: `b.value` tenía tipo `T` y el
  constructor no se validaba (`new Pair(1, "x", "tres")` compilaba).
- El valor por defecto de un parámetro de tipo (`class Cell<T = int>`)
  se parseaba y no se guardaba en ningún sitio.
- La inferencia de genéricos emparejaba uniones por posición, `null`
  incluido: un `int` dado a `label: L?` no fijaba `L`.
- `new Box<float>(2)`, `Map<str, float>.set("a", 1)` y
  `new Box<decimal>(1)` guardaban un int: la conversión se hacía contra
  la firma del llamado, que en una clase genérica tiene `T` borrado.
- El checker no visitaba el valor por defecto de un parámetro:
  `function a(x: float = "s")` compilaba, un nombre inexistente también.
  En `f(x: float = 1)` el compilador deducía del relleno que `x` era
  int, y `f(2.5)` lanzaba «expected 'int', got 'float'».
- `[...conjunto]`, `[..."ab"]`, `[...generador]` y `[...quizaNulo]`
  compilaban y lanzaban al ejecutar; `[...tupla]` también lanzaba.
- Asignar no tocaba el estrechamiento: tras `if (x == null) return`,
  `x = null; x.length` compilaba. Igual dentro de un `if` y en un bucle.
  Y al revés: ni `x = 5` quitaba el `null` de un `int?`.

## 2. Pendiente, por orden

Cada item: qué falta, dónde, cómo se mide.

### 2.1 Forma de llamada: lo que queda

- `new` de una clase importada: 92 ns frente a 14 de un `new` local no
  inlineado (0,2 inlineado). Va por llamada genérica al valor clase.
  Para alocar directo y llamar al constructor por método hace falta que
  `ForeignClass` diga si la clase está decorada (un decorador puede
  devolver otra cosa que una clase). No fiar el tamaño de payload al
  módulo que importa: el JIT lo escribe en la cabecera sin compararlo
  con el layout real de la clase (`emit_alloc_instance`); para una clase
  ajena hay que alocar por el helper, que usa el layout del dueño.
- Slots de método para clases importadas: medido, no hace falta. Con
  tipos exactos la llamada por nombre con caché iguala al slot (6,8 ns
  las dos).
- La forma de un argumento sale de dos sitios: la firma del llamado
  (`guard_args`) y los parámetros que validó el checker (`call_params`,
  usados para ensanchar y para clases sin tabla de métodos). El segundo
  es la fuente correcta; falta mover `guard_args` a él y borrar el caso
  `int` -> `float?` que quedó dentro de `as_declared`.
- `new Map()` sigue siendo `Map<dynamic, dynamic>` sin avisar, cuando
  `new Set()` y cualquier clase de usuario ya exigen los argumentos de
  tipo (`VN3201`).
- Un tipo genérico escrito sin argumentos en una anotación (`x: Cell`)
  no toma los valores por defecto de sus parámetros; solo `new` lo hace.
- El estrechamiento por asignación no cubre miembros (`this.v ??= 3`),
  ni asignaciones hechas desde un closure, y una asignación en una rama
  de un `if` invalida también la otra.

### 2.2 Tipos que `HirType` no distingue

`HirType` tiene Int, Float, Bool, Str, Ref, Dynamic, Array, Map, Set,
Tuple, Class, Nullable. Char, Bytes, Enum, Decimal, BigInt, Fn y los
tipos de objeto caen en `Ref` (`from_tir/ty.rs`) y pierden su identidad
antes de elegir opcode.

- Hecho en la tercera tanda: la variante es un slot global por módulo
  (`EnumInfo::variant_global`). Quedan por nombre los estáticos de clase
  (`Contador.total`): son mutables, así que un global aparte divergiría
  de la lectura dinámica. Necesitan slots en el objeto clase
  (`ClassObj.statics` es un `HashMap`) y es lo que produciría
  `Resolution::StaticField`, que sigue sin productor.
- `Eq` tipado, medido antes de hacerlo: la identidad de bits solo vale
  para bool, instancias de clase y enums sin payload. `char` es un
  objeto de heap que se compara por contenido y una variante con payload
  se compara por estructura (`variant_eq`). El `Eq` genérico solo cae al
  helper cuando los operandos difieren: unos 5 ns en ese caso (int 3,8,
  bool 5,4, referencia 8,4, enum 10,6, char 9,0 ns con la llamada).
  Pide un opcode nuevo o Enum/Char en `HirType`; no se hizo.
- `compare::eq` cierra con `_ => return false` y deja que
  `variante == int` compare el tag: revisar contra la Ley 7 y contra la
  regla de no mezclar tipos en `==`.
- Medida: recuento `EqInt`/`Eq` y `GetFixedField`/`GetProperty` en
  benchmarks, y A/B pareado. Un mejor recuento no es una mejora de
  tiempo: se confirma con el A/B.

### 2.3 `DynReason` que deberían ser tipos

`BackendTy` lleva funciones, tareas, generadores y objetos como
`Dynamic(DynReason::{Callable, Awaitable, Yields, Shape})`. Son tipos
conocidos disfrazados de dinámicos: todo consumidor tiene que mirar la
razón para no tratarlos como `dynamic` de verdad.

- Graduarlos a variantes propias de `BackendTy` y revisar cada `match`
  (Ley 7, sin `_ =>`).
- Va antes de 2.4: los tipos de objeto con layout fijo lo necesitan.

### 2.4 Tipos de objeto con layout fijo

Los tipos de objeto son exactos (memoria `varn-object-types-are-exact`)
pero se leen por `GetProperty` con caché (1,6 ns frente a 0,6). Es la
Fase E de `2026-10-09-representacion-un-bloque-por-valor.md`; ese plan
manda en el diseño. Aquí solo se anota que es el mayor bloque de
genéricos que queda (340 `GetProperty` en benchmarks).

### 2.5 Firmas de constructores de clases externas

Hecho para clases importadas raíz (`fd70b173`). Falta: clases
importadas con padre (hoy `collect_foreign_classes` las salta) y clases
nativas (`is_builtin_or_intrinsic`). Desde la tercera tanda la forma de
los argumentos ya no depende de esto (`call_params`); lo que sigue
dependiendo es poder alocar e inlinear.

### 2.6 Coste de entrar en un `try` compilado

65 ns por entrada (2M llamadas: 130 ms compilado, 190 ms interpretado).
Viene del helper `try_push` y de exigir activación con marco
(`proto.resumes_in_interpreter()` ⇒ `Activation::Framed`). No es
propagación de tipos; se anota porque `try` es ubicuo en `std`.

### 2.7 Funciones async sin JIT

Por diseño hoy («fase B»). Es la mayor masa de código interpretado:
todo `it(...)` de la suite y buena parte de `std`. Depende del rediseño
async aprobado (memoria `varn-async-redesign-approved`).

### 2.8 Llamada a método dinámico

Hace doble búsqueda (unos 80 ns frente a 45). Pendiente de la sesión
anterior.

### 2.9 Encontrado y sin arreglar

- `expect(a).toEqual(b)` de `std:test` no compara arrays ni objetos por
  contenido: `isEq` solo mira identidad y strings. Ningún test lo usaba
  con arrays.
- Un `int` variable no se acepta donde va un `float` (solo el literal),
  pero sí donde va un `decimal` o un `bigint`. Y la asignación rechaza
  hasta el literal (`a.value = 3` con `value: float`) cuando el
  inicializador y el argumento lo aceptan. Falta una sola regla.
- `function* gen(): Generator<int>` tiene tipo
  `Generator<Generator<int>>` al llamarla.
- Los elementos de un spread no se comprueban contra el tipo esperado
  del array (`const xs: int[] = [...textos]`).

## 3. Decisiones de lenguaje (tomadas 2026-10-10, delegadas por el dueño)

Criterio: una regla que quepa en una frase, que el tipo no mienta, y
ningún pecado heredado (tablas de falsos, `undefined`, huecos en
arrays). Cada una declara qué se gana, cómo se verifica y qué se borra
en su commit.

- **Índice fuera de rango lanza `RangeError`** (`b9744f27`). `xs[i]`,
  `s[i]`, `bytes[i]` y `rango[i]` tienen tipo `T` y ahora es verdad,
  también al escribir (se borra el relleno con `null`). El acceso que
  puede fallar es `at(i): T?`; crecer es `push`. Se descartó `T?`:
  obliga a `!` en cada bucle y mete nullables en el camino caliente.
- **Leer por clave da `V?`** (`e5ce9bc9`). En `Map<K, V>` y
  `{ [key: K]: V }` la clave ausente es un caso normal, no un error.
  La escritura pide `V`. Asignación compuesta aritmética sobre un
  destino nullable es error.
- **Solo `null` y `false` son falsos** (`f636c2ad`). `0`, `0.0`, `NaN`
  y `""` son verdaderos. Condicionar sobre un tipo que nunca es falso
  (int, str, array, instancia…) es error `VN3027`. Con `x: T?`,
  `if (x)` es `x != null` exacto en las dos ramas y compila a `IsNull`.
- **`&&` y `||` se afinan con la misma regla** para cualquier tipo
  (`never_false`), no solo instancias de clase.
- **Opcional y `null` son lo mismo** (`bac1cf33`, `2c1249fc`). Un
  miembro `tag?: T` es `T?`; un parámetro `p?: T` es `T?` en el cuerpo;
  quien llama puede pasar `T?` a cualquier parámetro opcional o con
  valor por defecto (el `null` toma el defecto).

Tercera tanda:

- **`new` deduce los argumentos de tipo; lo que no se deduce ni tiene
  valor por defecto es error `VN3201`** (`e3c679cc`). Se descartó
  rellenar con `dynamic`: es el `any` implícito de otros lenguajes.
- **El valor por defecto de un parámetro se comprueba contra su tipo**
  (`cbf7dad8`).
- **Un spread toma un array o una tupla, en un literal igual que en una
  llamada** (`dc287ea2`). Se descartó aceptar cualquier iterable: hoy
  no hay protocolo de iteración que el literal pueda recorrer sin
  ejecutar código (generadores), y aceptarlo a medias es peor.
- **Asignar deshace el estrechamiento; llenar con un valor no nulo lo
  crea** (`b2d9e3f7`).

Las cuatro se midieron con `vn check` sobre todo `std` y `tests` antes
de fijarlas: rompían, en total, un sitio.

Medido antes de decidir: en todo `std` + suite solo 2 tests dependían
del `null` fuera de rango, 9 sitios de la lectura por clave no nula y
17 condiciones de un tipo que nunca es falso (12 en el test 186).

## 4. Trampas conocidas

- `VARN_NO_JIT=` vacío ya no desactiva el JIT; el cuadrante JIT de la
  matriz usa `X=1`.
- El cuadrante `@embedded` se cuelga en el test 111 si
  `target/debug/vn.exe` está viejo: recompilar debug y matar los `vn`
  colgados.
- El verificador de TIR exige aridad exacta en `DirectFn`
  (`varn-tir/src/verify/coherence/calls.rs`). No relajarlo: rellenar en
  el emisor.
- Un worktree nuevo no compila `varn-builtins` por los finales de línea
  de los contratos (`contracts.json` compara hash de bytes): copiar
  `crates/varn-builtins/src/modules` desde el checkout principal.
- Los scripts de edición deben conservar CRLF.
- Tras cambiar el compilador, purgar `~/.varn/cache`.
- Los tests 111 y 112 lanzan `target/debug/vn.exe` por ruta fija. Para
  correr los cuadrantes `@embedded` con otro perfil hay que copiar ese
  binario ahí, o el bundle se rechaza («compilado por otra version») y
  el proceso se queda colgado.
- `tests/main.vn` aborta en el primer error no capturado y solo enseña
  ese. Para ver todo lo que rompe un cambio de lenguaje: `vn check`
  sobre cada `.vn` de `std/` y `tests/`, y `vn run` de cada test suelto.
- El corpus `tests/errors/` no tiene corredor: cada archivo declara en
  su primera línea el código que `vn check` debe emitir.
- Un cuadrante `@embedded` falló una vez en el test 113 por un archivo
  temporal que dejó un `vn` matado; al repetir pasó.
- Un test que no compila hace que `tests/main.vn` salga con código 1 y
  sin línea `PASSED`. Mirar la salida antes de commitear: en la tercera
  tanda se commiteó un test roto por encadenar la corrida y el commit.
- El checker solo acepta un `int` donde va un `float` si es un literal.
  Un test que pase una variable `int` a un parámetro `float` no compila.
- `tests/errors/` y `/tmp/chk.sh` miden el impacto de una regla nueva,
  pero `std` se compila dentro del build: si `std` rompe, es el build el
  que falla, no la medición.
