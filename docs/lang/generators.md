# Varn Language — Generadores

> Fuentes: `tests/20-generators.vn`, `tests/76-generator-values.vn`, `tests/77-async-generators.vn`.

---

## 1. Funciones Generadoras (`function*`)

Una función marcada con `*` es un **generador**: cada vez que se llama a `.next()`, la ejecución avanza hasta el siguiente `yield` y se detiene, devolviendo `{ value, done }`.

```varn
function* range_gen(start: int, end: int) {
    let i = start
    while (i < end) {
        yield i
        i = i + 1
    }
}
// tests/20-generators.vn:2

const gen = range_gen(0, 5)
assert("gen value 0", gen.next().value === 0)
assert("gen done 0",  !gen.next().done)
gen.next()
gen.next()
const g3 = gen.next()
assert("gen value 3", g3.value === 3)
const gend = gen.next()
assert("gen done",    gend.done)   // value es null cuando done === true
```

---

## 2. Generadores Infinitos

```varn
function* fib_gen() {
    let a = 0
    let b = 1
    while (true) {
        yield a
        const tmp = a + b
        a = b
        b = tmp
    }
}
// tests/20-generators.vn:23

const fib2 = fib_gen()
const fibs: int[] = []
for (let i = 0; i < 8; i = i + 1) {
    fibs.push(fib2.next().value)
}
assert("fib gen [0]", fibs[0] === 0)
assert("fib gen [1]", fibs[1] === 1)
assert("fib gen [7]", fibs[7] === 13)
```

---

## 3. Valores Complejos en `yield`

Los generadores pueden hacer `yield` de cualquier tipo de heap: strings, arrays, objetos, otros generadores.

```varn
function* yieldsStrings() {
    yield "a string"
}
assert("yielded string", yieldsStrings().next().value === "a string")

function* yieldsArrays() {
    yield [1, 2, 3]
}
const arr = yieldsArrays().next().value
assert("yielded array", arr.length === 3 && arr[1] === 2)
// tests/76-generator-values.vn:49

function* yieldsObjects() {
    yield { a: 1, b: 2 }
}
const obj = yieldsObjects().next().value
assert("yielded object", obj.a === 1 && obj.b === 2)
```

---

## 4. Generadores que Acceden a Estado Global

```varn
let moduleCounter = 0
function bumpCounter(): int {
    moduleCounter = moduleCounter + 1
    return moduleCounter
}
function* touchesModuleState() {
    yield bumpCounter()
    yield bumpCounter()
}
const counted = touchesModuleState()
assert("gen sees module state, first",  counted.next().value === 1)
assert("gen sees module state, second", counted.next().value === 2)
// tests/76-generator-values.vn:29
```

---

## 5. Generadores que Hacen `return`

El valor del `return` final es accesible en el último `next()` con `done === true`:

```varn
function* returnsAHeapValue() {
    yield [1]
    return [9, 9]
}
const returning = returnsAHeapValue()
returning.next()
const finalResult = returning.next()
assert("returned heap value", finalResult.done && finalResult.value[0] === 9)
// tests/76-generator-values.vn:74
```

---

## 6. Generadores Anidados (Yield de Generadores)

```varn
function* inner() {
    yield 41
}
function* yieldsAGenerator() {
    yield inner()    // yield el generador como valor
}
const nested = yieldsAGenerator().next().value
assert("generator yields a generator", nested.next().value === 41)
// tests/76-generator-values.vn:61
```

---

## 7. Generadores en `for...of`

```varn
const gen = range_gen(0, 5)
// Consumo manual:
gen.next()   // { value: 0, done: false }
gen.next()   // { value: 1, done: false }

// Consumo con for...of
for (let i = 0; i < 8; i = i + 1) {
    fibs.push(fib2.next().value)
}
```

---

## 8. Async Generators (`async function*`)

Combinan `async`/`await` con `yield` para producir valores de forma asíncrona.

```varn
async function tenTimes(n: int): int {
    return n * 10
}

async function* awaitsThenYields() {
    yield await tenTimes(1)    // 10
    yield await tenTimes(2)    // 20
    yield await tenTimes(3)    // 30
}
// tests/77-async-generators.vn:20

const manual = awaitsThenYields()
assert("async gen first",  manual.next().value === 10)
assert("async gen second", manual.next().value === 20)
assert("async gen third",  manual.next().value === 30)
assert("async gen done",   manual.next().done)
```

### `await` entre yields

```varn
async function* awaitsBetweenYields() {
    const first = await tenTimes(4)    // 40
    yield first
    const second = await tenTimes(first - 35)  // tenTimes(5) = 50
    yield second
}

const between = awaitsBetweenYields()
assert("await before first yield", between.next().value === 40)
assert("await between yields",     between.next().value === 50)
```

### Heap values desde async generators

```varn
async function* yieldsHeapValues() {
    yield [await tenTimes(1), await tenTimes(2)]   // [10, 20]
}
const heaps = yieldsHeapValues().next().value
assert("async gen heap value", heaps.length === 2 && heaps[1] === 20)
```

### `for await` sobre async generators

```varn
let total = 0
for await (const v of awaitsThenYields()) {
    total = total + v
}
assert("for await sums", total === 60)

// También funciona con for...of normal (valores ya settled)
let plain = 0
for (const v of awaitsThenYields()) {
    plain = plain + v
}
assert("plain for over async gen", plain === 60)
```

### Manejo de errores dentro de async generators

```varn
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

### `await` de suspensiones reales

```varn
async function* awaitsSleep() {
    await sleep(1)
    yield 7
}
assert("async gen awaits sleep", awaitsSleep().next().value === 7)
```

---

## 9. Interfaz del Generador

| Propiedad/Método | Tipo | Descripción |
|-----------------|------|-------------|
| `.next()` | `{ value: T, done: bool }` | Avanza al siguiente `yield`. |
| `.next().value` | `T` | El valor del `yield` actual o del `return` final. |
| `.next().done` | `bool` | `true` cuando la función generadora ha terminado. |

Para **async generators**: `.next()` puede ser llamado normalmente; `await gen.next()` también es válido.
