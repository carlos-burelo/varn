# Varn — Overview (desde tests)

Fuente única: `tests/*.vn`. Código manda.

## Suite

- 208 ficheros `tests/*.vn`. `tests/main.vn` importa 206 (`main.vn:1-145,147-208`).
- No importado directo: `75-async-shapes-lib.vn` (lib auxiliar de `75-async-shapes.vn`), más `main.vn` mismo.
- Cierre: `import { summary } from "std:test"` (`main.vn:146`), `summary()` (`main.vn:209`).
- Huecos de numeración que no existen: 39, 81, 91, 104–108.

## Rasgos probados (cita mínima)

- `try/catch/finally`: `tests/11-errors.vn:5-26` (safeDivide, withFinally), `44-48` (catch TypeError, `e.name`).
- `try` prefijo (early-return sobre Result/Option/nullable): `tests/109-try-operator.vn:18-19` (Result), `:35` (Option), `:44` (nullable `str?`).
- Multi-catch tipado + unión: `tests/90-multi-catch.vn:23-30` (handler `catch (e: TypeError)`, `RangeError`, network, database).
- `match`: `tests/10-match.vn:4-9` (literal, `|`, `_`).
- Clases: `tests/12-classes.vn:3-40` (static, getter/setter, herencia, `super`, `override`, abstract).
- Async: `tests/21-async.vn:4-38` (`async/await`, `spawn`, `parallel`).
- Generadores: `tests/20-generators.vn:3-34` (`function*`, `yield`, `.next()` `{value,done}`).
- Async-generadores: `tests/77-async-generators.vn:22-26` (`async function*`, `yield await`).
- Isolates + canales: `tests/47-isolates-multithread.vn:2` (imports `spawnIsolate,channel,Sender,Receiver,ChannelClosed`), `:10-39` (workers), más `tests/54-channels.vn` (close, `ChannelClosed` en send/receive).

## Regla

Sin sintaxis no probada: lo no citado arriba y en `docs/lang/*.md` con cita `tests/*.vn:L-L` no es contrato.
