# Try/Catch a JIT puro — estado (breaking, suite verde, NO 100%)

## Puro hoy (verificado con tier forzado y `debug -p clif` ROUTE)

- Sync y Async/generators con try/catch en JIT puro y reanudación OSR.
- Mecanismo único: `jit_error_flag` + `jit_native_result` + `take_handler` + dispatch/err_ret inline; cero `longjmp` para errores; cero OSR catch. `JIT_HELPERS_VERSION` 2.
- Resolución de async+try: en OSR resume (`osr.rs`), las variables `header.live` se cargan y definen antes de evaluar `jit_error_flag` y ramificar hacia `rej` (catch handler landing block). Esto garantiza que el bloque de aterrizaje del catch y sus sucesores tengan todas las variables vivas preservadas (incluyendo recursos para `dispose`/`disposeAsync`), eliminando la corrupción de receptores.
- Estado y error de JIT preservados en `Frozen` (`jit_error_flag` + `jit_native_result`) a través de freeze/thaw.
- Gate `(proto.is_async || proto.is_generator) && has_try` y veto OSR `has_try` completamente removidos.

## Gates que quedan (bail a intérprete)

1. `drive.rs`: try-en-loop. Al quitarlo, `conContinue` (78) da `EntryLivein`/stack overflow. Causa: dispatch compartido fuera del loop rompe single-entry de los análisis de loops. Seguir con loop opts desactivados rompió 179; revertido.
2. `drive.rs`: `imported` de 188 por nombre (parche reconocido). Sin él, 188 falla: segundo dispatch fantasma en catch de error nativo `Headers` sin `propagate` previo. El mismo código exacto vía `run` sí pasa puro; bajo framework `test` no. Requiere aislar la interacción framework/tasks.

## Validación

- `verify -Fast`: Matriz 4/4 completa superada (3188 passed, 0 failed en los 4 cuadrantes).
- `cargo fmt` y `cargo clippy` al día.
