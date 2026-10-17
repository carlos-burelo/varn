# Varn — Manejo de errores (desde tests)

## 1. `throw` + `try/catch/finally`

```varn
function safeDivide(a: int, b: int): float {
    try {
        if (b === 0) throw new RangeError("division by zero");
        return (a as float) / (b as float);
    } catch (e) {
        return -1;
    }
}
function withFinally(): int {
    try {
        return 42;
    } finally {
        finallyRan = true;
    }
}
```

`tests/11-errors.vn`. `finally` corre aunque haya `return`. Captura básica:

```varn
try {
    throw new TypeError("bad type");
} catch (e) {
    caughtName = e.name;
}
```

`tests/11-errors.vn`. Re-lanzar con `throw e`, errores custom con `extends Error`, `instanceof` en catch: mismo fichero, resto del file.

## 2. `try` prefijo (operador, no bloque)

```varn
let x: int = try parse_positive(a);
let y: int = try parse_positive(b);
return Result.Ok(x + y);
```

`tests/109-try-operator.vn`. Sobre `Option`: `:35`. Sobre nullable: `let non_null_str: str = try s;` (`:44`). Early-return del `Err`/`None`/`null` al llamante.

## 3. Multi-catch tipado

```varn
} catch (e: TypeError) {
    return `caught TypeError: ${e.message}`;
} catch (e: RangeError) {
    return `caught RangeError: ${e.message}`;
} catch (e: CustomNetworkError) {
```

`tests/90-multi-catch.vn` (sigue con `CustomDatabaseError`, `Error` general). Unión en un catch: `catch (e: TypeError | RangeError)`. Sin match de tipo, propaga al `try` exterior. `catch {` sin parámetro válido.

## 4. Errores async / canales / async-gen

- Async: `try/catch` dentro de `async function` (`tests/21-async.vn`).
- Canales: `send`/`receive` sobre canal cerrado rechazan con `ChannelClosed` (`tests/54-channels.vn`, `e instanceof ChannelClosed`).
- Isolates: errores de `spawnIsolate` (closure anónima, fn no exportada) llegan como rechazo con `message` (`tests/47-isolates-multithread.vn`).
- Async-gen: `try/catch` captura `await` rechazado dentro del cuerpo (`tests/77-async-generators.vn`); `throw` tras `yield` no capturable en `next()` (comentario `:99-102`).

Sin más sintaxis de errores que la citada.
