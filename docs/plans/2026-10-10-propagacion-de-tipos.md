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

## 1. Estado al cerrar 2026-10-10

Hecho (commits `0c3a2add`..`f58fc56e`, tests 185 y 186):

- Cero abandonos reales del JIT en corpus y benchmarks.
- `try/catch` compila (antes ninguna función con `try` lo hacía).
- Bucles versionados de cota constante compilan.
- Parámetro con valor por defecto recupera su tipo.
- Llamada directa a función libre entrega los argumentos con la forma
  de la firma (`int` → `int?`, argumento omitido → `null`).
- Veracidad de `int`/`float` es una comparación, no un `Cast`.
- `||` y `&&` devuelven un operando; sus tipos se afinan.
- Literal de array con un elemento `dynamic` es `dynamic[]`.
- `e as Error` funciona sin imports.

Frontera de `dynamic` (sesión anterior): completa. Ver memoria
`varn-dynamic-boundary`.

Cobertura estática medida antes de esta tanda (funciones tipadas):
benchmarks 58 %, corpus 66 %. Genéricos que quedaban en funciones de
benchmarks: `GetProperty` 340, `Eq` 157, `CallMethod` 50, `GetIndex` 8.
Re-medir al empezar (skill `varn-static-coverage`); estas cifras caducan.

## 2. Pendiente, por orden

Cada item: qué falta, dónde, cómo se mide.

### 2.1 Forma de llamada en métodos y closures

La adaptación de argumentos a la firma solo cubre `Resolution::DirectFn`
(`from_tir/build/call.rs::lower_args_as_declared`). El JIT deduce la
forma nativa de los tipos de los argumentos
(`clif/from_ssa/call/native_entry.rs`, `NativeShape::new`) y la compara
con la del destino; si no casan, cada llamada va al camino genérico.

- Falta: `lower_method_call` (`expr_invoke.rs`), `SuperCall`,
  `SuperMethodCall`, constructores (`new C(...)`) y llamadas a través
  de un valor con tipo función conocido.
- Sonda: `scratchpad` no persiste; rehacer `df2.vn` (función con `int?`,
  con `str`, con argumento omitido, llamadas 5M veces) en versión
  método y constructor.
- Medida: ns por llamada frente a la misma llamada con tipos exactos
  (referencia: 7 ns directa, 55–100 ns genérica).

### 2.2 Tipos que `HirType` no distingue

`HirType` tiene Int, Float, Bool, Str, Ref, Dynamic, Array, Map, Set,
Tuple, Class, Nullable. Char, Bytes, Enum, Decimal, BigInt, Fn y los
tipos de objeto caen en `Ref` (`from_tir/ty.rs`) y pierden su identidad
antes de elegir opcode.

- Empezar por Enum y Char: son los que alimentan `Eq` genérico.
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

`new Headers({})` y similares van a verificación en runtime porque el
emisor no conoce la firma del constructor de una clase importada o
nativa. Hace falta exportar la firma del constructor junto a la clase.

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

## 3. Decisiones de lenguaje sin tomar

No se implementan sin que el dueño decida:

- `xs[i]` y `s[i]` fuera de rango devuelven `null` con tipo `T`. O el
  tipo del acceso pasa a ser `T?`, o el acceso lanza. Mientras siga
  así, un `int` leído de un array puede ser `null` y no se puede plegar
  `IsNull` sobre valores que vienen de un índice.
- Un string vacío es verdadero (solo son falsos `null`, `false`, `0`,
  `0.0` y `NaN`). Está cubierto por el test 186; confirmar que es lo
  querido.
- En `a && b`, solo se descarta el tipo de la izquierda cuando es una
  instancia de clase. Arrays, tuplas y funciones también son siempre
  verdaderos y podrían afinarse igual.

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
