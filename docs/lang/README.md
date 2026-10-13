# Varn Language — índice (desde tests)

208 ficheros `tests/*.vn`; `tests/main.vn` importa 206; cierra con `summary()` (`main.vn:146,209`). Huecos: 39, 81, 91, 104–108. No importado directo: `75-async-shapes-lib.vn` (auxiliar).

| Doc | Cubre (citas) |
|---|---|
| `overview.md` | mapa suite + regla sin sintaxis no probada |
| `error_handling.md` | `try/catch/finally` `11-errors.vn:5-26,44-48`; `try` prefijo `109-try-operator.vn:18-19,35,44`; multi-catch `90-multi-catch.vn:23-30` |
| `statements.md` | `match` `10-match.vn:4-9` |
| `classes_objects.md` | clases `12-classes.vn:3-40` |
| `async_concurrency.md` | async `21-async.vn:4-38`; isolates `47-isolates-multithread.vn:2,10-39`; canales `54-channels.vn` |
| `generators.md` | generadores `20-generators.vn:3-34`; async-gen `77-async-generators.vn:22-26` |

Resto de `docs/lang/*.md`: vigente solo lo citado con `tests/*.vn:L-L`.
