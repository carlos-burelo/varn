# Benchmarks

Micro-benchmarks `.vn` con su contraparte TypeScript (`.ts`) para comparación
pareada. La referencia es **Bun** (JavaScriptCore), no Node: los `.ts` se corren
directamente sin transpilar.

## Contenido por carpeta

- `vn/` — benchmarks Varn (20 ficheros `bench_*.vn`).
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

**Térmica.** La máquina baja ~2x bajo carga sostenida (un `cargo build` la
calienta). Solo son comparables las corridas pareadas en el mismo momento. Tomar
una lectura en frío como canario antes de confiar en números absolutos.

**No copiar números históricos.** Medir de nuevo. Ver `<performance_rules>` en
`CLAUDE.md`.

---

## Intérprete vs JIT

```
VARN_NO_JIT=1 vn bench tests/benchmarks/vn/bench_fib.vn -v
```

Apaga el JIT por completo (0 funciones compiladas, 0 B de código máquina). Sirve
para saber cuánto del tiempo es codegen y cuánto es representación.

---

La suite amplia de correctitud + timing vive en `tests/main.vn`
(`vn bench tests/main.vn`), no aquí.
