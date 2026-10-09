# Plan ganarle a Bun — optimización por caso (2026-10-09)

Base: `cargo xtask compare --skip-python` (release, 7 runs/2 warmup,
i7-1355U, checksums 100% verificados). Marcador: 2 ganados, 3 empatados,
9 rivales. Arranque 2.5× (19.1 vs 48.6ms, ya ganado, no tocar).

Reglas: cada item declara (a) ganancia, (b) medida, (c) qué se borra
(Ley 10). Bench pareado mismo perfil + checksum antes que tiempo
(disciplina `tests/benchmarks/README.md`). Sin re-litigar O1–O8
(`2026-10-08-plan-jit-machine-code.md`): se referencian, no se duplican.
Medido hoy: los 9 perdedores van 100% JIT, 0 bails, 0 bloqueos — el gap
es calidad de código máquina, no tiering.

## C1 — class_fields 3.70× (peor caso)

- Baseline: 11.3ms vs node 3.0ms (1.88× vs bun 6.0ms).
- Workload (`bench_class_fields.vn`): 1M iteraciones de
  `pt.x = pt.x + 1` (read-modify-write en mismo objeto y shape).
- Perfil: JIT 100%, 1 solo `toString` nativo, 5 allocs totales.
  Pérdida íntegra en el loop: shape-check + box/unbox por acceso.
- Qué:
  1. Hoist del shape-check fuera del loop cuando el objeto no escapa
     (licencia: `emit_get/set_fixed_field` en
     `varn-jit/src/clif/from_ssa/props/fields.rs:17,124`).
  2. Fast path int sin box en RMW (`x = x + 1` con kinds probados).
  3. Store-load forwarding `pt.x` leído tras escribirlo (eliminar el
     segundo load).
- Verifica: `bench_class_fields` pareado + `debug -p clif --fn` sin
  checks en el loop + 4 cuadrantes.

## C2 — matrix 1.92×

- Baseline: 15.2ms vs node 7.9ms (1.43× vs bun 10.6ms).
- Workload (`bench_matrix.vn`): 150³ = 3.4M accesos `a[i]` con
  multiplicación/suma int.
- Qué:
  1. Bounds-check una sola vez por loop (índices probados por
     construcción `row*n+k` con límites del loop).
  2. Hoist de base de array fuera del loop interno.
  3. Overflow-check de int fuera del camino caliente o con bail
     especulativo (inducción ya es `iadd` pelado: no tocar).
- Verifica: `bench_matrix` + `bench_matrix_local` pareados + tiers.

## C3 — http_routing 2.50×

- Baseline: 286.6ms vs 115.4ms (2.30× vs bun). 1.34M llamadas vm-fast,
  0 slow: pura densidad de calls/str/map/closures.
- Qué (composición de O-items + específico):
  1. O2 (tax por llamada) + O3 (str) + O5 (Map) primero: son el grueso.
  2. `match` de segmentos literales por `memcmp` en vez de loop con
     `startsWith`/`slice` por segmento.
  3. `params` como mapa pequeño inline (O5) en vez de mapa general.
- Verifica: `bench_http_routing` pareado + checksums de rutas.

## C4 — str_ops 1.59×

- Baseline: 124.0ms vs 78.6ms. Medido hoy: `toString` 611k×25ns =
  15.6ms (12.6% del total); resto concat/charCode/slice/join.
- Qué: O3 (charCode/toString/substring/slice inline) + O4 (concat
  con tip-append) + `int_to_str` para enteros pequeños sin alloc.
  En ese orden (peso medido).
- Verifica: `bench_str_ops` por sección con checksums 12/12 intactos.

## C5 — json_api_payloads 1.79× + json_native 1.47×

- Baseline: 40.1 vs 22.3ms; 35.1 vs 24.0ms.
- Qué:
  1. Materialización directa string→campo (evitar `String` temporal,
     patrón del puente con vistas ya verde).
  2. Números sin `BigDecimal` intermedio cuando encajan en f64/int.
  3. Reutilizar la state machine simplificada (precedente neutral).
- Verifica: `bench_json*` pareados + roundtrips idénticos.

## C6 — csv_pipeline 1.16×, csv_etl 1.33×, collection_pipeline 1.11×

- Gaps marginales con raíces compartidas (split, `push`, map, closures).
- Qué: NINGÚN item dedicado todavía. Se benefician de O3 (split/slice),
  O1 (alloc arrays/maps) y O5. Re-medir tras C3–C5; abrir item propio
  solo si algún gap supera 1.2× con causa atribuida.
- Verifica: re-medir en la misma sesión que C3–C5.

## No tocar (empatados/ganados o fuera de alcance)

fib, dto, json_pure, gc_alloc, new_retained (ganan o empatan);
task_soak/task_spawn (100% intérprete: territorio plan async B1–B3,
no peepholes aquí); O7 bails y O8 ya tienen dueño en sus planes.

## Orden sugerido (por peso × riesgo)

C1 → C4 → C3 → C2 → C5 → re-medir C6. Cada item: bench pareado
antes/después mismo perfil + checksum + 4 cuadrantes + commit
atómico. Un item sin ganancia medida se aparca, no se insiste.
