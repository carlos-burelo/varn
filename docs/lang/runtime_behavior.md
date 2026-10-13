# Varn — Comportamiento en tiempo de ejecución (links)

> Fuentes: `tests/21-async.vn`, `tests/47-isolates-multithread.vn`, `../VM_ARCHITECTURE.md`, `../RUNTIME_ARCHITECTURE.md`, `../COMPILER_ARCHITECTURE.md`.

Sin cifras: FrameStore, GC, JIT y pipeline ya están documentados en esos archivos.
No duplicarlos aquí.

## Lo verificado en tests

- Dos backends: intérprete bytecode y JIT. `VARN_NO_JIT=1` fuerza intérprete
  (matriz de validación en `CONTRIBUTING.md`).
- Heap compartido en hilo principal; cada `spawnIsolate` crea heap separado,
  comunicación solo por canales
  (`tests/47-isolates-multithread.vn:10-39`).
- Top-level `await` en módulo (`tests/21-async.vn:98`,
  `tests/77-async-generators.vn:105`).
- `isIsolate`: `true` dentro del isolate hijo; guarda `if (!isIsolate)`
  para no re-ejecutar (`tests/47-isolates-multithread.vn`).
- `int` 64 bits con `IntegerOverflow`, sin wrap
  (`tests/120-int-division.vn:25`); `int / int` = `int` truncado
  (`tests/120-int-division.vn:16-20`).
- Stdlib dos procedencias: árbol `std/` en dev, bundle embebido en release
  (`VARN_STD=@embedded`).

## Closures

Capturan por upvalue del frame padre (`tests/07-closures.vn:7`); ver
`../VM_ARCHITECTURE.md` para el mecanismo.
