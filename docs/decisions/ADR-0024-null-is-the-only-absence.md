# ADR-0024 — null es la única ausencia

## Decisión

Varn tiene un solo valor ausente: `null`, con tipo `T?`. `Option<T>` (`Some`/`None`, `isSome`, `unwrap`, `unwrapOr`) se elimina del núcleo. `Result<T, E>` se queda: modela un fallo con causa, no una ausencia.

## Por qué

Dos formas de "puede faltar" obligan a recordar cuál usa cada API (`pop(): T?` frente a `Generator.next(): Option<T>`), a convertir entre ellas y a duplicar azúcar y reglas del checker. Es el mismo criterio que unificó `undefined` y `null`.

## Qué cambia

- El protocolo de iteración es `next(): T?`: `null` marca el final. `Generator<T>`, `Iterator<T>`, `AsyncGenerator<T>` (`Task<T?>`), `Receiver<T>.next()` (`TaskHandle<T?>`) lo siguen.
- Un generador no puede ceder `null`: el checker rechaza `yield null`, `yield;` y valores anulables, y la VM lanza un error si un valor dinámico resulta ser `null` en un `yield`. Así `null` no se confunde con el final.
- `for-of` sobre un tipo con `next(): T?` baja a un bucle que sale al ver `null`; el elemento es `T`.
- `if (const v = e)` y `while (const v = e)` (azúcar del parser) sustituyen a `match Some(v)`.
- El operador `try` sigue aceptando `Result` y tipos anulables.

## Qué se borra

`core:types/option`, `CoreSum::Option`, `NativeCtx::option`, `ExecCtx::make_option`, las etiquetas de variante en el `for-of` y los usos en `std/signals`.

## Verificación

`tests/319-null-is-the-only-absence.vn`, los fixtures `tests/errors/generator-cannot-yield-null.vn`, `yield-needs-a-value.vn` y `option-no-longer-exists.vn`, y la migración de los tests de generadores, `try` y señales. Matriz de `CONTRIBUTING.md` al cierre.

## Consecuencia aceptada

Un canal o un generador no puede transportar `null` como dato si se consume con `next()`: `null` significa "terminó". `receive()` no tiene esa limitación.
