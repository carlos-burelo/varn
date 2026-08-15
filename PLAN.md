# Plan: deuda restante en `varn-vm`

Estado tras el refactor F0–F6 (commits `b038b9c` → `921a56b`, tag
`vm-refactor-complete`). Lo que sigue es lo que la auditoría encontró y **no**
se arregló, ordenado por lo que cuesta si se ignora.

Las referencias son por **nombre de símbolo** primero y línea después: las
líneas se mueven, los nombres no.

## Progreso

| | Estado | Commit |
|---|---|---|
| **P1** desenrollado de excepciones ×3 | ✅ hecho | `96279a3` |
| **P2** superficie pública | ✅ hecho | `6f66c7c` |
| **P3** cola de `JitHelpers` sin test | ✅ hecho | `e15f56f` |
| **P4** tupla posicional en `resolve_native_op` | ✅ hecho | `5089959` |
| **P5** archivos sobre 500 | pendiente | |
| **P6** función comentada | ✅ hecho | `e15f56f` |
| **P7** puntos de aborto | 🟡 parcial | `e13f325`, `1e57454` |
| **P8** deps sin uso | ✅ hecho | `e15f56f` |
| **P9** indentación | ✅ hecho | `bda5167` (`cargo fmt` del proyecto) |

---

## Estado de partida

| | |
|---|---|
| Archivos `.rs` en `varn-vm` | 95 |
| Archivo mayor | `exec/dispatch/mod.rs`, 772 líneas |
| Archivos > 1000 líneas | 0 |
| Funciones muertas | 0 (el compilador las detecta desde F1) |
| Listas del ABI JIT a sincronizar a mano | 1 (`varn-jit/src/helper_abi.rs`) |
| `#[allow(...)]` en el crate | 8, todos justificados |
| `TODO` / `FIXME` / `HACK` | 0 |
| Archivos huérfanos | 0 |

---

## Protocolo de validación

**Obligatorio para toda tarea de este plan.** Ninguna se declara terminada sin
las cuatro.

1. `cargo check --workspace --all-targets` → 0 errores, 0 warnings.
2. `cargo test -p varn-vm` → verde.
3. `cargo run --release --bin vn -- run ./tests/main.vn` → **991/0**, en las tres
   procedencias:
   - árbol: `./target/release/vn.exe run ./tests/main.vn`
   - intérprete: `VARN_NO_JIT=1 ...`
   - bundle: `VARN_STD=@embedded ...` (purgar con `vn cache clean` al cambiar)
4. `cargo run --release --bin vn -- bench ./tests/main.vn -v` → la **cobertura
   clif** debe seguir en `31 321 / 32 159` frames (31 275 que entran compilados
   más 46 rescatados por OSR). Si cambia, el refactor movió comportamiento, no
   sólo código.

   Dos correcciones sobre lo que decía antes esta línea. El `31 285` nunca se
   reprodujo: el binario de `1d7656d` da 31 264 sobre las 72 pruebas de
   entonces, y el denominador subió a 32 159 al añadirse
   `tests/73-str-charcode-json-shape.vn`. Y el titular contaba sólo las
   entradas compiladas, así que reportaba 880 frames «al intérprete» cuando 45
   de ellos habían terminado compilados — la cobertura sale ahora de
   `JitStatsSnapshot::machine_code_frames`, única definición que usan el
   titular, la tabla y el escalado de los contadores del intérprete.

Para tareas que tocan rutas calientes (P1, P5), además medición pareada.

### Cómo medir aquí, y por qué el método obvio miente

Dos binarios `vn.exe` **distintos se invalidan mutuamente la caché de
bytecode**: cada cambio de binario cuesta ~180 ms de recompilación. Medido en
`tests/main.vn`: la 1ª corrida de cada par ~125 ms, la 2ª ~310 ms, **sea cual
sea el binario**. El mismo binario 8 veces seguidas no oscila (323 ms la
primera, luego ~130 estable) — eso descarta térmica y descarta scheduling
P-core/E-core (fijar afinidad no lo quita).

Consecuencia: **alternar corrida a corrida mide el rebuild de caché, no el
código.** Con ese método la razón oscila 0.40 / 2.45 según el slot y su mediana
es un artefacto del número de rondas.

Método correcto:

1. Construir el binario base desde el commit de referencia
   (`git worktree add <tmp> <commit>` + `cargo build --release --bin vn`).
2. Alternar **bloques**, no corridas: N corridas seguidas del mismo binario.
3. **Descartar la 1ª corrida de cada bloque** — es la que paga el cambio de caché.
4. Alternar qué binario abre cada bloque, para que la deriva térmica se cancele.
5. Reportar **min y mediana**. Con la máquina cargada el min es el estadístico
   robusto: en una tanda la mediana del propio baseline se movió 12 % entre dos
   pasadas mientras el min se movía 2 %.

Con n ≥ 24 y máquina tranquila esto resuelve ±1 %. Toda razón entre 0.98 y 1.02
es ruido en este host, y hay que decirlo así en vez de reportarla como mejora.

`benchmarks/compare.ps1` usa min-de-5 y está expuesto al mismo efecto, pero sus
rivales sirven de control: **si bun y node se mueven en la misma dirección y
magnitud, es la máquina, no Varn.**

---

## P1 — El desenrollado de excepciones está escrito tres veces — ✅ hecho (`96279a3`)

Unificado en `exec::frame_ctrl::unwind_to_handler`, junto a
`resolve_constructor_return`. Antes de colapsarlas se compararon las tres línea
a línea: difieren **sólo** en cómo llegan al `ExecCtx` y de dónde sale el valor
lanzado. Ninguna llevaba una regla extra, así que no se perdió nada — y esa
comprobación importaba, porque una diferencia habría sido o un bug o un
requisito no documentado, y borrarla a ciegas destruye la evidencia en ambos
casos.

`handler` se toma **por valor**: todos los llamantes ya lo habían sacado de
donde vivía, y la firma es lo que hace eso innegociable.

Validado con 11 / 21 / 47 comprobados explícitamente, cobertura clif intacta, y
pareado n=30: **0.975 min / 1.010 mediana**.

<details><summary>Diagnóstico original</summary>

Es exactamente el mismo defecto que la regla de retorno de
constructor que se unificó en `921a56b`, pero sobre semántica de excepciones.

Tres copias del mismo bloque:

| Sitio | Contexto |
|---|---|
| `exec::dispatch::run_until_inner_raw` (mod.rs ~506) | intérprete, `*ctx` crudo |
| `exec::dispatch::jit_frame::run_compiled_frame` (~92) | salida de frame compilado |
| `exec::ctx_tasks::run_lazy_task_sync` (~158) | fork de tarea, contexto propio |

Los tres hacen la misma secuencia: sacar frames hasta `handler.frame_depth`,
`record_frame_pop` por cada uno, cerrar upvalues, truncar la pila al
`register_count` del frame receptor, escribir el valor lanzado en
`handler.err_reg` (creciendo la pila si el slot no existe) y poner
`ip = handler.catch_ip`.

**Por qué importa:** un `catch` que se comporte distinto según si el frame venía
del intérprete, de código compilado o de una tarea es un bug que ningún test de
un solo tier detecta. Es la misma clase de fallo que la regla de constructor, y
esta es peor porque el control de errores es lo que el usuario del lenguaje usa
para razonar sobre corrección.

**Cómo:** extraer a `exec::frame_ctrl` — donde ya vive
`resolve_constructor_return` por la misma razón:

```rust
/// Desenrolla hasta el handler y deja el frame receptor listo para ejecutar
/// su bloque catch. Devuelve el índice del frame que queda arriba.
pub(crate) fn unwind_to_handler(
    ctx: &mut ExecCtx,
    handler: TryHandler,
    thrown: VmValue,
) -> usize
```

Las tres variantes difieren sólo en cómo llegan al `ExecCtx`; las dos de puntero
crudo pasan `&mut *ctx`.

**Aceptación:** las cuatro validaciones, más medición pareada (toca el bucle de
frames). Comprobar explícitamente los tests de `main.vn` que ejercitan `try` a
través de los tres caminos: **11 (Error handling)**, **21 (Async/await)**,
**47 (Isolates)**.

**Riesgo:** medio. Es semántica de excepciones y las tres variantes se ven
iguales pero hay que leerlas línea a línea antes de unificar — si una difiere,
esa diferencia es o un bug o un requisito no documentado, y hay que decidir cuál
antes de borrarla.

</details>

---

## P2 — La superficie pública sigue abierta — ✅ hecho (`6f66c7c`)

**63 → 5 `pub mod`.** Quedan `exec`, `loader`, `jit`, más `jit::helpers` y
`exec::host_values`, que varn-pipeline alcanza directamente. Los `pub use` de
`lib.rs` siguen funcionando: re-exportar un item `pub` desde un módulo privado
es legal y es el patrón estándar.

**Y pagó de inmediato** — cuatro piezas más de código muerto que los pases
anteriores no podían ver, todas tipos o variantes, no funciones:

- `PreparedCall::Native` — destruida en cuatro sitios, construida en ninguno.
  Se fueron con ella cuatro brazos de `match` muertos.
- `ControlSignal` — cuatro variantes, una sola construida y siempre
  incondicionalmente, y todos los llamantes la descartaban con `?;`. Las tres
  funciones devuelven ahora `VmResult<()>` y el enum desapareció.
- `GcError` — tres variantes, ninguna construible, así que
  `Result<usize, GcError>` era un `Result` que nunca podía ser `Err`. Cada
  llamante tenía que atender un caso imposible y `Vm::collect_gc` lo hacía con
  `.unwrap_or(0)`, que habría tragado en silencio un fallo real el día que se
  introdujera uno. Marcar y barrer recorren estructuras que el heap ya posee:
  son infalibles, y ahora lo dicen.
- Tres re-exports sin uso en `value.rs`.

Contadores de GC idénticos al baseline pre-refactor (36 minor, 2 colecciones,
112 039 liberados, 1 553 vivos) — esa es la evidencia de que quitar `GcError` no
movió comportamiento.

<details><summary>Diagnóstico original</summary>

F1 estrechó **funciones** (322 `pub(crate) fn` contra 24 `pub fn`) pero no
módulos ni tipos.

| | Declarado | Usado desde fuera |
|---|---|---|
| `pub mod` | 63 | 3 rutas (`exec`, `loader`, `jit`) |
| `pub struct` | 31 | — |
| `pub enum` | 10 | — |
| campos `pub` | 132 | — |

Más 6 tipos re-exportados por `lib.rs` (`ExecSettings`, `Vm`, `GlobalStore`,
`Heap`, `prefill_native_modules`, `varn_jit`).

**Por qué importa:** con 63 módulos públicos, cualquier cosa dentro de ellos es
API. Eso apagó el lint `dead_code` durante años y es lo que dejó acumular ~70
funciones muertas. Estrecharlo devuelve el trabajo al compilador de forma
permanente.

**Cómo:** pasar los `pub mod` a `pub(crate) mod` salvo los tres usados, y
conservar los `pub use` de `lib.rs` — re-exportar un item `pub` desde un módulo
privado es legal y es el patrón estándar. Iterar sobre los errores del
compilador, que nombran exactamente lo que hay que volver a abrir.

**Aceptación:** las cuatro validaciones. El número final de `pub` debe ser
justificable item por item.

</details>

---

## P3 — La cola de `JitHelpers` no tiene red — ✅ hecho (`e15f56f`)

`gc_safepoint` y `clif_call_fallback` movidos a la lista compartida, así que el
test `every_helper_address_is_real` ya los cubre. Los offsets de la cola tienen
ahora sus propias aserciones. **`globals_offset` queda deliberadamente fuera**:
es legítimamente 0 si `globals` resulta ser el primer campo de `ExecCtx`, y
afirmar sobre él convertiría un accidente de layout en un requisito.

<details><summary>Diagnóstico original</summary>

F3 dejó una sola lista compartida (`varn-jit/src/helper_abi.rs`) para los 106
campos de dirección de función, y un test que verifica que ninguno es 0. Los
**17 campos de la cola** escritos a mano (lib.rs 202–241, de `resolve_native_op`
a `clif_call_fallback`) quedan fuera de esa lista y fuera del test. Total de la
struct: 123.

Dos de ellos **sí** son direcciones de función: `gc_safepoint` y
`clif_call_fallback`. Un 0 ahí es un salto a la dirección nula desde código
generado, descubierto cuando algún programa llegue por primera vez a ese opcode.

**Cómo:** dos opciones, no excluyentes.

1. Mover `gc_safepoint` y `clif_call_fallback` a la lista compartida (son
   `ctx::fn as usize` como el resto; no hay razón para que estén fuera).
2. Añadir aserciones para los offsets y layouts probados: no pueden ser 0
   tampoco, salvo `globals_offset`, que legítimamente puede serlo si `globals`
   es el primer campo de `ExecCtx` — eso hay que comprobarlo, no asumirlo.

**Aceptación:** el test cubre los 123 campos, o los que queden fuera están
documentados con la razón por la que no se pueden verificar.

</details>

---

## P4 — `resolve_native_op` devuelve una tupla posicional — ✅ hecho (`5089959`)

Ahora devuelve `varn_types::NativeOpTarget` con campos nombrados, más un
constructor `unknown()` para que el caso "op-id no está en la tabla" sea un
nombre y no un `(0, 0, empty)` que el lector tenga que descifrar. El tipo vive
en `varn-types` porque es lo que ambos crates ya comparten.

<details><summary>Diagnóstico original</summary>

```rust
pub resolve_native_op: fn(u64) -> (usize, usize, varn_types::SignatureDescriptor),
```

Los dos `usize` son `func_ptr` y `raw_func_ptr`. **Intercambiarlos compila** y
produce un salto a la dirección equivocada desde código generado.

**Cómo:** un struct con nombre en `varn-types` (es lo que ambos crates
comparten):

```rust
pub struct NativeOpTarget {
    pub func_ptr: usize,
    pub raw_func_ptr: usize,
    pub signature: SignatureDescriptor,
}
```

Un único call site: `clif/alloc.rs`, en el lowering de `CallNativeOp`.

**Aceptación:** las cuatro validaciones. Vigilar `str_ops` y `json_native`, que
son los benchmarks que más pasan por `CallNativeOp`.

</details>

---

## P5 — Archivos sobre el umbral de 500

| Archivo | Líneas | Nota |
|---|---|---|
| `exec/dispatch/mod.rs` | 772 | **deliberado**, ver abajo |
| `nursery.rs` | 647 | sin auditar |
| `exec/ctx.rs` | 505 | sin auditar |

`nursery.rs` y `exec/ctx.rs` no se tocaron en F0–F6 y no se han mirado por
dominio. Antes de partirlos hay que comprobar si tienen dominios separados o si
son cohesivos y su tamaño está justificado — partir por número de líneas es
exactamente lo que este plan evita.

---

## P6 — Función comentada en `reg_ops/misc_ops.rs` — ✅ hecho (`e15f56f`)

Eran 24 de 203 líneas: un `exec_build_object_with_shape` desactivado a base de
`//`. Código muerto conservado por si acaso; git ya lo conservaba. Borrado.

---

## P7 — Puntos de aborto en el runtime — 🟡 parcial (`e13f325`, `1e57454`)

**Hecho.** Los dos archivos que la auditoría señalaba, más un hallazgo que
apareció al mirarlos:

- `exec/jit_helpers/modules.rs` — `jit_load_module` y `jit_load_module_by_idx`
  hacían `panic!` cuando `load_module_from_source` devolvía `Err`, mientras el
  intérprete propaga con `?`. Divergencia entre tiers: el mismo `import` era
  error capturable o aborto del proceso según si el frame estaba compilado.
  Ahora ambos salen por `jit_propagate_error`.
  **No es un bug demostrado**: no logré construir un reproductor. `import` es
  sólo de top level ("hir: nested declaration kind" lo rechaza dentro de una
  función) y el top level del módulo no se ofrece al JIT. Cuenta como poner de
  acuerdo a los tiers, no como cerrar un crash conocido.
- `exec/dispatch/ops_math_cmp.rs` — los 8 sitios son el mismo patrón: un
  `match op` interno sobre opcodes que el brazo externo ya estrechó, con un `_`
  muerto. **Inalcanzables por construcción**, así que siguen siendo
  `unreachable!` — pero cada uno nombra ahora su invariante, para que una
  edición equivocada dé un mensaje diagnosticable y no un panic pelado.
- `exec::arith::{add,sub,mul}` devolvían `VmResult` sin contener un solo
  `Err(`, y `jit_add`/`jit_sub`/`jit_mul` hacían `.unwrap()`. Mismo defecto que
  `GcError`, pero con `.unwrap()` como manejo del caso imposible — se
  convertiría en aborto del host el día que alguien añada un fallo a `add`.
  Arreglado en el origen: las tres devuelven `VmValue`.

**Queda: 99 sitios en 35 archivos**, y la muestra que revisé dice que el grueso
es categoría 1 y 2, no deuda:

- `heap/jit.rs` (9) — sondas de layout en el arranque (`"array payload probe
  failed"`). Abortar ahí es **deliberado y correcto**: un cambio de layout en
  std debe fallar ruidosamente al arrancar en vez de corromper memoria en
  caliente.
- `exec/dispatch/reg_ops/calls.rs` (6) — `frames.last_mut().unwrap()` dentro de
  un `if self.frames.len() > frame_idx + 1`. Seguro por construcción.
- `exec/jit_helpers/classes.rs` (6) — `expect("non-string const")` y
  `panic!("Unknown class member op kind")`: bytecode roto, no entrada de
  usuario. Categoría 2.

**No he clasificado los 99 uno a uno.** Lo anterior es una muestra, no un censo.
Antes de tocar más, clasificar el resto; sólo la categoría "alcanzable desde
código de usuario" es deuda real, y en lo revisado hasta ahora esa categoría
tenía exactamente un miembro (el `import`).

<details><summary>Diagnóstico original</summary>

| | Cuenta |
|---|---|
| `panic!` | 23 |
| `.unwrap()` | 33 |
| `.expect()` | 33 |
| `unreachable!()` | 12 |

Concentración: `exec/dispatch/ops_math_cmp.rs` (8), `exec/jit_helpers/modules.rs`
(5), `exec/dispatch/mod.rs` (3), `exec/jit_helpers/suspend.rs` (3).

**Por qué importa:** en un lenguaje, un `panic!` mata el proceso anfitrión en
vez de levantar un error capturable desde Varn. Un programa de usuario no
debería poder tumbar el runtime.

**Cómo:** no es una tarea única. Clasificar cada sitio en:

- **inalcanzable por construcción** — dejar, pero como `unreachable!` con el
  invariante escrito, no como `unwrap`;
- **error del compilador/bytecode** — `panic!` es correcto: es un bug nuestro,
  no del programa del usuario;
- **alcanzable desde código de usuario** — convertir a `RuntimeError`.

Sólo la tercera categoría es deuda real. Empezar por `ops_math_cmp.rs`, que es
aritmética y por tanto lo más expuesto a entrada del usuario.

**Riesgo:** bajo por sitio, alto en volumen. Hacerlo por archivo, no de golpe.

</details>

---

## P8 — Dependencias declaradas y nunca nombradas — ✅ hecho (`e15f56f`)

Quitadas cuatro: `varn-jit`→`varn-base`, `varn-backend`→`rustc-hash`,
`varn-parser`→`varn-lexer` y `varn-base`. (`varn-vm`→`parking_lot` ya había
caído en `921a56b`.)

La de `varn-parser`→`varn-lexer` se comprobó en vez de asumirse: el parser toma
`Token` y `TokenKind` de `varn_core`, no del crate del lexer, así que la
dependencia sobraba de verdad. Confirmado quitando y compilando, no sólo por
búsqueda.

---

## P9 — Indentación en `exec/host/isolates.rs` — ✅ resuelto

`spawn_isolate` y `to_sendable` conservaban 4 espacios de más tras moverse desde
un `impl`. Resuelto en `bda5167` (`fmt(all): format all project`), que además
cambia una premisa de este plan: **el repo ahora sí está formateado con
rustfmt**. Antes no lo estaba, y por eso no se aplicó en su momento.

**Consecuencia para lo que queda:** pasar `cargo fmt` antes de cada commit. Con
el repo ya limpio, un cambio sin formatear introduce ruido de formato en el
diff y vuelve a romper la invariante.

---

## Orden recomendado

~~P1~~ · ~~P2~~ · ~~P3~~ · ~~P4~~ · ~~P6~~ · ~~P8~~ · ~~P9~~ — hechos.

Queda:

1. **P7**, resto — clasificar los 99 sitios restantes por archivo antes de
   tocar ninguno. La muestra revisada sugiere que casi todo es categoría 1 y 2;
   si eso se confirma, P7 se cierra documentando en vez de cambiando.
2. **P5** — `nursery.rs` (647) y `exec/ctx.rs` (505), sólo si el análisis por
   dominio lo justifica. `exec/dispatch/mod.rs` no entra: ver el final.

Con el repo ya formateado (`bda5167`), pasar `cargo fmt` antes de cada commit.

---

## Lo que deliberadamente NO se hace

**`exec/dispatch/mod.rs` se queda en 772 líneas.** Lo que resta es el bucle de
dispatch por opcode (~550 líneas) y el único argumento para partirlo es su
tamaño. Eso no es razón para reestructurar el bucle caliente del intérprete.

El bloque que sí salió en F6 (`jit_frame.rs`) salió porque era **otro trabajo**
—entrar a un frame compilado y reconciliar sus cuatro finales— que corría una
vez por entrada de frame, no una vez por opcode. Ese es el criterio: se parte
por responsabilidad, y sólo cuando la medición pareada dice que no cuesta.

Cualquier propuesta futura de partir el bucle de opcodes tiene que traer una
medición delante, no un conteo de líneas.
