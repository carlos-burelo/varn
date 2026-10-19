# Varn — Comportamiento en tiempo de ejecución (links)

> Fuentes: `tests/21-async.vn`, `tests/47-isolates-multithread.vn`, `../VM_ARCHITECTURE.md`, `../RUNTIME_ARCHITECTURE.md`, `../COMPILER_ARCHITECTURE.md`.

Sin cifras: FrameStore, GC, JIT y pipeline ya están documentados en esos archivos.
No duplicarlos aquí.

## Lo verificado en tests

- Dos backends: intérprete bytecode y JIT. `VARN_NO_JIT=1` fuerza intérprete
  (matriz de validación en `CONTRIBUTING.md`).
- Heap compartido en hilo principal; cada `spawnIsolate` crea heap separado,
  comunicación solo por canales
  (`tests/47-isolates-multithread.vn`).
- Top-level `await` en módulo (`tests/21-async.vn`,
  `tests/77-async-generators.vn`).
- `import.meta.main`: `true` solo en el módulo de entrada; `false` en módulos
  importados y en los que carga un isolate. Vive en el slot 0 de la región de
  globales de cada módulo y lo escribe el loader (`tests/228-import-meta-main.vn`).
- `int` 64 bits con `IntegerOverflow`, sin wrap
  (`tests/120-int-division.vn`); `int / int` = `int` truncado
  (`tests/120-int-division.vn`).
- La stdlib se carga siempre desde una imagen `std.vnb`: en un checkout se
  construye desde `std/` en la caché; en un binario de release va embebida
  (`VARN_STD=@embedded`). Ver `../MODULES_SYSTEM.md`.

## Closures

Capturan por upvalue del frame padre (`tests/07-closures.vn`); ver
`../VM_ARCHITECTURE.md` para el mecanismo.
