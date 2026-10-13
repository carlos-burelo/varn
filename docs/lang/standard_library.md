# Varn — Biblioteca estándar (puntero)

> Precisión parcial. Verificado contra tests/... Código manda.

Mapa completo de paquetes: `../STDLIB_ARCHITECTURE.md` (27 dirs en `std/`).
Este archivo no duplica esa lista; solo ancla lo ejercitado por la suite.

## Núcleo async y tareas (`std:task`)

`async/await`, `spawn`, `parallel`, `TaskGroup`, canales, isolates:
`async_concurrency.md`. Tests: `tests/21-async.vn:1-2,25-80`,
`tests/97-stdlib-task-combinators.vn:1,9-33` (`Task.all`, `allSettled`, `race`),
`tests/110-stdlib-consolidation.vn:3` (`Mutex`, `Semaphore`, `WaitGroup`),
`tests/47-isolates-multithread.vn`, `tests/54-channels.vn`.

## Colecciones y tipos de plataforma

- `Array` (`map/filter/reduce/find/push/pop/sort/slice/join/spread`):
  `tests/05-arrays.vn`, `tests/88-range-indexing.vn`.
- `Map` / `Set`: `tests/17-map-set.vn`.
- `Range` (`..`, `..=`, `Range.from`, `step`, `toArray`, slicing):
  `tests/18-ranges.vn`, `tests/88-range-indexing.vn`.
- Tuplas `#[…]` / records `#{…}`: `tests/69-tuples-records.vn`.
- `str`: `tests/03-strings.vn:7-58` (`trim`, `padStart/End`, `charCode`,
  `substr`, `isEmpty/isBlank/isDigit`, `capitalize`, `str.fromCharCode`, `str.join`).
- `char`: `tests/34-char-type.vn:3` (`charCodeAt`, `char.fromCode`, `toString`).
- `int` API (`wrapping/saturating/checked*`, `div/floorDiv/ceilDiv/rem/mod`,
  `parse/tryParse`): `tests/123-int-api.vn`.
- `decimal` / `bigint`: `tests/35-decimal-bigint.vn`,
  `tests/124-bigint-arbitrary.vn`, `tests/122-numeric-conversions.vn`.
- `Bytes` + streams: `tests/112-canonical-bytes-and-streams.vn`.
- Rangos genéricos, tipos literales, intersecciones, capacidades y operadores:
  `tests/130-range-generic.vn`, `tests/127-literal-types.vn`,
  `tests/118-intersections.vn`, `tests/132-capabilities.vn`,
  `tests/133-operator-capabilities.vn`.

## Formatos, red, sistema

- `std:csv`, `std:json`: `tests/82-std-csv.vn:80`.
- `std:regex`, `std:crypto` (UUID), `std:time`:
  `tests/83-canonical-types-regex-uuid-time.vn:59,75`.
- `std:process`, `std:compress`, `std:cli`, `std:env`, `std:path`:
  `tests/84-stdlib-process-compress-cli-env.vn:11,36,48,64`.
- `std:sqlite`, `std:ws`, archivos: `tests/85-stdlib-sqlite-ws-archive.vn:72,92`.
- Tagged templates + SQL (`db.sql`, `db.sqlOne`): `tests/86-tagged-templates.vn:6,48-61`.
- `std:encoding`, `std:cli` (Color/Table/Prompt), `std:log`, `std:collections`
  (PriorityQueue, LRUCache): `tests/110-stdlib-consolidation.vn:3-5,75-181`.
- `std:net`, `std:http`, `std:ws`:
  `tests/111-network-apis-overhaul.vn:1-3,25-77`.
- Migración y superficie completa: `tests/31-stdlib-migration-test.vn`,
  `tests/42-stdlib-comprehensive-test.vn`.

## Test y reflexión

- `std:test` (`describe/it/expect`): `tests/21-async.vn:1`,
  `tests/97-stdlib-task-combinators.vn:2`; framework: `tests/98-stdlib-test-framework.vn`.
- `std:reflect` (`MetaKey`, `ClassRef`; contextos de decorador globales):
  `tests/27-decorators.vn:1`.
- `std:fs` (`read/write/remove/exists`), `std:sys` (`env/platform/cwd`):
  `tests/80-capabilities-security.vn:6,42`.
- Ficheros vía `using`: `tests/44-implicit-extensions-test.vn:12-40`,
  `tests/45-simple-file-test.vn:8-14`.

Métodos no citados arriba: NO-VERIFICADOS. Ver `../STDLIB_ARCHITECTURE.md`.
