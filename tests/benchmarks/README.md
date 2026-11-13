> Estado verificado 2026-11-13: VIGENTE — metodología (checksum antes que tiempo, `cargo xtask compare`, re-medir siempre). Sin tabla de ratios históricos que borrar: no hay.
>
# Benchmarks

Micro-benchmarks `.vn` con su contraparte TypeScript (`.ts`) para comparación
pareada. La referencia es **Bun** (JavaScriptCore), no Node: los `.ts` se corren
directamente sin transpilar.

## Contenido por carpeta

- `vn/` — benchmarks Varn (28 ficheros `bench_*.vn`).
- `ts/` — contrapartes TypeScript para Bun/Node.
- `js/` — contrapartes JavaScript planas (las corren Bun y Node).
- `py/` — contrapartes Python + `_harness.py` compartido.
- `scripts/` — `compare.ps1`, wrapper de `cargo xtask compare`.

## Tabla comparativa: `compare.ps1`

Corre cada benchmark en Varn, Bun, Node y Python uno tras otro y saca una sola
tabla. Es la forma recomendada de medir en cada iteración de mejora.

```powershell
.\tests\benchmarks\scripts\compare.ps1                          # todo
.\tests\benchmarks\scripts\compare.ps1 -SkipPython              # Python es 10-40x mas lento
.\tests\benchmarks\scripts\compare.ps1 -Only matrix,array_ops
.\tests\benchmarks\scripts\compare.ps1 -Markdown                # para pegar en docs
.\tests\benchmarks\scripts\compare.ps1 -NoJit                   # cuanto aporta el JIT
```

Aplica por su cuenta las dos reglas de este README: verifica que **todos** los
runtimes produzcan el mismo checksum antes de comparar tiempos (uno que
discrepe sale como `FAIL` y queda fuera del ratio), y corre los runtimes de un
benchmark de forma adyacente para que compartan condiciones. Avisa si `vn.exe`
es más viejo que el fuente, y marca las filas cuyo CV supera el 10%.

La columna `vs best` es Varn dividido por el rival **más rápido**, que no siempre
es Bun — Node gana en varios.

Las contrapartes viven en `js/` (las corren Bun y Node) y `py/`. Los tres arneses
imprimen checksum a stderr y milisegundos a stdout, que es lo que el script lee.

```
vn bench tests/benchmarks/vn/bench_fib.vn        # arnés de medición del CLI
vn bench tests/benchmarks/vn/bench_fib.vn -v     # + IC, GC, JIT, hotspots de opcodes
```

Comparación pareada (desde la raíz del repo):

```
vn run  tests/benchmarks/vn/bench_fib.vn
bun run tests/benchmarks/ts/bench_fib.ts
```

---

## Resultados (2026-11-13)

Binario `release` de `85b0e6bb` (anterior a `01b21300`, que cambia el comparador de `sort`;
`sort_objects` no se volvió a medir en esta matriz), Windows 11 x64, una corrida de `compare.ps1 -Runtimes vn,bun,node -Runs 5
-Warmup 2`. Milisegundos, mediana. «Proceso» es el tiempo de pared del proceso completo;
«estable» es proceso menos arranque. Una sola corrida de la matriz: diferencias de unos
5 ms están dentro del ruido.

| Carga | vn proceso | node | bun | vn estable | node | bun |
|---|---|---|---|---|---|---|
| `array_ops` | 25.4 | 107.0 | 62.0 | 6 | 67 | 46 |
| `class_fields` | 20.8 | 44.9 | 19.8 | 2 | 5 | 4 |
| `collection_pipeline` | 34.4 | 65.4 | 37.5 | 15 | 25 | 22 |
| `dto` | 29.5 | 156.7 | 140.4 | 10 | 116 | 125 |
| `dto_local` | 29.9 | 59.0 | 35.0 | 11 | 19 | 19 |
| `fib` | 49.2 | 666.2 | 399.0 | 30 | 626 | 383 |
| `gc_alloc` | 41.8 | 263.9 | 311.2 | 22 | 224 | 296 |
| `generator` | 34.8 | 46.7 | 25.8 | 15 | 6 | 10 |
| `http_routing` | 154.5 | 158.5 | 125.1 | 135 | 118 | 109 |
| `json` | 41.3 | 60.1 | 38.0 | 22 | 20 | 22 |
| `json_api_payloads` | 33.7 | 55.4 | 35.5 | 14 | 15 | 20 |
| `json_parts` | 38.1 | 60.3 | 37.9 | 19 | 20 | 22 |
| `json_pure` | 254.7 | 267.5 | 233.0 | 235 | 227 | 217 |
| `math` | 23.7 | 108.9 | 105.5 | 4 | 69 | 90 |
| `matrix` | 27.8 | 93.6 | 67.0 | 8 | 53 | 51 |
| `matrix_local` | 31.9 | 52.6 | 23.2 | 13 | 12 | 8 |
| `multiple` | 167.6 | 285.0 | 174.0 | 148 | 245 | 158 |
| `new_retained` | 26.3 | 54.8 | 29.5 | 7 | 14 | 14 |
| `now` | 23.8 | 51.9 | 32.4 | 4 | 12 | 17 |
| `polymorphism` | 122.3 | 181.7 | 56.1 | 103 | 141 | 40 |
| `sort_objects` | 240.8 | 123.3 | 87.3 | 222 | 83 | 72 |
| `str_ops` | 81.4 | 90.8 | 83.6 | 62 | 50 | 68 |
| `word_freq` | 52.0 | 98.4 | 61.8 | 33 | 58 | 46 |
| `csv_etl` | 36.2 | — | — | 17 | — | — |
| `csv_pipeline` | 116.3 | — | — | 97 | — | — |
| `gpu_matmul` | 253.4 | — | — | 234 | — | — |
| `task_soak` | 341.8 | — | — | 322 | — | — |
| `task_spawn` | 124.7 | — | — | 105 | — | — |

Varn es el más rápido en 14 de 23 cargas comparables en tiempo de proceso y en 15 de 23
en estado estable. Las cinco cargas sin par (`csv_etl`, `csv_pipeline`, `gpu_matmul`,
`task_soak`, `task_spawn`) solo corren en Varn.

Mayores brechas en estado estable (vn / mejor rival): `sort_objects` 222 / 72,
`polymorphism` 103 / 40, `generator` 15 / 6, `matrix_local` 13 / 8, `http_routing`
135 / 109, `str_ops` 62 / 50. Las causas medidas y las palancas abiertas están en
[`docs/ROADMAP.md`](../../docs/ROADMAP.md).

---

## Disciplina de medición

**Primero la salida, después el tiempo.** Un benchmark que imprime un resultado
distinto al de su par `.ts` no está midiendo el mismo trabajo, y su número es
basura. Ya pasó: durante semanas `bench_dto` construía 664 DTOs en vez de 43 332
y por eso "ganaba". Antes de citar un tiempo, comprobar que `vn` y `bun` imprimen
lo mismo. `cargo xtask compare` lo verifica solo: firma vacía o con números
distintos sale como `MISMATCH` y queda fuera del ratio.

**Qué compara la firma.** Se ignoran líneas de timing (`elapsed|took|time`) y
fragmentos `xxx_ms=N` / `ms=N`. Los conteos de bytes (`bytes: N`, `N bytes`,
`Bytes = N`) también se excluyen: son artefacto del serializador (Varn imprime
floats integrales como `5.0`, Bun como `5`; `CSV.stringify` de Varn añade `\n`
final), no trabajo semántico. Cada bench debe imprimir al menos un checksum
semántico (conteos, sumas, checksums por sección); sin números comparables la
fila es `MISMATCH` por diseño.

**CSV y JSON nativo miden bindings, no lenguaje.** `bench_csv_*` usa
`CSV.parse/stringify` nativo en Rust mientras el `.ts` implementa el parser a
mano; `bench_json*` enfrenta `serde_json` contra el JSON de JavaScriptCore.
Comparan el costo del binding nativo, no la velocidad del lenguaje. Para
velocidad pura del lenguaje, mirar `fib`, `matrix`, `str_ops`,
`collection_pipeline`.

**Ojo con el escape analysis de Bun.** JavaScriptCore elimina por completo las
allocations que no escapan del bucle. Un micro que hace `new Obj(...)` y solo
suma un campo puede reportar ~0.07 ms en Bun: no es que sea rápido asignando, es
que **no asignó nada**. Para medir allocation de verdad, los objetos tienen que
escapar (guardarlos en un array que siga vivo). Si el número de Bun parece
imposible, lo es.

**Repetir dentro de un proceso distorsiona.** `vn bench` repite la ejecución en el mismo
proceso (`execute (warm)`): la heap crece entre repeticiones y la mediana puede duplicar el
valor de un proceso nuevo. En `bench_polymorphism`: mínimo 190 ms y mediana 390 ms dentro de
`vn bench`, contra unos 150 ms en procesos nuevos (141-213 ms). Para A/B entre binarios, alternar
procesos nuevos (`compare.ps1`, o `vn run` en bucle) y no fiarse de diferencias menores del
10%. El perfil `quick` tampoco sirve para tiempos de ejecución: este mismo benchmark da unos
390 ms en `quick` y unos 150 ms en `release` (la causa no se investigó).

**Térmica.** La máquina baja ~2x bajo carga sostenida (un `cargo build` la
calienta). Solo son comparables las corridas pareadas en el mismo momento. Tomar
una lectura en frío como canario antes de confiar en números absolutos.

**No copiar números históricos.** Medir de nuevo. Ver `<performance_rules>` en
`CLAUDE.md`.

---

## Intérprete vs JIT

```
VN_NO_JIT=1 vn bench tests/benchmarks/vn/bench_fib.vn -v
```

Apaga el JIT por completo (0 funciones compiladas, 0 B de código máquina). Sirve
para saber cuánto del tiempo es codegen y cuánto es representación.

---

La suite amplia de correctitud + timing vive en `tests/main.vn`
(`vn bench tests/main.vn`), no aquí.
