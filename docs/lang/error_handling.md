# Varn Language — Manejo de Errores

> Fuentes: `tests/11-errors.vn`, `tests/21-async.vn`, `tests/47-isolates-multithread.vn`, `tests/54-channels.vn`, `tests/77-async-generators.vn`, `tests/78-try-early-exit.vn`.

---

## 1. `throw`

Lanza cualquier valor de tipo `Error` o subclase:

```varn
throw new Error("mensaje genérico")
throw new TypeError("tipo incorrecto")
throw new RangeError("fuera de rango")
throw new ValidationError("email", "invalid format")   // clase personalizada
throw e    // re-lanzar un error capturado
```

---

## 2. `try / catch`

```varn
function safeDivide(a: int, b: int): float {
    try {
        if (b === 0) throw new RangeError("division by zero")
        return a / b
    } catch (e) {
        return -1
    }
}
assert("try no error",      safeDivide(10, 2) === 5)
assert("try catch returns", safeDivide(10, 0) === -1)
// tests/11-errors.vn:2
```

---

## 3. `try / catch / finally`

`finally` siempre se ejecuta, incluso cuando hay `return`:

```varn
let finallyRan = false
function withFinally(): int {
    try {
        return 42
    } finally {
        finallyRan = true   // se ejecuta aunque haya return
    }
}
const fres = withFinally()
assert("finally return value", fres === 42)
assert("finally ran",          finallyRan)
// tests/11-errors.vn:66
```

`finally` con `break`:

```varn
function conBreakYFinally(): int {
    let pasos = 0
    for (let i = 0; i < 3; i = i + 1) {
        try {
            if (i == 1) { break }
        } catch (e) {
        } finally {
            pasos = pasos + 1    // se ejecuta en cada iteración antes del break
        }
    }
    return pasos
}
assert("try/catch/finally con break ejecuta sus finally", conBreakYFinally() === 2)
// tests/78-try-early-exit.vn:75
```

---

## 4. Tipos de Error Incorporados

| Clase | Descripción |
|-------|-------------|
| `Error` | Clase base de todos los errores. |
| `TypeError` | Error de tipo incorrecto. |
| `RangeError` | Error de valor fuera de rango. |

Propiedades comunes:

```varn
e.name       // nombre del error (str)
e.message    // mensaje (str)
```

---

## 5. Errores Personalizados

```varn
class ValidationError extends Error {    // tests/11-errors.vn:44
    field: str
    constructor(field: str, msg: str) {
        super(msg)
        this.name = "ValidationError"
        this.field = field
    }
}

let ve: ValidationError | null = null
try {
    throw new ValidationError("email", "invalid format")
} catch (e) {
    if (e instanceof ValidationError) {
        ve = e
    }
}
assert("custom error name",    ve?.name === "ValidationError")
assert("custom error message", ve?.message === "invalid format")
assert("custom error field",   ve?.field === "email")
assert("instanceof Error",     ve instanceof Error)
```

---

## 6. `instanceof` en Catch

```varn
let isRangeError = false
try {
    throw new RangeError("too big")
} catch (e) {
    isRangeError = e instanceof RangeError
}
assert("catch instanceof", isRangeError)   // tests/11-errors.vn:27
```

---

## 7. Try/Catch Anidado y Re-lanzar

```varn
let innerCaught = false
let outerCaught = false
try {
    try {
        throw new Error("inner")
    } catch (e) {
        innerCaught = true
        throw e    // re-lanza al catch exterior
    }
} catch (e) {
    outerCaught = true
}
assert("nested try inner caught", innerCaught)
assert("nested try rethrow",      outerCaught)
// tests/11-errors.vn:29
```

---

## 8. Salidas Tempranas desde `try/catch`

Salir de un `try/catch` con `break`, `continue` o `return` cierra correctamente la región guardada. No quedan handlers colgantes:

```varn
function lanzaTrasSalir(via: str): void {
    if (via === "break") { conBreak() }       // break dentro de try/catch en loop
    if (via === "continue") { conContinue() } // continue
    if (via === "return") { conReturn() }     // return
    throw new Error("debe llegar al catch del llamante")
}

function capturaEnElSitioCorrecto(via: str): bool {
    let llego = false
    try {
        lanzaTrasSalir(via)
    } catch (e) {
        llego = true
    }
    return llego
}

assert("break no handler colgante",    capturaEnElSitioCorrecto("break"))
assert("continue no handler colgante", capturaEnElSitioCorrecto("continue"))
assert("return no handler colgante",   capturaEnElSitioCorrecto("return"))
// tests/78-try-early-exit.vn:69
```

---

## 9. Catch Vacío con Early Exit (no panics el compilador)

```varn
function vacioConBreak(): int {
    let pasos = 0
    for (let i = 0; i < 3; i = i + 1) {
        try {
            pasos = pasos + 1
            if (i == 1) { break }
        } catch (e) { }   // catch vacío: válido
    }
    return pasos
}
assert("catch vacío + break no falla", vacioConBreak() === 2)

function retornaFueraDeBucle(): int {
    try {
        return 42
    } catch (e) { }
    return -1
}
assert("catch vacío + return fuera de bucle", retornaFueraDeBucle() === 42)
// tests/78-try-early-exit.vn:119
```

---

## 10. Errores en Contextos Asíncronos

```varn
async function asyncError(): str {
    try {
        throw new Error("async fail")
    } catch (e) {
        return "caught:" + e.message
    }
}
assert("async catch", await asyncError() === "caught:async fail")
// tests/21-async.vn:15
```

### Grupo de tareas con errores

```varn
using failGroup = TaskGroup<int>()
failGroup.spawn(asyncAdd(2, 3))
failGroup.spawn(asyncBoom())      // lanza "boom"
try {
    await failGroup.join()
    assert("group join should fail", false)
} catch (e) {
    assert("group join rejects", true)
}
// tests/21-async.vn:67
```

---

## 11. `ChannelClosed` como Error

```varn
import { ChannelClosed } from "std:task"

const ch = channel<str>(1)
ch.tx.close()
try {
    await ch.rx.receive()
} catch (e) {
    assert("recv closed", e instanceof ChannelClosed)
}
try {
    await ch.tx.send("nope")
} catch (e) {
    assert("send closed", e instanceof ChannelClosed)
}
// tests/54-channels.vn:11
```

---

## 12. Errores en Async Generators

```varn
async function* throwsAfterAYield() {
    yield 1
    throw new Error("boom")
}
const throwing = throwsAfterAYield()
assert("yield before throw", throwing.next().value === 1)
// tests/77-async-generators.vn:38

// Captura dentro del generador
async function* catchesItsOwnRejection() {
    try {
        await failing()
        yield -1
    } catch (e) {
        yield 99
    }
}
assert("async gen catches rejection", catchesItsOwnRejection().next().value === 99)
```
