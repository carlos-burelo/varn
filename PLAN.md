# PLAN — Cranelift: cerrar cobertura y recuperar rendimiento

Estado a 2026-07-30. Escrito para arrancarse en una sesión limpia sin volver a
derivar nada.

---

## 0.0 Corrección de rumbo (2026-07-30, tarde)

**Este plan medía la métrica equivocada.** Las Tareas 1-4 son todas cobertura
clif y coste de compilación sobre `tests/main.vn`, que es una **suite de
correctitud**: compila 31 funciones para correr cada una ~3 veces, y el 77% de
su tiempo es compilar. Un programa real compila una vez y corre largo, así que
bajar ese coste no se nota fuera de la suite. Los huecos reales frente a Bun
(JSC) estaban en la **llamada**, en la **inferencia de tipos que no llegaba a
los globals**, y en **alloc/GC** — ninguno tocado por las Tareas 1-4.

### Lo que se midió

Coste por operación, dentro de funciones clif, antes de los arreglos:

| operación | ns | veredicto |
|---|---|---|
| iter de loop int (`sum + k*3`) | 1.76 | óptimo |
| iter de loop float | 1.27 | óptimo |
| `{a: i}` que no escapa | 1.48 | scalar replacement funciona |
| lectura `int[]` | +2.0 c/u | bounds + guard de repr |
| `arr.push(i)` | 15.7 | |
| **llamada a otra función** | **43-53** | vs 1.5 inlineado |
| `new P1(i)` | 83.3 | = llamada + alloc |
| `new P5(...)` que escapa | 161-167 | +70.6 de promoción GC |

Aritmética escalar y escape analysis ya estaban a nivel V8. El agujero era la
llamada.

### Lo que aterrizó

1. **Llamada directa clif→clif, ligada tarde** — `43.4 → 2.76 ns` (15.7x).
   El fast path ya existía pero era **código muerto**: `calls_allowed` exigía
   `!has_alloc`, y `OpCode::Call` está EN la lista de `has_alloc`, así que
   nunca era cierto en una función que contuviera una llamada. Además
   `static_target` exigía el callee ya compilado, y los callers compilan antes
   que sus callees por definición. Ahora el sitio de llamada carga la entrada
   desde `FunctionProto::clif_raw` en tiempo de ejecución (0 = todavía no), y
   el ABI acepta parámetros/retornos de cualquier `SlotKind`, no sólo `Int`.
   `tests/main.vn` **correr** 10.44 → 4.89 ms.

2. **BUG DE CORRECTITUD: llamadas con ≥4 parámetros corrompían argumentos.**
   `emit_generic_call` pasaba 4 `VmValue` planos a `clif_call_fallback` pero
   declaraba el `argc` real por separado, y ese conteo incluye el slot r0. Con
   4 params reales se stageaban 4 valores para un `arg_count` de 5 y
   `prepare_call` leía el frame un slot abajo. `f4(1,2,3,4)` daba **123** en
   vez de 1234; `f6(...)` daba **230123** en vez de 123456. `VARN_NO_JIT=1`
   era correcto, así que sólo un caller compilado lo exponía; la suite no lo
   cubría. Los argumentos ahora viajan por la ventana de registros flusheada,
   sin techo de aridad. Fijado por `tests/57-call-arity.vn`.

3. **Inferencia de tipo de elemento para arrays a nivel de módulo.** Existía
   (`binder::array_evolve`) pero excluía el scope Global por diseño. Lo que un
   scan de un solo archivo no puede justificar es un binding que **sale** del
   archivo, y eso es exactamente lo que `bind_export` escapa ahora (forma
   declaración, forma especificador y `export default <expr>`). `bench_matrix`
   sin anotar: **58.9 → 38.4 ms**.

4. **Cache de payload de array en funciones que alocan.** Estaba desactivada
   porque cualquier alocación puede mover el Vec del heap bajo el puntero
   cacheado. Ahora se habilita por región de loop cuyo cuerpo no aloca, y el
   safepoint de back-edge invalida las caches en su rama tomada. 1.17x en el
   micro que tiene esa forma exacta.

Resultado pareado (método robusto a térmica: los dos binarios adyacentes en
cada ronda, mediana de las razones): **matrix 1.65x**, resto plano, ninguna
regresión. `tests/main.vn` 805/805 en clif, `VARN_NO_JIT=1` y `VARN_NO_CLIF=1`;
`cargo test --workspace --release` 48 suites verdes.

### Lo que sigue abierto, por ROI medido

1. **Constructores.** `new X(...)` sigue costando ~83 ns porque el callee es un
   `Class`, no un `VmClosure`, así que nunca toca el linker: va por
   `clif_call_fallback` → `call_vm_window` → `construct_staged_fast`, que
   empuja un `CallFrame` de verdad (143 333 frame pushes en `bench_dto`). El
   arreglo real es inlinear el constructor en el sitio de llamada cuando su
   cuerpo son sólo `SetFixedField` desde parámetros.
2. **Promoción del GC: 70.6 ns por objeto promovido.** Medido aislando
   `freshEscape` (166.6) contra `freshShortLived` (96.0) a igual tasa de
   alocación. **Subir la nursery NO sirve**: de 16384 a 131072 bajó los minor
   GC de 20 a 3 y empeoró `bench_dto` un 18.6% y `bench_gc_alloc` un 11.8%
   (locality). El coste es por objeto evacuado, no por colección. Esto pide el
   trabajo de representación desboxeada, no un ajuste de política.
3. Las Tareas 1-4 de abajo — reales, pero por debajo de estas dos. Ojo: la
   Tarea 4 **no arregla `bench_dto`**. Comprobado: envolver el benchmark entero
   en una función (100% clif, 0 frames intérprete) da 81.5 ms contra 76.8 ms
   del original top-level. `bench_dto` es alloc-bound, no dispatch-bound.

### Método de medición (corrección)

Tomar el min de N por binario está **sesgado**: la máquina calienta de forma
monótona, así que el binario que ocupe el primer slot del barrido se lleva el
min. Un mismo binario midió `bench_fib` a 36.4 ms en la ronda 1 y a 74.0 ms en
la ronda 3. Medir los dos binarios **adyacentes** en cada ronda, tomar la razón
por ronda y reportar la mediana; alternar el orden entre rondas.

---

## 0. Dónde estamos

### Migración de opcodes: terminada

Los 134 variantes de `OpCode` tienen arm de lowering en `crates/varn-jit/src/clif/`.
El template JIT **ya no existe**: `varn_jit::compile` lowerea con Cranelift o
devuelve `Err`, y el bail deja la función al intérprete. No hay tier intermedio.

Cobertura real sobre `tests/main.vn` (`VARN_CLIF_TRACE=1`):

| | |
|---|---|
| `CLIF ROUTE` | 450 |
| `CLIF BAIL` (rechazo del lowering) | **0** |
| `CLIF GATE` (`code.len() > 250`) | **47** |

Ningún opcode rechaza. Lo que queda fuera son **funciones enteras** que la
puerta de tamaño descarta antes de llamar a clif — todos los `<module>`
top-level entre ellas. La traza `CLIF GATE` existe precisamente porque ese
rechazo no aparecía ni en `CLIF BAIL` ni en `compile_fail`, y sin ella el
"0 bails" sobreestima la cobertura.

### Rendimiento

`tests/main.vn` no es execution-bound, es **compile-bound**. Con
`VARN_NO_JIT=1` el árbol actual y el baseline `505c004` miden lo mismo
(15.2 vs 15.9 ms): todo el delta con JIT activo es tiempo de compilación.

Cranelift compila ~**65x** más lento que el template borrado — sobre las mismas
144 funciones de `tests/47-isolates-multithread.vn`: **2.48 ms** (template) vs
**91.5 ms** (clif), ~19 µs vs ~1.24 ms por función.

| `tests/main.vn` execute min | |
|---|---|
| baseline `505c004` (07-25) | 23.9 ms |
| antes del tiering | 87.3 ms |
| **hoy** | **38.3 ms** (suite completa: 42.1 ms min / 45.6 ms p50) |

> Actualizado tras la sección 0.0: el reparto es `compilar 34.3 ms (77%) +
> correr 10.4 ms (23%)`, y la parte de **correr** bajó a **4.89 ms** con la
> llamada directa. El coste que persiguen las Tareas 2-3 es el 77% de una
> suite de tests, y lo empequeñece la `precompilación de módulos`: **141.7 ms**
> de arranque en frío, fuera del p50.

### Estado del árbol

**Sin commitear**: ~27 archivos, +1177/−547. Incluye la migración de 11 opcodes
a clif de la sesión anterior, cuatro reparaciones de correctitud, el tiering
perezoso, y la limpieza de scaffolding de debug en `tests/28|42|53`.

`HEAD` (`d67e92b`) **no puede correr la suite** — panica en el test 7
(`ctx_jit_values.rs`, index 512 en len 1). El último commit sano para
comparaciones es **`505c004`** (07-25), que todavía tenía el template JIT.

Validación actual: 775/775 con clif, con `VARN_NO_JIT=1` y con
`VARN_NO_CLIF=1`; `cargo test --workspace --release` 48 suites sin fallos.

**Primer paso de cualquier sesión nueva: commitear esto.** Es mucho trabajo
verde sin proteger, y `HEAD` está roto.

---

## Tarea 1 — Bug de paridad de tiers (BLOQUEANTE)

> **Estado 2026-07-30: mitad resuelta.** La causa del fallo de `Duration hours`
> era que **la stdlib nunca se type-checkeaba**: `stdlib_loader::compile_source`
> corría el checker y tiraba sus diagnósticos, así que `std/time.vn` podía
> declarar `int_div(...): int` devolviendo un float entero. Ahora el bundle se
> valida al construirse (`compile_source_checked`), existe `float.toInt(): int`
> (única conversión real: `as int` no baja a nada), y los 7 errores de tipos que
> el gate destapó están arreglados — junto con tres bugs del checker que
> afectaban también a código de usuario. Commit `a09e24a`.
>
> **Queda un SEGUNDO bug de paridad, distinto, en closures/upvalues**, que sigue
> bloqueando cualquier umbral > 1:
>
> ```text
> tests/scratch.vn:
>     import "./33-globals-async-coherence.vn"
>     import "./37-complex-closures.vn"
> vn bench tests/scratch.vn --runs 1   →  ASSERT FAIL: sm after start
> ```
>
> `vn run` pasa. `vn bench` falla porque ejecuta el programa dos veces (warmup +
> medidas): sólo entonces `makeStateMachine` llega a su segunda entrada de frame
> y se compila, y el `current` capturado vuelve a leerse con su valor previo al
> `transition`. Reproduce con umbral 2 y con 4; necesita el módulo async 33
> delante del 37; con 37 solo no reproduce. Es vida útil de stack/upvalue, no
> aritmética. Empezar por comparar `emit_make_closure`/`emit_close_upvalue`
> (`clif/alloc.rs`) contra `capture_upvalue`/`close_upvalues_above`
> (`exec/ctx_frames.rs`) y por qué la suspensión del módulo 33 cambia el layout.
>
> **`vn bench` es gate obligatorio** (CLAUDE.md), no sólo `vn run`: este fallo no
> aparece en `run`.

Lo que sigue es el análisis original, ya resuelto, que se conserva por contexto.

### Repro exacto

En `crates/varn-vm/src/frame.rs:306`:

```rust
const JIT_TIER_THRESHOLD: u32 = 1;   // subir a 2
```

Con `2`:

```
cargo run --release --bin vn -- run ./tests/main.vn
  → ASSERT FAIL: Duration hours
    at <module> (tests/31-stdlib-migration-test.vn)
```

`VARN_NO_JIT=1` pasa. Con umbral `1` pasa. O sea: falla sólo cuando **unas
llamadas se interpretan y otras corren compiladas**. La compilación ansiosa
lo ocultaba porque todo corría JIT desde la primera llamada.

### Hipótesis principal (verificar antes de tocar nada)

`tests/31-stdlib-migration-test.vn:10` es `assert("Duration hours", d.hours === 1)`.
La línea 9, `d.totalMilliseconds`, **pasa**. Mirando `std/time.vn:187-195`:

```varn
this.totalMilliseconds = ms;                    // pasa
this.hours = int_div(total_s, 3600);            // falla
```

El campo que falla es exactamente el que viene de `int_div`. `int_div` devuelve
un **float entero** en un sink declarado `int`; el contrato es que la VM coerce
en el sink, no en la definición — está documentado en
`crates/varn-jit/src/clif/kinds.rs` (arm `OpCode::Call`, ~línea 109):

> A call result is ALWAYS boxed bits, even into an `int`-typed slot […] stdlib
> code relies on the VM coercing a whole float (`int_div`) at the int sink.

Sospecha: el store del campo (`SetFixedField` / `SetProperty`) coerce distinto
en el intérprete que en clif, así que una instancia `Duration` construida por el
constructor interpretado y otra construida por el constructor compilado guardan
representaciones distintas para `hours`, y `=== 1` falla en una de las dos.

Ver [[varn-int-semantics-i48]]: las reglas viven SÓLO en `varn-core/numeric.rs`
y deben ser tier-idénticas.

### Cómo atacarlo

1. Reducir a un `.vn` mínimo: una clase con `this.x = int_div(a, b)`, construirla
   dos veces, comparar `x` entre la instancia 1 (interpretada) y la 2
   (compilada). Con umbral 2 eso es exactamente lo que ocurre.
2. Imprimir los bits crudos (`heap.str_repr` no basta — hace falta el `VmValue.0`)
   del campo en ambas instancias.
3. Comparar el camino de store: intérprete en `crates/varn-vm/src/exec/props.rs`
   (`set_fixed_field`) contra el emitido por `crates/varn-jit/src/clif/fields.rs`.
4. Arreglar en el punto único de verdad; no parchear un tier.

### Criterio de aceptación

- Umbral 2: `tests/main.vn` 775/775 con clif, con `VARN_NO_JIT=1` y con
  `VARN_NO_CLIF=1`.
- Un test nuevo que fije la paridad: misma clase construida antes y después del
  umbral, campos idénticos.

---

## Tarea 2 — Subir el umbral de tiering y medir

Sólo después de la Tarea 1. **Sigue bloqueada**: el umbral está en `1` porque
2 y 4 rompen `vn bench` (ver el recuadro de la Tarea 1). Medir cualquier umbral
antes de arreglar ese bug es medir un árbol incorrecto.

`crates/varn-vm/src/frame.rs:306`. Hoy vale `1`, que es el tiering más débil
posible: salta únicamente los protos que se **construyen y nunca se entran**
(en el test de isolates, 144 funciones compiladas para ejecutar 38 frames JIT).
Todo lo que se ejecuta sigue compilándose antes de su primera llamada.

El ahorro real es dejar que las primeras N llamadas interpreten. Barrer
`2, 4, 8, 16` y quedarse con el mejor, midiendo:

- `tests/main.vn` (execute min)
- `tests/47-isolates-multithread.vn` — el más sensible, 60.8 → 27.9 ms con el
  tiering actual, baseline 10.9 ms
- los benchmarks de `benchmarks/` (fib, matrix, …) — un umbral alto castiga
  funciones calientes de arranque corto; vigilar que no regresen

Contexto de la implementación actual:

- `FunctionProto.jit_entry_count` (`crates/varn-types/src/chunk.rs:551`) cuenta
  entradas de frame mientras el proto siga sin compilar.
- `VmClosure::hot_jit_fn` (`crates/varn-vm/src/frame.rs:320`) cuenta y compila.
- Punto de tiering: `crates/varn-vm/src/exec/dispatch/mod.rs`, entrada de frame
  en `run_until_inner_raw`.
- La entrada compilada vive **sólo** en `FunctionProto.jit_entry`. `VmClosure` ya
  no tiene campos `jit_entry`/`jit_code` (muchas closures comparten proto y el
  tiering ocurre después de construirlas). Leer con `closure.jit_fn()`.

---

## Tarea 3 — Coste de compilación de Cranelift

~1.24 ms por función es el número a bajar; el tiering sólo evita pagarlo, no lo
reduce. Palancas, de menor a mayor riesgo:

1. **`opt_level`**: `crates/varn-jit/src/clif/mod.rs:52` fija `"speed"`. Probar
   `"none"` para el primer tier y reservar `"speed"` para un segundo escalón.
   Medir compile time **y** execute; es el clásico trade de tiering.
2. **Reutilizar `Context`**: `compile_piece` (`clif/lower.rs`) crea un
   `cranelift_codegen::Context` por función. Cranelift documenta reusar el
   Context entre compilaciones para amortizar allocs. Es el cambio más barato.
3. **Compartir código entre isolates**: hoy cada isolate recompila sus protos.
   Un `FunctionProto` compilado podría publicar su `JitBuffer` a los demás si la
   linkage estática lo permite (ojo: `ClifLinker` liga contra los globals vivos
   del ctx, ver `clif_link::CtxLinker`).

Medir siempre con el método de la sección "Cómo medir".

---

## Tarea 4 — Tope de 250 words: frames clif reanudables

`crates/varn-jit/src/lib.rs:357`. **No es un presupuesto de compilación** — es
lo único que mantiene los `<module>` top-level fuera de clif, y con ellos las
únicas formas que ponen un frame clif debajo de otro.

Por qué no se puede subir el número y ya:

- Un frame clif no es reanudable: no tiene `ip` de bytecode propio, así que la
  VM sólo sabe reentrarlo desde `ip == 0`.
- `execute_jit_frame` (`crates/varn-vm/src/exec/dispatch/mod.rs:54`) instala
  `setjmp` **sólo para el frame clif más externo** (`is_outer`, línea 60).
- Un `throw` en una llamada clif anidada hace longjmp hasta ese buffer externo y
  destruye la pila nativa de todos los frames clif intermedios, cuyos
  `CallFrame` siguen vivos con `ip == 0` → el frame loop los reentra por JIT
  desde arriba → reejecución infinita.

Comprobado: con el tope en 8192,
`assert("try catch returns", safeDivide(10, 0) === -1)` de
`tests/11-errors.vn` cuelga y muere en
`memory allocation of 51539607552 bytes failed`. Prefijo mínimo que falla:
`head -11 tests/11-errors.vn`.

La suspensión (`Await`, `Yield`) tiene el mismo hueco sin el bucle.

### Diseño requerido

1. **Buffer de salto por frame.** `execute_jit_frame` guarda el
   `ctx.jit_jmp_buf` anterior, instala el suyo, y lo restaura al salir. En la
   rama de error (`code == 1`, línea 132), si el handler NO pertenece a este
   frame (`handler.frame_depth - 1 < frame_idx`), re-lanzar con
   `my_longjmp(saved, 1)` para que el padre desenrolle su propio frame nativo.
   Cuando `saved` es null estamos en el más externo: devolver `Err` como hoy.
2. **La suspensión necesita el buffer MÁS EXTERNO**, no el innermost: un frame
   clif intermedio no puede parkearse a mitad de función. Hace falta un
   `ctx.jit_suspend_buf` separado que `jit_await` / `jit_yield` usen, o un ip de
   side-exit por punto de suspensión.
3. Precedente ya resuelto para un opcode concreto: `jit_load_module`
   (`crates/varn-vm/src/exec/ctx_jit_runtime.rs`) rebobina el ip a la propia
   instrucción `LoadModule` y parkea **su** frame — capturado ANTES de la carga,
   porque el módulo suspendido deja su frame encima. Ahí funciona porque el
   destino de reanudación es conocido. Ver también el `if let Some(resume_ip)`
   tolerante en el handler `code == 2` (`dispatch/mod.rs:167`).

### Criterio de aceptación

- Tope subido a ≥8192, `tests/main.vn` 775/775 en los tres modos.
- `CLIF GATE` a 0 sobre la suite.
- Las 47 funciones gated hoy incluyen `runComprehensiveTest` (2004 words),
  `testIsolates` (720), `runAsync` (652), `parse` (462), `dirname` (434),
  `normalize` (405).

---

## Cómo medir (obligatorio)

La máquina termaliza duro: en esta sesión el **mismo binario** pasó de 23.9 a
35 ms. Un A/B suelto miente.

- Dos binarios, órdenes alternados, **min de 6 rondas**, nada más corriendo.
  Un build en paralelo distorsiona 6x (medí 628 ms donde eran 92).
- Baseline de comparación: worktree en `505c004`
  (`git worktree add <tmp> 505c004 --detach`), `cargo build --release --bin vn`.
  Ese árbol necesita `--release`: con `--profile quick` no linkea
  (`.varn_ops$A` undefined).
- La suite de `505c004` corre **713** asserts y la de hoy **775**; para comparar
  trabajo igual, excluir `tests/42` de ambos `main.vn`.
- Usar `execute` **min**, no p50 ni e2e.
- `bench -v` da lo que de verdad importa aquí: `freshly compiled`,
  `total compile time`, `JIT runs`, `calls slow/prepare`, `frame pushes`.
  Fue esa tabla — no el timing — la que localizó la regresión.

---

## Trampas conocidas (cuestan horas si se redescubren)

- **`FunctionProto.arity = 1 + nparams`**: incluye el slot r0 del callee, que en
  un método es el receptor. `clif::lower` debe usar
  `proto.arity.saturating_sub(1)` (tres sitios). Tratarlo como el número de
  params hace que `param_kinds.len() != nparams` sea siempre cierto → **bail en
  todas las funciones, backend clif muerto sin un solo error visible**.
- El caller stagea r0 como placeholder `null`; un `BoundMethod` **rellena** ese
  slot, no inserta delante. `expected = arity + has_this` corre los params un
  registro (`Box.map(f)` → "value is not callable: null").
- El fast path Vm de `BoundMethod` en `try_prepare_call_fast` rompe `super`
  (`tests/48-opt-unsupported-phase1.vn`, "super write via method"). Sólo el
  target `Native` va por fast path.
- `.vnc` cachea por hash de fuente: purgar antes de validar cambios del
  compilador.
- No usar `--profile quick` para nada que toque `varn-builtins`: no linkea.

---

## Orden sugerido

1. Commitear el árbol verde actual.
2. Tarea 1 (paridad de tiers) — desbloquea la 2.
3. Tarea 2 (barrido de umbral) — el retorno de perf más barato.
4. Tarea 3 (coste de compilación) — empezar por reusar el `Context`.
5. Tarea 4 (frames reanudables) — la más grande; cierra la cobertura de verdad.
