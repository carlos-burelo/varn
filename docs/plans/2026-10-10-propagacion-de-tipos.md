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

## 2. Pendiente, por orden

Cada item: qué falta, dónde, cómo se mide.

### 2.1 Forma de llamada: lo que queda

- Un constructor con parámetro por defecto no se inlinea
  (`inlinable_constructor` lo rechaza): 59 ns frente a 1,8 del
  constructor sin defecto.
- Métodos de clases importadas: el emisor no tiene sus firmas
  (`ForeignClass.methods` son solo nombres), así que no hay forma ni
  slot de vtable. No vale con añadir la firma: el slot lo decide el
  módulo dueño. Hace falta exportar firma + slot juntos.
- `new Box(1)` sin instanciar no valida los argumentos (no hay con qué
  sustituir `T`); falta inferir la instanciación desde los argumentos.
- Tras `x ||= v`, `x` no se estrecha a no nulo.

### 2.2 Tipos que `HirType` no distingue

`HirType` tiene Int, Float, Bool, Str, Ref, Dynamic, Array, Map, Set,
Tuple, Class, Nullable. Char, Bytes, Enum, Decimal, BigInt, Fn y los
tipos de objeto caen en `Ref` (`from_tir/ty.rs`) y pierden su identidad
antes de elegir opcode.

- Medido en la segunda tanda: la igualdad entre enums no es lenta
  (18 ns con la llamada, igual que int). El `Eq` genérico ya resuelve en
  línea los bits idénticos y los dos SSO; solo va al helper cuando los
  operandos difieren y son bool, enum o referencias. Lo caro del enum es
  llegar a la variante: `Color.Red` es un `GetProperty` por nombre
  (36–53 ns tras el arreglo, 84–90 antes). El arreglo de cimiento es
  que la variante sin payload sea un slot resuelto en compilación:
  `Resolution::StaticField` existe en el TIR y nadie lo produce.
- Después, Enum y Char en `HirType`: son los que alimentan `Eq` genérico
  cuando los operandos difieren.
- `Eq` tipado para str, bool, enum y referencia (hoy 1,2 ns genérico
  frente a 0,4 tipado; `eqStrLong` 12,6 ns).
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
nativas (`is_builtin_or_intrinsic`).

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
