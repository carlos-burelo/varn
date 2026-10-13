# Plan unificado de rendimiento JIT (living, 2026-10-09)
> Histórico. No spec vigente. Código manda. Revalidar antes de usar.
> Validación: `.\scripts\verify.ps1 -Fast` (matriz 4 cuadrantes, release); iteración `.\scripts\verify.ps1 -Quick`; sin JIT `VARN_NO_JIT=1 vn run tests/main.vn`. Cifras aquí = sesión indicada, no cita: re-medir antes de usar.

Sustituye a `2026-10-09-plan-beat-bun.md`, `2026-10-09-plan-async-fase-b.md`
y `2026-10-08-plan-jit-machine-code.md` (eliminados; su contenido vive
aquí, incluido el trabajo sin commitear que había en el árbol).
Memoria viva: actualizar estado con cada item verde. Si contradice al
código, el código manda (Ley 12: las cifras las produce la auditoría,
no se copian de documentos).

## 0. Reglas (Ley 10 + disciplina de medición)

- Cada item declara (a) ganancia, (b) medida, (c) qué se borra.
- Bench pareado mismo perfil + checksum antes que tiempo
  (`tests/benchmarks/README.md`: firma vacía o distinta = MISMATCH).
- Iterar en `quick`; números contra Bun/Node solo en `release`
  (Ley 13). Sin re-litigar ADRs ni secciones cerradas.
- Un item sin ganancia medida se aparca, no se insiste.

## 1. Estado medido
- Baselines rivales históricas (release, i7-1355U): class_fields
  11.3ms vs node 3.0ms (1.88× vs bun); matrix 15.2 vs 7.9
  (1.43× vs bun 10.6); http_routing 286.6 vs 115.4 (2.30× vs bun);
  str_ops 124.0 vs 78.6; json_api 40.1 vs 22.3; json_native 35.1
  vs 24.0; csv 1.16/1.33×; collection 1.11×. Arranque 2.5× ya
  ganado (19.1 vs 48.6ms, no tocar). Marcador original: 2
  ganados, 3 empatados, 9 rivales.
- Los 9 perdedores van 100% JIT, 0 bails, 0 bloqueos: el gap es
  calidad de código máquina, no tiering.
- Re-auditoría 2026-10-09 (código + ejecución): 24/24 benches con
  checksums sanos; perfiles intactos (class_fields 100%/1
  `toString`/5 allocs; http 16/16, 1.34M vm-fast, 0 slow; str_ops
  12/12, `toString` 611k); sin ops JSON en el JIT; `matrix` y
  `matrix_local` son el mismo fichero.
- Comparativa oficial (`xtask compare --skip-python`, release,
  7 runs/2 warmup, i7-1355U): primera pasada 7-2-4 con 1 MISMATCH
  (`new_retained`, bug de versionado ya fixeado en `b00f7d6b`);
  segunda pasada **3-2-9, integridad 100% verificada, cero
  mismatches**. Absolutos entre pasadas NO comparables (térmica:
  bun startup 44→67ms entre sesiones; lo válido es pareado por
  fila + A/B por item).
- Caveats de medición: `class_fields` reporta bun/node ~0.0ms
  (JSC/V8 eliminan el loop sin escape — veredicto `--`;
  README ya avisa; el bench necesita objetos que escapen para
  medir alloc de verdad). `new_retained` verificado 1.32×
  slower (9.8 vs 7.4ms) tras el fix.

## 2. Verde (hecho, con commits)

- **C1 class_fields ~2.9×** (`f91f7ee2`): store int nativo sin
  box/unbox (`store_compact_native`), forwarding store→load con
  clave canónica de global, caché de base por `Variable` solo
  globales (`GLOBAL_CANON_BIT`; vars de loop excluidas o test
  139 corrompe tras evacuación). Medida A/B quick 21.0→7.2ms.
- **C2-overflow matrix 1.22×** (`1f3a8e05`): `from_ssa/ranges.rs`
  (intervalos, merge entry/latch, síntesis ±1, vacío si no
  converge) elide `smul/sadd_overflow` en índices;
  `scalar.rs` vía `no_overflow`. A/B quick 14.67→12.01→12.01ms.
  Matrix 12→5 sitios overflow en CLIF.
- **Cimientos O (otra sesión, verdes)**: substring views v1
  (slice −27%, 12/12 checksums); bridge str con vistas
  (`use<>`, slice/split ~2.2× intérprete); nursery ×16
  (minors colapsan, pared −16/−32/−23%); `ArrayLength` inline
  (bisección http v3split 135→102ms); JSON state machine
  simplificada (neutral, sonda tortura verde).
- **B0 batería generadores** (sin producto): checksums JIT/NO_JIT
  idénticos; pingpong 100k=31ms, mapfilter 100k=44ms (quick).

## 3. En curso en otra sesión (NO TOCAR sin coordinar)

- **O1** bump-alloc inline nursery (1.1M allocs str, 1.45M http).
- **O2** tax por llamada en callees alloc-free (http 100k vm-fast).
- **O3** str inline charCode/toString/substring/slice (3.1×, 46ns,
  142ns×200k).
- **O4** tip-append en concat con literal (concat_parts 2.2×).
- **O5** Map literal/get especializado (500k Map en http).
- **O6** comparaciones Dyn ordenadas con fast path.
- **O7** bails `Dynamic→Int` en block-params.
- **B1** generadores (quitar gate, partir en `resume_ip`, entradas
  OSR, driver `osr_request`). **B2** tasks (freeze/thaw, scheduler
  intacto). **B3** endurecimiento (CatchParam, fuzz×GC, AOT).
- Convivencia: `scalar.rs:emit_bin` lo comparten inducción
  (`in_range_steps`) y C2 (`no_overflow`); `No objetivos` del
  plan O quedó parcialmente stale tras C1/C2 (ver §5).

## 4. Bloqueado tras O-items (diseño listo, sin código)

- **C4 str_ops 1.59×**: O3 + O4 + `int_to_str` chicos, en ese orden.
- **C3 http_routing 2.50×**: O2 + O3 + O5 primero; luego `match`
  por `memcmp` y `params` como mapa inline.
- **C5 json 1.79/1.47×**: puente nativo (string→campo directo,
  números sin `BigDecimal`) + O5; medir parse/stringify/traversal.
- **C6**: re-medir csv/collection tras lo anterior; item propio
  solo si gap >1.2× con causa.

## 5. Aparcado con causa atribuida

- **C2-bounds matrix ~1.13× adicional** (quick 12.01→10.6ms,
  checksums intactos, 100% JIT 0 bails): versionado innermost en
  `drive.rs` (`lower_block` + `emit_version_guard` + clones
  fast/slow): guarda `len ≥ maxidx+1` en preheader (vistas
  resueltas por `prefill` de los objetos de la guarda, NO por
  `pins`: el retain por pins filtraba el 100% de los planes y el
  versionado jamás disparaba); fast elide `ult` (`arrays.rs`
  vía `Ctx::elide` + `fast_bounds`), slow intacto como fallback;
  si un objeto no está materializado en preheader se emite el
  término original (sin versionar) en vez de petar.
  Elegibilidad en `ranges.rs` (solo innermost, sin calls/push/
  stores-globales, índices acotados ≥0, objetos invariantes,
  preheader único con Jump; sin OSR).
- **C1-hoist-total class_fields 1.25×** (A/B quick ABA
  7.25→5.78→5.87ms, checksums intactos): LICM de `LoadGlobalIdx`
  en `drive.rs` (preheader por loop, slots nunca-escritos en el
  body + cuerpos puros sin user-code; kills en stores/calls) +
  `ranges::loop_sets` compartido. Reglas de sanidad: reuso en
  header solo si el slot no se escribe en el loop (si no, loop
  infinito con valor pre-loop); al reusar global boxeada en slot
  Int, pasar por `land`+unbox (si no, confusión I128/I64 que el
  backend peta); mapas `values` por versión (el compartido
  contaminaba sucesores con valores de clones y rompía
  `new_retained`: guarda sin `skip_term`, `band` sin `uextend`).
- **`ArrayPush` inline**: exige auditar barrera GC (riesgo).
- **`matrix_local`**: duplicado exacto de `matrix`; dedupe con
  dueño de benches, no borrar unilateral.

## 6. No tocar

- fib, dto, json_pure, gc_alloc (ganan o empatan;
  outputs re-verificados post-rangos). `new_retained`: era
  MISMATCH por el bug de versionado, hoy verificado 1.32× slower
  — pasa a rivales (gap pequeño, sin item hasta O-items).
- task_soak/task_spawn: 100% intérprete hasta B1–B3.
- task_soak 346ms / task_spawn 114ms; async gates intactos.
- Modelo tasks FIFO; AOT bailea async/generadores.

## 7. Orden

C1 ✓ → C2-overflow ✓ → C2-bounds ✓ → C1-hoist-total ✓ → O-items (ajenos) → C4 → C3 → C5 →
re-medir C6. Cada item: pareado + checksum + 4 cuadrantes +
commit atómico.
