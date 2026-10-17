# Varn — Overview (desde tests)

Fuente única: `tests/*.vn`. Código manda.

## Suite

- 220 archivos `tests/*.vn`; `tests/main.vn` importa 218.
- No importados: `main.vn` y `75-async-shapes-lib.vn` (auxiliar de `75-async-shapes.vn`).
- Cierre: `import { summary } from "std:test"` + `summary()` al final de `main.vn`.
- Huecos de numeración: 39, 81, 91, 104–108.

## Rasgos probados (cita mínima)

- `try/catch/finally`: `tests/11-errors.vn` (safeDivide, withFinally), `44-48` (catch TypeError, `e.name`).
- `try` prefijo (early-return sobre Result/Option/nullable): `tests/109-try-operator.vn` (Result), `:35` (Option), `:44` (nullable `str?`).
- Multi-catch tipado + unión: `tests/90-multi-catch.vn` (handler `catch (e: TypeError)`, `RangeError`, network, database).
- `match`: `tests/10-match.vn` (literal, `|`, `_`).
- Clases: `tests/12-classes.vn` (static, getter/setter, herencia, `super`, `override`, abstract).
- Async: `tests/21-async.vn` (`async/await`, `spawn`, `parallel`).
- Generadores: `tests/20-generators.vn` (`function*`, `yield`, `.next()` `{value,done}`).
- Async-generadores: `tests/77-async-generators.vn` (`async function*`, `yield await`).
- Isolates + canales: `tests/47-isolates-multithread.vn` (imports `spawnIsolate,channel,Sender,Receiver,ChannelClosed`), `:10-39` (workers), más `tests/54-channels.vn` (close, `ChannelClosed` en send/receive).

## Regla

Sin sintaxis no probada: lo no citado arriba ni en `docs/lang/*.md` con su `tests/*.vn` no es contrato.
