# Plan async fase B — JIT para generadores y tasks (diseño, sin código)

Estado medido: `task_soak` 346ms + `task_spawn` 114ms 100% intérprete.
Bails: `lower.rs:142-144`, `drive.rs:43-51`, `tiering.rs:144-147`
(`is_generator||is_async`). Opcodes sí bajan (`extra/suspend.rs`: `await_/yield_/spawn`
con `spill(live)` + helper); lo que falta es ENTRAR al código al reanudar.

## Por qué es difícil (descartado con evidencia)

- Reanudar NO es volver a entrar por arriba: el frame vive, el `ip` avanzó,
  los valores están en homes. Requiere entrada a mitad de función.
- Corrutinas simétricas (`setjmp` de vuelta) son UB: el driver corre código
  arbitrario sobre el mismo stack nativo entre suspender y reanudar.
- `&mut ExecCtx` en entradas del trampolín sería UB (stacked borrows): el JIT
  reentra por el mismo puntero. Por eso el trampolín usa punteros crudos con
  bloques explícitos — no tocar (auditoría 2026-10-08).
- Conclusión: la única reanudación sana es entrada-OSR (releer homes),
  nunca preservar el stack nativo.

## Lo que YA existe (no construir)

- Suspensión: `spill(live)` + `resume_ip` + helper (`extra/suspend.rs:10-66`);
  `SuspendLive{resume_ip,regs}` por punto (`chunk/proto/records.rs:2`).
- Homes como verdad: `osr::emit_entry` recarga vivas+params desde homes
  (`osr.rs:34-52`); ABI OSR ya es `nparams=0`.
- Destino: el driver escribe el input en `dest_reg` antes de `run_until`
  (`generator.rs:64-67`); el código reanudado lo lee normal.
- Generadores conservan todo sin copiar (`ExecCtx` propio vivo); try/catch
  con `CatchParam` sigue baileando para TODO el mundo (sin regla nueva).
- Congelado/descongelado de tasks lee stack + poda por `SuspendLive`
  (`scheduler/suspend.rs:105-284`); con spill previo vale para frames JIT.

## B0 — batería base (sin código producto)

- Microbench generadores (`tests/benchmarks/vn/bench_generator.vn`):
  ping-pong, map/filter sobre generador, loop-carry a través de resume,
  anidados, capturas por upvalue, try FUERA del yield.
- Baseline intérprete (quick): pingpong 100k=31ms, mapfilter 100k=44ms.
  Checksums idénticos JIT/NO_JIT (bit a bit verificado).
- [x] B0 hecho (batería + baseline, sin tocar producto).

## B1 — generadores (sin scheduler, sin freeze)

1. Quitar `is_generator` de los 3 gates (mantener `is_async`).
2. Pre-paso: partir bloques SSA en cada `resume_ip` de `SuspendLive`
   (vivas del punto como params; verifica `check_block_args`).
3. Entradas de reanudación: generalizar OSR (`loop_header_at` → punto
   de resume + vivas de `SuspendLive`); caché multi-slot por `(ip,entry)`;
   compilar bajo demanda al reanudar.
4. Driver: al reanudar (`started`), primar `osr_request(ip)` si hay entrada;
   respetar epoch (código invalidado → fallback a intérprete, nunca abortar).
5. Contadores/registros para entradas de resume (tiering/stats).
- Validación: batería B0 idéntica JIT/NO_JIT + pared + suite 56 (tier parity)
  + matriz 4 cuadrantes. Test crítico: loop-carry + resume combinados,
  upvalues sin cerrar (igual que intérprete).
- Riesgos: explosión de compiles (acotado: pocos puntos/fn); spill incompleto
  = corrupción silenciosa (exige metadata de registros exacta).
- [ ]

## B2 — tasks async (scheduler + freeze)

- Todo B1 + `is_async` fuera de los gates.
- Freeze/thaw sobre homes con spill previo (sin cambios si el spill es completo);
  reanudación por la misma entrada-OSR; scheduler intacto (llama `run_until`).
- AOT sigue baileando async/generadores (empaquetado offline, fuera de B2).
- Validación: `task_soak`/`task_spawn` números + matriz + pruebas 21,75,77,154.
- [ ]

## B3 — endurecimiento (fuera de B2)

- `CatchParam` a través de resume (tablas de unwind para código máquina).
- Fuzz de intercalados suspender/reanudar × GC majors en el medio.
- AOT para async/generadores.
- [ ]

## Fuera de alcance (bailan como hoy)

- `CatchParam` alcanzable (todas las funciones, no solo async).
- AOT (`unreachable!()` en `aot/mod.rs:319-324,500-505`).
- Cambio de modelo de tasks (sin work-stealing, FIFO por diseño).
