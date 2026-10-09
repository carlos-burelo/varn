# Plan código máquina JIT (microbenchs 2026-10-08)

Base: batería 23 benches `tests/benchmarks/vn`, release, `--runs 3`.
Evidencia cruda: microbench_run.txt, typeloss_run.txt, prof_{str,json,http}.txt
(en temp opencode). Checksums vn==bun en str_ops (12/12).

Estado medido: routing clif 100% salvo async; typeloss caliente ≈0;
fib35 vn 63ms vs bun 368ms vs node 502ms (int+call ya gana 6-8×).
Pérdidas vn vs bun: concat_parts 2.2×, char_code 3.1×, slice 1.75×,
int_to_str 1.5×, join/search 1.5×.

## O1 — bump-alloc inline en nursery (mayor peso)

- Por qué: `emit_build_array/map/object` (`varn-jit/src/clif/from_ssa/heap.rs:129-221`)
  = spill a ventana + `call_indirect` + reload de `exec_ctx` por alloc.
  1.1M allocs str_ops, 1.45M http_routing, 2.2M json_pure por corrida.
- Qué: cursor/límite nursery inline en el lowering; slow path → helper actual.
- Verifica: bench pareado http/json/str antes/después + `tests/main.vn` 4 cuadrantes.
- [ ]

## O2 — recortar tax por llamada en callees alloc-free

- Por qué: http 100k `vm-fast`, `Move` 24% opcodes, 101k pushes.
  `run_entered` (`varn-jit/src/clif/from_ssa/call/direct.rs:47-70`) publica
  roots + indirect + `publish` + `jit_call_leave` siempre.
- Qué: callees probadas alloc-free omiten publish/leave.
- Verifica: http_routing pareado + tier-identidad `VARN_NO_JIT=1`.
- [ ]

## O3 — intrínsecos str inline (charCode/toString/substring/slice)

- Por qué: cada uno es `call_helper_void` + reload (`heap.rs:79-88`,
  `varn-builtins/src/modules/core/types/str/str.rs:107-136` alloc intermedio
  `String` siempre). char_code 3.1×, toString 46ns×611k, substring 142ns×200k.
- Qué: charCode = load con offset; int pequeño → decimal inline;
  substring/slice con vista existente sin `String` temporal.
- Verifica: str_ops por sección vs bun (checksums deben seguir iguales).
- [ ]

## O4 — tip-append inline en concat con literal

- Por qué: benchConcatAppend = `call_indirect` + reload + safepoint por iter
  (`debug -p clif --fn benchConcatAppend`); concat_parts 2.2× con
  toString + 2 concats por iter.
- Qué: fast path cap-check + memcpy + bump len para acumulador en tip de Ext;
  fallback al helper (aliasing ya resuelto por diseño Ext).
- Verifica: concat_append/parts vs bun + test alias-before-append existente.
- [ ]

## O5 — Map literal/get especializado

- Por qué: http 500k `Map`, `BuildMap` 3.9% + `MapGetIndex` 3.5% opcodes,
  todo helper (`heap.rs:142`, `props/index.rs`).
- Qué: mapa pequeño inline o shape para claves estáticas.
- Verifica: http_routing pareado.
- [ ]

## O6 — comparaciones ordenadas Dyn con fast path

- Por qué: `boxed_compare` (`varn-jit/src/clif/generic.rs:33`) solo inlinea
  `Eq/Ne`; `Lt/Gt/Le/Ge` siempre helper. Typed ya es `cmp` nativo.
- Qué: extender el fast path bitwise a ordenadas cuando ambos tag==Int/Float.
- Verifica: microbench dedicado Dyn vs tipado + suite 4 cuadrantes.
- [ ]

## O7 — bails `Dynamic→Int` en block-params

- Por qué: `ByteWriter.constructor`, `PlainDateTime.constructor`,
  `WaitGroup.add`, `RateLimiter.take` caen enteras al intérprete.
- Qué: insertar conversión en from_ssa en vez de bailear.
- Verifica: `debug -p bails` vacío en esos módulos + suite.
- [ ]

## O8 — async/generadores fase B (gap de tier, no peephole)

- Por qué: task_soak 346ms + task_spawn 114ms 100% intérprete
  (`lower.rs:142-144`, `tiering.rs:144-147`).
- Qué: diseño aparte; no entra en este plan salvo O7 si aplica.
- [ ]

## No objetivos (ya rinden, no tocar)

` ssub/sadd_overflow` + inducción (`i+1` = `iadd` pelado, `scalar.rs:288`),
`Eq` inline, SSO `length` inline (`strings.rs:5`), array view hoist,
`GetFixedField` compacto nativo (`props/fields.rs:70`), `ModInt`/`DivInt`
nativos (`int_div.rs`). fib 6-8× sobre V8 lo prueba.

## Validación por item

`tests/main.vn` 4 cuadrantes + bench pareado mismo perfil (Ley 13:
iterar en `quick`, números vs bun/node solo en `release`) + checksum
idéntico al baseline. Sin (a) ganancia (b) medida (c) borrado, no se acepta.

## Cimientos — substring views (v1, 2026-10-08, verde)

- `HeapStr::View{base,start,len}` (`varn-vm/src/heap/str.rs`) + `alloc_str_view_of`
  (`heap/strings.rs`): vistas sin alloc, base `Arc` inmutable (sin aliasing),
  ≤5B→SSO, ascii YES se hereda.
- `jit_str_substring` (`exec/jit_helpers/strings.rs`) + brazo `CallNativeOp`
  (`varn-jit/.../call/native.rs`, `str_substring_op_id` en `varn-core/op_id.rs`).
  Fallback genérico intacto; bridge/contratos/`.vn` sin tocar (RPITIT `impl IntoVm`
  descartado: E0499 en edición 2021).
- Medida A/B misma sesión (release, 5 reps): slice 9.5-11.5ms vs control
  `substr` 12.9-16ms (~-27%), checksums 12/12 + NO_JIT idénticos.
  Lección: el helper escaneaba `is_ascii()` O(n) por llamada; usa flag cacheado.
- `verify.ps1 -Fast`: 4 cuadrantes 2287/0, clippy PASS, contratos PASS, fmt verde.
- v2 (pendiente): slice/substr/split por el mismo patrón.

## Cimientos — bridge str con vistas (edición 2024, `use<>`)

- Desbloqueo: `ret_ty(Str)` → `-> impl IntoVm + use<Self>` (`varn-contract/mapping.rs`).
  Los `-> String` existentes siguen valiendo; `use<>` vacío lo rechaza el compilador.
- `NativeCtx::str_sub` (default + overrides Heap/ExecCtx) + `substring/slice/substr/at/trim/split/lines/words`
  devuelven `VmValue` (vistas). `.vn`/contratos/tipos estáticos intactos.
- Medida NO_JIT misma sesión: slice 60→28ms, split 34→14ms (~2.2× intérprete).
  Sonda 30 casos idéntica JIT/NO_JIT/dinámico; suite 2287/0 ×2.

## Cimientos — nursery ×16 (196k→786k celdas)

- `YOUNG_THRESHOLD` 49152→786432 (`varn-vm/src/heap/young.rs`, única fuente).
- Tabla mismo perfil (quick): json minors 23→2, promociones 2.2M→300k, majors 7→3;
  http 30→1, 707k→536k; str 22→1, 200k→21. Pared −16/−32/−23%.
- Criterio de saturación: lo que aún promociona es live por diseño (grafo parseado,
  reqs retenidas); más nursery no separaría nada. Coste: pico joven mayor
  (bajo demanda, sin pre-alloc); pausas minor más largas.
