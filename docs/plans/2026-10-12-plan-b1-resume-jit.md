# Plan B1: reanudar async/generadores en JIT (2026-10-12)

Sesión limpia dedicada. Al terminar se regresa a
`2026-10-10-propagacion-de-tipos.md` (2.7 queda en primera fase).

## 0. Punto de partida (verificado en código, no copiado)

- Suspender ya funciona compilado: `spill` a home + `await_helper` /
  `yield_helper(dest_reg, resume_ip)` + `longjmp` al buffer exterior
  (`varn-vm/src/exec/jit_helpers/suspend.rs`, `jit_frame.rs:40-68`).
- Entrada a JIT solo con `ip == 0` (`dispatch/mod.rs:73`); reanudar
  (`ip != 0`) va siempre a intérprete. OSR existe (`osr_request` →
  `osr_jit_fn`) pero `tiering.rs:144` veta suspendibles.
- `SsaOp::Await{operand, resume_ip, live}` ya viaja en el SSA portable;
  `SsaLoopHeader{block, ip, live}` se construye de offsets emitidos
  (`portable/mod.rs:319`). No hay tabla `resume_ip`→bloque.
- `state_machine` parte en cada `Await`/`Yield` (bloques cont) y calcula
  `state_size`; `suspend.rs` analiza `resume_live` + handlers; el
  scheduler congela por `suspend_live` y reanuda montando intérprete
  (`scheduler/suspend.rs`, `scheduler/drive.rs:112-193`).
- Entrada nativa vetada en async/generadores (`lower.rs:22`): llamar
  siempre crea la tarea; la entrada enmarcada ejecuta tareas.
- Medido (`quick`, A/B pareado): async sin suspensión 1,9x, pingpong
  16%, B0 checksums idénticos, fase B 0, suite + matriz verdes.

## 1. Objetivo y criterio de salida

Re-entrar a código nativo en `resume_ip` con el estado desde home.
Sale verde si: B0 + suite 4 cuadrantes + soak de tareas + A/B con
ganancia en `pingpong` y en cadena `await` (hoy dominada por
maquinaria). Sin ganancia medida se aparca.

## 2. Fases (en orden, cada una con su commit verde)

### B1a. Tabla `resume_ip`→bloque en el SSA portable

- Registrar por cada sitio `Await`/`Yield`: (`next_ip`, bloque cont,
  valores vivos), espejo de `loop_headers` (`portable/mod.rs:319`).
- Cuestión abierta a cerrar aquí: el bloque cont sale de
  `split_at_suspend_point` SIN params; la entrada OSR los exige.
  Decidir entre (i) añadir params al partir + args en el salto, o
  (ii) materializar el estado sin params (solo homes + `live`).
  Lo que se elija vale para B1b sin cambios.
- Verifica: `cargo check` + dump `tir`/`ssa` de `bench_generator.vn`
  muestra la tabla; nada cambia en ejecución (puerta B1c cerrada).

### B1b. Entrada en `drive.rs` por bloque de reanudación

- Generalizar `loop_header_at(ip)` a `resume_at(ip)` (o unificar
  ambas tablas en una sola estructura: Ley 8, una sola tabla de
  entradas secundarias).
- Cargar params desde home como la entrada OSR; `resume_ip`
  arbitrario exige el mismo `Activation::Framed` (nada nativo).
- Verifica: generador de un solo `yield` re-entra (traza `osr_entries`
  sube en `pingpong`); checksums B0 intactos.

### B1c. El scheduler pide la entrada en vez de interpretar

- `thaw` + `Delivery` (incluida `Settled(Rejected)`: reanudar
  lanzando dentro de marco compilado) → `osr_request(resume_ip)`.
- Quitar el veto de `tiering.rs:144` solo para reanudación
  (la compilación de entrada sigue igual; sin entrada OSR de
  bucles en suspendibles hasta B1d).
- Verifica: test 154 (rechazo con locales intactos) + 77 + 20/21 en
  las tres combinaciones de niveles; soak 3000 tareas sin fugas.

### B1d. Endurecimiento (antiguo B3)

- `CatchParam` al reanudar, `try` alrededor de `await` compilado,
  `yield` delegado, GC con estado nativo vivo (soak + `154`),
  fuzz de reanudación × GC, AOT (sigue vetado: solo JIT).
- Verifica: matriz 4/4 + goldens si cambia algún dump + A/B final.

## 3. No tocar

- `dispatch/mod.rs:73` (`ip == 0`): la invariante que hace seguro
  todo lo anterior.
- `direct_entry` y entrada nativa en async: vetadas (test 154 las
  cubre: `batch[200]` era el `acc`).
- Protocolo `Freeze`/`thaw`/`suspend_live`: se reutiliza, no se
  rediseña (eso es B2, otra sesión).
- Formato de `SsaProto` serializado: añadir tabla rompe caché;
  cae dentro de `BUILD_FINGERPRINT` (verificar que invalida).

## 4. Mapa de archivos

- `varn-compiler/src/passes/state_machine/*`, `ssa/suspend.rs`
  (análisis y tablas).
- `varn-compiler/src/ssa/portable/mod.rs` (construcción),
  `varn-types/src/ssa/mod.rs` (tablas).
- `varn-jit/src/clif/from_ssa/drive.rs` (entrada),
  `varn-vm/src/jit/tiering.rs` (veto OSR),
  `varn-vm/src/exec/dispatch/mod.rs` (`osr_request`),
  `varn-vm/src/exec/scheduler/drive.rs` (`thaw` + `Delivery`).
- Oráculo: `bench_generator.vn`, tests 20/21/54/76/77/154/157,
  soak de `ADR-0019`.
