# Varn Language — Sentencias y Control de Flujo

> Fuentes: `tests/09-control-flow.vn`, `tests/10-match.vn`, `tests/11-errors.vn`, `tests/18-ranges.vn`, `tests/34-char-type.vn`, `tests/47-isolates-multithread.vn`, `tests/54-channels.vn`, `tests/78-try-early-exit.vn`, `tests/79-control-flow-narrowing.vn`.

---

## 1. Declaración de Variables

```varn
let x = 42              // mutable
const y = "hello"       // inmutable
var z = true            // mutable (alias var)

let typed: int = 100
const explicit: str? = null
```

---

## 2. Asignación y Asignación Compuesta

```varn
x = x + 1          // asignación simple
a ??= 42            // null-coalescing assignment
cfg.timeout ??= 3000   // en propiedad de objeto
items[0] ??= "filled"  // en índice de array
```

---

## 3. Bloques

Un bloque `{ }` agrupa sentencias y crea un nuevo ámbito léxico:

```varn
{
    const local = 42   // solo visible dentro de este bloque
    let sum = 0
    for (let i = 0; i < 10; i = i + 1) { sum = sum + i }
}
```

---

## 4. Condicionales (`if / else`)

```varn
function classify(n: int): str {
    if (n < 0) return "neg"
    else if (n === 0) return "zero"
    else return "pos"
}
```

Con bloques:

```varn
if (val !== null) {
    return val.length     // type narrowed to str (tests/79-control-flow-narrowing.vn:5)
}
```

---

## 5. Match Expressions

```varn
function describeNum(n: int): str {
    return match (n) {        // tests/10-match.vn:3
        0 => "zero",
        1 => "one",
        2 | 3 => "two or three",   // multi-valor
        _ => "other"               // wildcard (default)
    }
}
```

Match sobre enum con payload:

```varn
let v = match x {
    A(n) => n,       // desestructura el payload
    B(_) => -1,
}
```

Match anidado en `this`:

```varn
area(): float {
    return match (this) {
        Circle(radius)           => 3.14159 * radius * radius,
        Rectangle(width, height) => width * height
    }
}
```

---

## 6. Bucle `while`

```varn
let i = 0
while (i < arr.length) {
    if (arr[i] > threshold) { break }
    i = i + 1
}
// tests/09-control-flow.vn:12
```

---

## 7. Bucle `for` (estilo C)

```varn
for (let i = 0; i <= max; i = i + 1) {
    if (i % 2 === 0) continue
    total = total + i
}
// tests/09-control-flow.vn:25
```

---

## 8. Bucle `for...of`

Itera sobre arrays, rangos, generadores e iterables:

```varn
let forOfSum = 0
for (const n of [10, 20, 30]) {
    forOfSum = forOfSum + n
}
assert("for-of sum", forOfSum === 60)   // tests/09-control-flow.vn:34

// Con rango inclusivo
let rangeSum = 0
for (const i of 1..=10) {
    rangeSum = rangeSum + i
}
assert("range for-of sum", rangeSum === 55)   // tests/18-ranges.vn:23

// Con break
let forOfBreak = 0
for (const n of [1, 2, 3, 4, 5]) {
    if (n === 3) break
    forOfBreak = forOfBreak + n
}
assert("for-of break", forOfBreak === 3)

// Con generador
for (let i = 0; i < 8; i = i + 1) {
    fibs.push(fib2.next().value)
}
```

---

## 9. Bucle `for...in`

Itera sobre las claves de un objeto:

```varn
const obj2 = { x: 1, y: 2, z: 3 }
let keyCount = 0
for (const k in obj2) {
    keyCount = keyCount + 1
}
assert("for-in key count", keyCount === 3)   // tests/09-control-flow.vn:48
```

---

## 10. `for await` (Async Iteration)

Consume iterables asíncronos, como canales o async generators:

```varn
for await (const v of ch.rx) {         // tests/54-channels.vn:36
    sum = sum + v
}

for await (const msg of rx) {          // tests/47-isolates-multithread.vn:27
    match (msg) {
        Val(n) => await tx.send(n + 1),
        Msg.Exit => open = false,
    }
    if (!open) break
}

for await (const v of awaitsThenYields()) {   // tests/77-async-generators.vn:71
    total = total + v
}
```

---

## 11. `break` y `continue`

```varn
while (i < arr.length) {
    if (arr[i] > threshold) { break }   // sale del bucle
    i = i + 1
}

for (let i = 0; i <= max; i = i + 1) {
    if (i % 2 === 0) continue    // salta a la siguiente iteración
    total = total + i
}
```

---

## 12. `return`

```varn
function firstAbove(arr: int[], threshold: int): int {
    let i = 0
    while (i < arr.length) {
        if (arr[i] > threshold) return arr[i]   // salida temprana
        i = i + 1
    }
    return -1
}
```

`return` dentro de `try/catch` ejecuta correctamente el `finally`:

```varn
function withFinally(): int {
    try {
        return 42
    } finally {
        finallyRan = true    // se ejecuta aunque haya return
    }
}
assert("finally ran", finallyRan)   // tests/11-errors.vn:75
```

---

## 13. Manejo de Errores (`try / catch / finally`)

```varn
function safeDivide(a: int, b: int): float {
    try {
        if (b === 0) throw new RangeError("division by zero")
        return a / b
    } catch (e) {
        return -1
    }
}
// tests/11-errors.vn:2

// finally siempre se ejecuta
try {
    // código que puede fallar
} catch (e) {
    // manejo del error
} finally {
    // siempre se ejecuta
}
```

### Tipos de error incorporados

```varn
throw new Error("mensaje")
throw new TypeError("bad type")
throw new RangeError("too big")
```

### Catch con `instanceof`

```varn
try {
    throw new RangeError("too big")
} catch (e) {
    isRangeError = e instanceof RangeError
}
assert("catch instanceof", isRangeError)   // tests/11-errors.vn:27
```

### Rethrow

```varn
try {
    try {
        throw new Error("inner")
    } catch (e) {
        innerCaught = true
        throw e    // re-lanza
    }
} catch (e) {
    outerCaught = true
}
```

### Salidas tempranas desde `try` (`break`, `continue`, `return`)

Salir de un `try/catch` por `break`, `continue` o `return` cierra correctamente la región guardada:

```varn
function conBreak(): void {
    for (let i = 0; i < 3; i = i + 1) {
        try {
            if (i == 1) { break; }
        } catch (e) { /* nunca se llega aquí */ }
    }
}
// tests/78-try-early-exit.vn:19

function conReturn(): void {
    for (let i = 0; i < 3; i = i + 1) {
        try {
            if (i == 1) { return; }
        } catch (e) {}
    }
}
assert("break no deja handler colgante", capturaEnElSitioCorrecto("break"))
assert("return no deja handler colgante", capturaEnElSitioCorrecto("return"))
```

---

## 14. `throw`

```varn
throw new Error("mensaje")
throw new TypeError("bad type")
throw new ValidationError("email", "invalid format")
throw e    // re-lanzar una excepción capturada
```

---

## 15. `using` (Resource Management)

Gestión de recursos con disposición automática al salir del bloque:

```varn
using group = TaskGroup<int>()       // tests/21-async.vn:57
const g1 = group.spawn(asyncAdd(7, 8))
const joined = await group.join()

// Bloque explícito con using
{
    using tx = ch.tx               // tests/54-channels.vn:45
    await tx.send(5)
}   // tx.dispose() se llama aquí automáticamente
```

---

## 16. Operador Ternario

```varn
function classify(n: int): str {
    return n < 0 ? "neg" : n === 0 ? "zero" : "pos"
}
assert("nested ternary neg",  classify(-1) === "neg")
assert("nested ternary zero", classify(0) === "zero")
assert("nested ternary pos",  classify(1) === "pos")
// tests/09-control-flow.vn:3
```

---

## 17. Sentencia de Expresión

Cualquier expresión puede usarse como sentencia (útil para efectos secundarios):

```varn
counter = counter + 1
[10, 20, 30].forEach((n) => { counter = counter + n })
IdGen.reset()
mut.push(4)
```

---

## 18. Expresión `assert` (función de test)

```varn
assert("mensaje", condición_booleana)
// Lanza error si la condición es falsa. Presente en casi todos los tests.
assert("int add", 1 + 2 === 3)
assert("trim",    s.trim() === "Hello, World!")
```
