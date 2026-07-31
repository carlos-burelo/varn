# Rediseño del output de `vn bench` y `vn debug`

Fecha: 2026-07-30
Estado: aprobado, pendiente de plan de implementación
Commit base: `aa6dc54`

## 1. Objetivo

El output de `vn bench` y `vn bench -v` contiene casi todos los datos necesarios
para diagnosticar el estado actual del backend, pero los presenta de forma que el
hallazgo dominante queda invisible, y en varios puntos reporta números
incorrectos o engañosos.

Este rediseño persigue tres cosas:

1. Corregir las métricas que hoy son matemáticamente inválidas.
2. Elevar a titular el eje que gobierna el trabajo de perf actual: **compilar vs
   correr**, y **cobertura Cranelift**.
3. Alinear el output con el destino declarado del proyecto: **el intérprete y el
   template JIT desaparecen; el 100% de la ejecución pasa por Cranelift**. Bajo
   ese objetivo, varias cifras dejan de ser estadísticas neutrales y pasan a ser
   contadores de defecto con objetivo cero.

## 2. Hallazgos medidos que motivan el rediseño

### 2.1 El JIT hace `tests/main.vn` 2.3x más lento

A/B suelto sobre el mismo binario release, corridas consecutivas:

| Configuración | `execute` p50 | min | max | σ |
|---|---|---|---|---|
| clif activo | 48.65 ms | 44.34 ms | 52.01 ms | 2.408 ms |
| `VARN_NO_JIT=1` | 21.46 ms | 19.50 ms | 23.43 ms | 1.246 ms |

Delta ≈ **27.2 ms por corrida**, del mismo orden que los 36.35 ms de
`total compile time` que reporta el `-v` (la diferencia se explica por deriva
térmica entre corridas; ver 2.4).

No es un A/B pareado, así que la cifra exacta no es de fiar. La magnitud sí: 27 ms
está muy por encima del ruido observado.

### 2.2 La compilación entra en cada corrida cronometrada

`vn bench` recompila las 31 funciones en **cada** corrida medida, no solo en el
warmup. Evidencia directa: el `-v` reporta `freshly compiled 31` después de las
10 corridas.

Mecanismo: `Vm::from_snapshot` hace `ctx.heap.deep_clone()`
(`crates/varn-vm/src/vm.rs:48`) y `Vm::run` re-resuelve el pool de constantes
contra ese heap nuevo (`vm.rs:55`). Los protos alcanzables por el heap llegan a
cada corrida con `jit_entry` vacío, aunque `proto_rc` y el mapa `precompiled` sí
se comparten por `Rc`.

Consecuencia para leer el bench: **`execute p50` no es steady-state**, es
compile+run en frío. El costo de compilar nunca se amortiza dentro del bench.

Con clif como única ruta esto deja de ser un artefacto del harness: pasa a ser el
costo real de cualquier programa corto.

### 2.3 `VM Opcode Hotspots` describe el ~5% del programa

Los contadores de opcode solo se incrementan en el despacho del intérprete. Con
94.9% de los frames ejecutándose en clif, los 13 625 opcodes reportados cubren
únicamente la fracción interpretada, bajo un título que sugiere ser el perfil
completo del programa. `Register VM Stats` (Move opcodes, frame pushes/pops)
tiene exactamente el mismo problema.

### 2.4 La máquina deriva térmicamente dentro de una sesión

La corrida del usuario marcaba `execute p50` 41.82 ms; minutos después, el mismo
binario marcaba 48.65 ms. ~16% de deriva sin cambio de código. El output no
advierte nada de esto.

## 3. Alcance

### Dentro

- Reestructuración de los cuatro archivos de bench en un dominio `bench/`.
- Corrección de las métricas inválidas (sección 5).
- Headline nuevo (sección 6).
- Sección de cobertura Cranelift en `-v`, reemplazando `JIT Compiler & Execution
  Stats` (sección 7).
- Fases `tiers`, `bails` y `summary` en `vn debug`; filtro `--fn`;
  `--list-phases`; `--json` (sección 8).

### Fuera

- Salida `--json` para `bench` (sí para `debug`).
- Baselines persistidos y diff contra baseline.
- Arreglar la recompilación por corrida. Este spec **reporta** el problema; no lo
  corrige.

## 4. Estructura de archivos

`bench_impl.rs` tiene 882 líneas, por encima del umbral de refactor recomendado
(700) del gobierno de tamaño de archivos.

Además, `fmt_dur`, `fmt_f` y `fmt_bytes` están duplicados literalmente entre
`bench_phase.rs:58-115` y `bench_output.rs:250-297,357-365`, y `fmt_num` tiene
**tres** copias (las dos anteriores más `bench_hotspots.rs:9-19`).

```
crates/varn-cli/src/bench/
  mod.rs         orquestación de run_bench
  harness.rs     time_n, time_n_freq, warmup, muestreo
  stats.rs       PhaseStats, CV, detección de outliers
  report/
    fmt.rs       única copia de fmt_dur / fmt_num / fmt_bytes / fmt_pct
    headline.rs  veredicto y avisos
    table.rs     tabla de fases
    coverage.rs  cobertura Cranelift
    profile.rs   VM / GC / IC
    hotspots.rs  funciones, nativos, globals, allocs
```

Los archivos `bench_impl.rs`, `bench_output.rs`, `bench_phase.rs` y
`bench_hotspots.rs` desaparecen. No se mantiene ruta paralela.

## 5. Correcciones de métricas

| Problema | Ubicación | Corrección |
|---|---|---|
| `e2e` reporta 102.7% del total; su `%` se calcula contra la suma de fases, pero e2e no es una fase | `bench_phase.rs:141-157` | `e2e` sale de la tabla. Va bajo la regla, con `overhead = e2e_p50 − Σ fases` explícito en lugar de un porcentaje |
| `total.min` y `total.max` son suma de mins y suma de maxes | `bench_phase.rs:159-161` | Se dejan vacíos. No son derivables sin muestras pareadas por corrida |
| Path con prefijo Windows `\\?\` | `bench_impl.rs:127` (`canonicalize_path`) | Strip del prefijo; mostrar relativo al cwd cuando esté debajo |
| Decimales elegidos por celda: `8.82 µs` junto a `202 µs` junto a `37.87 ms` | `fmt_dur`, ambas copias | Unidad y decimales fijos **por columna**, elegidos por la magnitud de la columna |
| `W_NAME = 28` contra nombres de global de ~70 chars (paths absolutos) rompe la alineación | `bench_hotspots.rs:6,98-110` | Nombre corto (`31-stdlib-migration-test::hi`), truncado por el medio si excede |
| Breakdown del checker suma 36.9 µs contra `check.p50` de 52.6 µs: 30% sin atribuir | `bench_output.rs:19-30` | Fila `other` explícita con el residual |
| 5 de 7 filas del checker y 2 de 4 del parser son `0 ns 0%` | `bench_output.rs:208-248` | Ocultas por defecto; `--all-rows` para mostrarlas |
| Cuatro anchos de columna distintos: `{:<14}`, `{:<22}`, `{:<26}`, `{:<28}` | los tres archivos | Una constante compartida en `report/fmt.rs` |
| Tres contadores de alocación que no reconcilian: `heap allocs 23 251`, `nursery allocs 31 726`, suma de `Allocation Types` ≈ 12k | `bench_output.rs:125-166` + `bench_hotspots.rs:112-124` | Fila de conciliación que explique la relación entre los tres |
| `Total pipeline time` repite la columna `total` de la tabla | `bench_impl.rs:513-519` | Eliminada |
| `Peak CPU freq: 1700 MHz (base 1700 MHz · 100% · base)` dice "base" tres veces; `(temp n/a — needs kernel driver)` en cada corrida | `bench_impl.rs:520-542` | Una línea, sin redundancia. El aviso de temperatura desaparece |
| `Cold-start throughput` suma precompile + p50, omitiendo la compilación JIT | `bench_impl.rs:549-561` | Ver sección 6: el desglose compilar/correr lo sustituye |

## 6. Headline

```
  varn bench · tests/main.vn
  10 runs · release · clif · aa6dc54

  49.5 ms  p50 e2e

  execute 48.7 ms  =  compilar 27.2 ms (56%)  +  correr 21.5 ms (44%)
                      31 fns · 0.88 ms/fn       1.00 compilaciones/corrida

  cobertura clif   986/1039 frames (94.9%)   [ámbito: módulo entrante]
  ✗ 1 de 32 funciones fuera de clif · 53 frames al intérprete
      <module>   gate: >250 words   [frames clif no reanudables]

  ⚠ spread 15% — A/B suelto no confiable
```

El split se calcula como `execute_p50 − compile_time`, independiente de cualquier
corrida con `VARN_NO_JIT`. Nótese que en la medición de 2.1 los 21.5 ms de
"correr" en clif coinciden casi exactamente con los 21.46 ms del intérprete
completo: sugiere que clif tampoco gana en tiempo de ejecución sobre este
workload, no solo que pierde en compilación. Es una observación de un A/B suelto,
no una conclusión; el output debe exponer el número, no interpretarlo.

### Datos nuevos que requiere

- **Identidad del binario** — `release`/`debug` vía `cfg!(debug_assertions)`;
  backend activo vía `clif::enabled()`; commit vía `option_env!("VARN_GIT_SHA")`.
  Si no hay build script que lo defina, el campo se omite en lugar de mentir.
- **Split compilar/correr** — leer `JIT_STATS.total_compile_time_ns` alrededor de
  la fase execute. Los atómicos ya existen (`varn-jit/src/lib.rs:286-295`).
- **`compilaciones/corrida`** = `compile_success / runs`. Es la métrica de
  amortización: **1.00 significa cero amortización**. Baja cuando se arregle el
  cacheo entre corridas; sube cuando `JIT_TIER_THRESHOLD` pase de 1.
- **CV** = `σ / p50` por fase. Umbrales: >5% aviso, >10% "A/B suelto no
  confiable". Ata el output al método de benchmark pareado.

### Decisión: fuera el comparativo automático contra el intérprete

Se descarta ejecutar `VARN_NO_JIT=1` por defecto para comparar. Como consejo de
rendimiento apuntaría a un estado que el proyecto está eliminando.

Se conserva como flag explícito `--vs-interp`, cuyo propósito es **paridad de
tiers**, no rendimiento. Sigue siendo necesario: `JIT_TIER_THRESHOLD` no puede
pasar de 1 por un bug de paridad en closures/upvalues.

## 7. Sección de cobertura en `-v`

Reemplaza al bloque `JIT Compiler & Execution Stats`.

```
Cobertura Cranelift                        [ámbito: módulo entrante]
  ruteadas                 31   96.9%
  gate (nunca ofrecidas)    1    3.1%   <module> >250 words
  bail de lowering          0    0.0%
  ──────────────────────────────────
  funciones                32  100.0%

  frames clif             986   94.9%
  frames intérprete        53    5.1%   ← objetivo 0

  compilar   27.2 ms   0.88 ms/fn   184 KB
  top-3:  <arrow> 4.1 ms · describe 2.8 ms · classify 2.2 ms
```

### 7.1 Fuente de datos

`ClifInspection` (`varn-jit/src/clif/debug.rs:96-104`) ya expone
`route: Result<(), String>` por función, y `inspect()` corre la lowering real sin
ejecutar. No hace falta instrumentación nueva para la clasificación
ruteada/bail.

Falta añadir: tiempo de compilación y tamaño de código **por función**. Hoy
`JIT_STATS` solo acumula totales.

### 7.2 Las tres filas deben sumar, o el número miente

El gate de `code.len() > 250` dispara **antes** de ofrecer la función a clif
(`varn-jit/src/lib.rs:357-369`). El propio comentario del código lo advierte: una
función rechazada ahí no aparece ni en `CLIF BAIL` ni en `compile_fail`, y
contar "0 bails" sin contarla **sobreestima la cobertura**.

`inspect()` tampoco aplica ese gate — vive en `varn_jit::compile`, fuera de
`try_compile`. La tabla debe aplicarlo explícitamente o reportará como ruteadas
funciones que en producción nunca llegan a clif.

Ese gate no es un presupuesto de compilación: es lo que impide frames clif
anidados, que no son reanudables. Levantarlo exige hacerlos reanudables, no subir
el número. Por eso el `<module>` de `main.vn` aparece como el bail dominante y el
output debe nombrar la causa, no solo el síntoma.

### 7.3 Secciones que pasan a ser interpreted-only

`VM Opcode Hotspots` y `Register VM Stats` se rotulan explícitamente como
cobertura del intérprete únicamente, con el porcentaje de frames que representan.
Se apagan solas conforme el intérprete desaparezca.

## 8. `vn debug`

Fases nuevas:

- **`-p tiers`** — tabla por función: tier, words de bytecode, ruteo, razón de
  bail, tiempo de compilación, bytes de código.
- **`-p bails`** — solo lo que no rutea, agrupado por gate vs lowering:

  ```
  vn debug tests/main.vn -p bails

    gate (>250 words)
      <module>            412 words    [no reanudable]
    lowering
      (ninguna)
  ```

- **`-p summary`** — grafo de módulos, número de funciones y globales, tamaño de
  bytecode, pool de constantes, top-10 funciones más grandes.

Transversales:

- **`--fn <nombre>`** — filtra cualquier dump (`hir`, `ssa`, `bytecode`, `clif`).
  Hoy `-p bytecode` sobre `main.vn` vuelca el módulo entero, lo que lo hace
  inutilizable.
- **`--list-phases`** — hoy las fases solo son visibles en el mensaje de error de
  `flags.rs:183-190`.
- **`--json`**.

## 9. Riesgos y límites conocidos

1. **El denominador de funciones solo cuenta lo que el JIT llegó a ver.** El top
   level del módulo y los generadores nunca se le ofrecen, así que sus frames
   interpretados no tienen función que los explique. Medido: 53 frames
   interpretados con 0 funciones rechazadas. Mitigado rotulando la fila como
   `ofrecidas al JIT` y emitiendo una nota explícita cuando hay frames
   interpretados sin ninguna función rechazada. **Los frames, no las funciones,
   son la cifra de cobertura honesta.**
2. **El tiempo de compilación se agrega across threads.** Con isolates, varios
   hilos compilan en paralelo y el agregado puede superar el wall-clock de
   `execute` (medido: 130 ms de compilación en una fase de 90 ms). Cuando eso
   pasa, la resta `execute − compilar` no significa nada y el output reporta el
   agregado en vez del split.
3. **Las cifras del JIT vienen de una corrida instrumentada aparte.** Promediar
   un snapshot multi-corrida mezcla magnitudes por-corrida (tiempo de
   compilación) con acumuladas (conteo de funciones); ese error producía
   "compilar 144% del execute".
4. **`cargo test` no enlaza en este entorno.** `undefined symbol: .varn_ops$A` /
   `.varn_ops$C`: el perfil `dev` no fija `codegen-units = 1`, que los marcadores
   de sección MSVC de `varn-builtins` requieren. Afecta a `varn-lsp`, `xtask`,
   `varn-vm` y `varn-pipeline` por igual — es previo e independiente de este
   cambio. Los tests unitarios de `bench/` se ejecutan con
   `cargo test -p varn-cli --release --bin vn`.
5. **El commit en el headline requiere build script.** Sin él, el campo se omite.

## 10. Validación

Según las reglas de validación del proyecto, `cargo test` no es indicador
suficiente. El cambio se valida con:

```
cargo run --release --bin vn -- bench ./tests/main.vn -v
cargo run --release --bin vn -- run ./tests/main.vn
VARN_NO_JIT=1 cargo run --release --bin vn -- bench ./tests/main.vn -v
vn debug ./tests/main.vn -p tiers
vn debug ./tests/main.vn -p bails
```

Criterios de aceptación, todos verificados:

| Criterio | Resultado |
|---|---|
| Ningún porcentaje supera 100% | ✅ |
| La fila `total` no reporta min/max derivados de sumas | ✅ imprime `—` |
| `ruteadas + gate + bail` suma las funciones inspeccionadas | ✅ 31+0+0 = 31 |
| Todo número de cobertura lleva ámbito rotulado | ✅ `[ámbito: programa completo]` |
| Las secciones interpreted-only declaran qué fracción cubren | ✅ `[solo intérprete — 5.1% de los frames]` |
| `vn debug -p bails` reporta los bloqueadores por `gate` | ✅ `std:path::normalize` 405 words, `dirname` 434, top-levels hasta 1202 |
| Sin copias duplicadas de `fmt_dur`/`fmt_num`/`fmt_bytes` | ✅ una sola, en `report/fmt.rs` |
| El backend rotulado coincide con el que corre | ✅ `interp (VARN_NO_JIT)` bajo el flag |
| `tests/main.vn` pasa con y sin JIT | ✅ 787/787 en ambos |
| Build sin warnings | ✅ |
| Tests unitarios de `bench/` | ✅ 7/7 |

Como el output es la herramienta de medición, cualquier comparación de
rendimiento antes/después debe usar el método pareado: dos binarios, orden
alternado, mínimo de 6, y un benchmark de control. La deriva térmica documentada
en 2.4 (~16% en una sesión) invalida cualquier A/B suelto.
