# Varn Language — Sentencias y Control de Flujo

> Fuentes: `tests/09-control-flow.vn`, `tests/10-match.vn`, `tests/11-errors.vn`, `tests/18-ranges.vn`, `tests/34-char-type.vn`, `tests/47-isolates-multithread.vn`, `tests/54-channels.vn`, `tests/78-try-early-exit.vn`, `tests/79-control-flow-narrowing.vn`.

---

## 1. Declaración de Variables

```vn
let x = 42              // mutable
const y = "hello"       // inmutable
// `var` no existe: el compilador lo rechaza (VN2002)

let typed: int = 100
const explicit: str? = null
```

---

## 2. Asignación y Asignación Compuesta

```vn
x = x + 1          // asignación simple
a ??= 42            // null-coalescing assignment
cfg.timeout ??= 3000   // en propiedad de objeto
items[0] ??= "filled"  // en índice de array
```

---

## 3. Bloques

Un bloque `{ }` agrupa sentencias y crea un nuevo ámbito léxico:

```vn
{
    const local = 42   // solo visible dentro de este bloque
    let sum = 0
    for (let i = 0; i < 10; i = i + 1) { sum = sum + i }
}
```

---

## 4. Condicionales (`if / else`)

```vn
function classify(n: int): str {
    if (n < 0) return "neg"
    else if (n === 0) return "zero"
    else return "pos"
}
```

Con bloques:

```vn
if (val !== null) {
    return val.length     // type narrowed to str (tests/79-control-flow-narrowing.vn)
}
```

---

## 5. Match Expressions

```vn
function describeNum(n: int): str {
    return match (n) {        // tests/10-match.vn
        0 => "zero",
        1 => "one",
        2 | 3 => "two or three",   // multi-valor
        _ => "other"               // wildcard (default)
    }
}
```

Match sobre enum con payload (`tests/41-advanced-enums.vn`):

```vn
let v = match x {
    A(n) => n,       // desestructura el payload
    B(_) => -1,
}
```

Match anidado en `this` (`tests/41-advanced-enums.vn`, `tests/189-enum-variant-slots.vn`):

```vn
area(): float {
    return match (this) {
        Circle(radius)           => 3.14159 * radius * radius,
        Rectangle(width, height) => width * height
    }
}
```

---

## 6. Bucle `while`

```vn
let i = 0
while (i < arr.length) {
    if (arr[i] > threshold) { break }
    i = i + 1
}
// tests/09-control-flow.vn
```

---

## 7. Bucle `for` (estilo C)

```vn
for (let i = 0; i <= max; i = i + 1) {
    if (i % 2 === 0) continue
    total = total + i
}
// tests/09-control-flow.vn
```

---

## 8. Bucle `for...of`

Itera sobre arrays, rangos, generadores e iterables:

```vn
let forOfSum = 0
for (const n of [10, 20, 30]) {
    forOfSum = forOfSum + n
}
assertTrue("for-of sum", forOfSum === 60)   // tests/09-control-flow.vn

// Con rango inclusivo
let rangeSum = 0
for (const i of 1..=10) {
    rangeSum = rangeSum + i
}
assertTrue("range for-of sum", rangeSum === 55)   // tests/18-ranges.vn

// Con break
let forOfBreak = 0
for (const n of [1, 2, 3, 4, 5]) {
    if (n === 3) break
    forOfBreak = forOfBreak + n
}
assertTrue("for-of break", forOfBreak === 3)

// Con generador
for (let i = 0; i < 8; i = i + 1) {
    fibs.push(fib2.next().value)
}
```

---

## 9. Bucle `for...in`

Itera sobre las claves de un objeto:

```vn
const obj2 = { x: 1, y: 2, z: 3 }
let keyCount = 0
for (const k in obj2) {
    keyCount = keyCount + 1
}
assertTrue("for-in key count", keyCount === 3)   // tests/09-control-flow.vn
```

---

## 10. `for await` (Async Iteration)

Consume iterables asíncronos, como canales o async generators:

```vn
for await (const v of ch.rx) {         // tests/54-channels.vn
    sum = sum + v
}

for await (const msg of rx) {          // tests/47-isolates-multithread.vn
    match (msg) {
        Val(n) => await tx.send(n + 1),
        Msg.Exit => open = false,
    }
    if (!open) break
}

for await (const v of awaitsThenYields()) {   // tests/77-async-generators.vn
    total = total + v
}
```

---

## 11. `break` y `continue`

```vn
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

```vn
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

```vn
function withFinally(): int {
    try {
        return 42
    } finally {
        finallyRan = true    // se ejecuta aunque haya return
    }
}
assertTrue("finally ran", finallyRan)   // tests/11-errors.vn
```

---

## 13. Manejo de Errores (`try / catch / finally`)

```vn
function safeDivide(a: int, b: int): float {
    try {
        if (b === 0) throw new RangeError("division by zero")
        return (a as float) / (b as float)
    } catch (e) {
        return -1
    }
}
// tests/11-errors.vn

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

```vn
throw new Error("mensaje")
throw new TypeError("bad type")
throw new RangeError("too big")
```

### Catch con `instanceof`

```vn
try {
    throw new RangeError("too big")
} catch (e) {
    isRangeError = e instanceof RangeError
}
assertTrue("catch instanceof", isRangeError)   // tests/11-errors.vn
```

### Rethrow

```vn
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

```vn
function conBreak(): void {
    for (let i = 0; i < 3; i = i + 1) {
        try {
            if (i == 1) { break; }
        } catch (e) { /* nunca se llega aquí */ }
    }
}
// tests/78-try-early-exit.vn

function conReturn(): void {
    for (let i = 0; i < 3; i = i + 1) {
        try {
            if (i == 1) { return; }
        } catch (e) {}
    }
}
assertTrue("break no deja handler colgante", capturaEnElSitioCorrecto("break"))
assertTrue("return no deja handler colgante", capturaEnElSitioCorrecto("return"))
```

---

## 14. `throw`

```vn
throw new Error("mensaje")
throw new TypeError("bad type")
throw new ValidationError("email", "invalid format")
throw e    // re-lanzar una excepción capturada
```

---

## 15. `using` (Resource Management)

Gestión de recursos con disposición automática al salir del bloque:

```vn
using group = new TaskGroup<int>()   // tests/21-async.vn
const g1 = group.spawn(asyncAdd(7, 8))
const joined = await group.join()

// Bloque explícito con using
{
    using tx = ch.tx               // tests/54-channels.vn
    await tx.send(5)
}   // tx.dispose() se llama aquí automáticamente

// using con destructuring
{
    using { res1, res2 } = resource;   // tests/50-opt-unsupported-phase3.vn
}   // resource.dispose() corre al salir

// using con ficheros
using file = File.open(path, "w");   // tests/45-simple-file-test.vn
```

---

## 16. Operador Ternario

```vn
function classify(n: int): str {
    return n < 0 ? "neg" : n === 0 ? "zero" : "pos"
}
assertTrue("nested ternary neg",  classify(-1) === "neg")
assertTrue("nested ternary zero", classify(0) === "zero")
assertTrue("nested ternary pos",  classify(1) === "pos")
// tests/09-control-flow.vn
```

---

## 17. Sentencia de Expresión

Cualquier expresión puede usarse como sentencia (útil para efectos secundarios):

```vn
counter = counter + 1
[10, 20, 30].forEach((n) => { counter = counter + n })
IdGen.reset()
mut.push(4)
```

---

## 18. Expresión `assert` (función de test)

```vn
assertTrue("mensaje", condición_booleana)
// Lanza error si la condición es falsa. Presente en casi todos los tests.
assertTrue("int add", 1 + 2 === 3)
assertTrue("trim",    s.trim() === "Hello, World!")
```

---

## `if` y `while` con binding

La condición puede ser una declaración sobre un valor anulable. El cuerpo corre solo si el valor no es `null`, y el nombre ya llega estrechado a su tipo sin `null`:

```vn
if (const i = items.indexOf(x)) {
    print(i + 1)          // i: int
} else {
    print("no está")      // i no existe aquí
}

while (const next = queue.shift()) {
    handle(next)
}
```

Se acepta `const` o `let`, y cualquier patrón de destructuring (`if (const { a, b } = lookup())`). Es azúcar: `if (const v = e) A else B` equivale a `{ const t = e; if (t !== null) { const v = t; A } else B }`, y `while` equivale a un bucle que sale cuando `e` es `null`. El valor debe ser anulable; si no lo es, el compilador lo rechaza igual que `x !== null` sobre un `int`. Para `Option<T>` usa `match`.

Para recorrer con índice: `for (const [i, v] of xs.entries())`. `x in d` solo vale para diccionarios (`{ [k: str]: V }`) y `dynamic`; para rangos usa `.contains(x)`.

Test: `tests/318-binding-conditions-entries-and-optional-calls.vn`.

