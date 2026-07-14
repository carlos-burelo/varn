# Benchmarks

Micro-benchmarks `.vn` con su contraparte TypeScript (`.ts`) para comparación
pareada. La referencia es **Bun** (JavaScriptCore), no Node: los `.ts` se corren
directamente sin transpilar.

```
vn bench benchmarks/bench_fib.vn        # arnés de medición del CLI
vn bench benchmarks/bench_fib.vn -v     # + IC, GC, JIT, hotspots de opcodes
```

Comparación pareada:

```
vn run  benchmarks/bench_fib.vn
bun run benchmarks/bench_fib.ts
```

---

## Disciplina de medición

**Primero la salida, después el tiempo.** Un benchmark que imprime un resultado
distinto al de su par `.ts` no está midiendo el mismo trabajo, y su número es
basura. Ya pasó: durante semanas `bench_dto` construía 664 DTOs en vez de 43 332
y por eso "ganaba". Antes de citar un tiempo, comprobar que `vn` y `bun` imprimen
lo mismo.

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
VARN_NO_JIT=1 vn bench benchmarks/bench_fib.vn -v
```

Apaga el JIT por completo (0 funciones compiladas, 0 B de código máquina). Sirve
para saber cuánto del tiempo es codegen y cuánto es representación.

---

La suite amplia de correctitud + timing vive en `tests/main.vn`
(`vn bench tests/main.vn`), no aquí.
