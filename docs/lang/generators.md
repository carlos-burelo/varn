# Varn Language — Generadores

> Fuentes: `tests/20-generators.vn`, `tests/76-generator-values.vn`, `tests/77-async-generators.vn`.

---

## 1. Funciones Generadoras (`function*`)

Una función marcada con `*` es un **generador**: cada vez que se llama a `.next()`, la ejecución avanza hasta el siguiente `yield` y se detiene, devolviendo `{ value, done }`.

```vn
function* range_gen(start: int, end: int) {
    let i = start
    while (i < end) {
        yield i
        i = i + 1
    }
}
// tests/20-generators.vn

const gen = range_gen(0, 5)
assertTrue("gen value 0", gen.next().value === 0)
assertTrue("gen done 0",  !gen.next().done)
gen.next()
gen.next()
const g3 = gen.next()
assertTrue("gen value 3", g3.value === 3)
const gend = gen.next()
assertTrue("gen done",    gend.done)   // value es null cuando done === true
```

---

## 2. Generadores Infinitos

```vn
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
// tests/20-generators.vn

const fib2 = fib_gen()
const fibs: int[] = []
for (let i = 0; i < 8; i = i + 1) {
    fibs.push(fib2.next().value)
}
assertTrue("fib gen [0]", fibs[0] === 0)
assertTrue("fib gen [1]", fibs[1] === 1)
assertTrue("fib gen [7]", fibs[7] === 13)
```

---

## 3. Valores Complejos en `yield`

Los generadores pueden hacer `yield` de cualquier tipo de heap: strings, arrays, objetos, otros generadores.

```vn
function* yieldsStrings() {
    yield "a string"
}
assertTrue("yielded string", yieldsStrings().next().value === "a string")

function* yieldsArrays() {
    yield [1, 2, 3]
}
const arr = yieldsArrays().next().value
assertTrue("yielded array", arr.length === 3 && arr[1] === 2)
// tests/76-generator-values.vn

function* yieldsObjects() {
    yield { a: 1, b: 2 }
}
const obj = yieldsObjects().next().value
assertTrue("yielded object", obj.a === 1 && obj.b === 2)
```

---

## 4. Generadores que Acceden a Estado Global

```vn
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
assertTrue("gen sees module state, first",  counted.next().value === 1)
assertTrue("gen sees module state, second", counted.next().value === 2)
// tests/76-generator-values.vn
```

---

## 5. Generadores que Hacen `return`

El valor del `return` final es accesible en el último `next()` con `done === true`:

```vn
function* returnsAHeapValue() {
    yield [1]
    return [9, 9]
}
const returning = returnsAHeapValue()
returning.next()
const finalResult = returning.next()
assertTrue("returned heap value", finalResult.done && finalResult.value[0] === 9)
// tests/76-generator-values.vn
```

---

## 6. Generadores Anidados (Yield de Generadores)

```vn
function* inner() {
    yield 41
}
function* yieldsAGenerator() {
    yield inner()    // yield el generador como valor
}
const nested = yieldsAGenerator().next().value
assertTrue("generator yields a generator", nested.next().value === 41)
// tests/76-generator-values.vn
```

---

## 7. Generadores en `for...of`

```vn
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

```vn
async function tenTimes(n: int): int {
    return n * 10
}

async function* awaitsThenYields() {
    yield await tenTimes(1)    // 10
    yield await tenTimes(2)    // 20
    yield await tenTimes(3)    // 30
}
// tests/77-async-generators.vn

const manual = awaitsThenYields()
assertTrue("async gen first",  manual.next().value === 10)
assertTrue("async gen second", manual.next().value === 20)
assertTrue("async gen third",  manual.next().value === 30)
assertTrue("async gen done",   manual.next().done)
```

### `await` entre yields

```vn
async function* awaitsBetweenYields() {
    const first = await tenTimes(4)    // 40
    yield first
    const second = await tenTimes(first - 35)  // tenTimes(5) = 50
    yield second
}

const between = awaitsBetweenYields()
assertTrue("await before first yield", between.next().value === 40)
assertTrue("await between yields",     between.next().value === 50)
```

### Heap values desde async generators

```vn
async function* yieldsHeapValues() {
    yield [await tenTimes(1), await tenTimes(2)]   // [10, 20]
}
const heaps = yieldsHeapValues().next().value
assertTrue("async gen heap value", heaps.length === 2 && heaps[1] === 20)
```

### `for await` sobre async generators

```vn
let total = 0
for await (const v of awaitsThenYields()) {
    total = total + v
}
assertTrue("for await sums", total === 60)

// También funciona con for...of normal (valores ya settled)
let plain = 0
for (const v of awaitsThenYields()) {
    plain = plain + v
}
assertTrue("plain for over async gen", plain === 60)
```

### Manejo de errores dentro de async generators

```vn
async function* catchesItsOwnRejection() {
    try {
        await failing()
        yield -1
    } catch (e) {
        yield 99
    }
}
assertTrue("async gen catches rejection", catchesItsOwnRejection().next().value === 99)
```

### `await` de suspensiones reales

```vn
async function* awaitsSleep() {
    await sleep(1)
    yield 7
}
assertTrue("async gen awaits sleep", awaitsSleep().next().value === 7)
```

---

## 9. Interfaz del Generador

| Propiedad/Método | Tipo | Descripción |
|-----------------|------|-------------|
| `.next()` | `T?` | Avanza al siguiente `yield`; devuelve el valor cedido, o `null` cuando el generador terminó. |
| `.close()` | `void` | Cierra un generador suspendido y ejecuta sus `finally`. |

Un generador no puede ceder `null` (marca el final). Para consumirlo sin comparar: `while (const v = gen.next()) { ... }` o `for (const v of gen)`.

Para **async generators**: `.next()` devuelve `Task<T?>` (`await gen.next()`), y `.close()` devuelve `Task<void>`.

---

## 10. `yield` como sentencia, `yield*` y cierre

`yield` no produce valor (`next()` no envía nada al generador), así que solo se acepta como sentencia: `const x = yield 1` es un error de sintaxis.

`yield* fuente;` delega: equivale a `for (const x of fuente) { yield x; }` y acepta cualquier cosa que `for...of` acepte.

Un generador suspendido dentro de un `try/finally` ejecuta sus `finally` cuando se cierra:

- `gen.close()` reanuda el generador con la señal de cierre; corren todos los `finally` que lo rodean, de dentro hacia fuera, y los errores que lancen llegan a quien llamó a `close()`.
- `for...of` y `for await` sobre un generador lo cierran al salir por `break`, `return` o `throw`.
- En un `async function*`, `close()` devuelve una `Task`: `await gen.close()`.

Los tests son `tests/317-generator-close-runs-finally.vn` y los fixtures `tests/errors/yield-*.vn`.

