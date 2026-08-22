# Varn Language — Funciones y Métodos

> Fuentes: `tests/07-closures.vn`, `tests/09-control-flow.vn`, `tests/14-generics.vn`, `tests/19-pipeline.vn`, `tests/22-recursion.vn`, `tests/36-advanced-generics.vn`, `tests/37-complex-closures.vn`, `tests/38-advanced-higher-order.vn`, `tests/40-named-arguments.vn`, `tests/86-tagged-templates.vn`.

---

## 1. Declaración de Funciones

### Forma estándar

```varn
function add(a: int, b: int): int {
    return a + b
}

function greet(name: str): void {
    print("Hello, " + name)
}
```

### Inferencia de retorno

Cuando el tipo de retorno se puede inferir, la anotación es opcional:

```varn
function double(n: int) { return n * 2 }   // retorno inferido: int
```

---

## 2. Parámetros Nombrados (Named Arguments)

Los parámetros pueden pasarse por nombre en cualquier orden:

```varn
function describe(name: str, age: int, city: str | null = null): str {
    if (city == null) { city = "Unknown" }
    return name + " is " + age.toString() + " years old and lives in " + city
}
// tests/40-named-arguments.vn

// Básico en orden
assert("in-order", describe(name: "Alice", age: 30, city: "London") == "Alice is 30 years old and lives in London")

// Fuera de orden
assert("out-of-order", describe(age: 25, name: "Bob") == "Bob is 25 years old and lives in Unknown")

// Mezcla de posicional y nombrado
assert("mixed", describe("Charlie", age: 40) == "Charlie is 40 years old and lives in Unknown")
```

---

## 3. Parámetros con Valor por Defecto

```varn
function describe(name: str, age: int, city: str | null = null): str { /* … */ }
// tests/40-named-arguments.vn:3

function greet(greeting: str, formal: bool | null = null): str {
    if (formal == true) return greeting + ", " + this.name
    return greeting + " " + this.name
}
// tests/40-named-arguments.vn:26
```

---

## 4. Funciones Genéricas

```varn
function identity<T>(v: T): T { return v }                     // tests/14-generics.vn:9
assert("generic id int",  identity<int>(42) === 42)
assert("generic id str",  identity<str>("hi") === "hi")

function swap<A, B>(a: A, b: B): B { return b }
assert("generic swap", swap<int, str>(1, "x") === "x")

function applyFunc<T, U>(f: (T) => U, val: T): U {            // tests/14-generics.vn:51
    return f(val)
}
assert("generic fn infer", applyFunc((x: int) => x.toString(), 123) === "123")
```

Funciones genéricas con named args:

```varn
function identity<T>(value: T, label: str): T { return value }
assert("generic named args", identity<int>(value: 42, label: "Answer") == 42)
// tests/40-named-arguments.vn:44
```

---

## 5. Funciones de Orden Superior (HOF)

```varn
function pipe<A, B, C>(f: (A) => B, g: (B) => C): (A) => C {
    return (x: A) => g(f(x))
}
// tests/36-advanced-generics.vn:115

function flip<A, B, C>(f: (A, B) => C): (B, A) => C {
    return (b: B, a: A) => f(a, b)
}

const toStr = (n: int) => `${n}`
const addBang = (s: str) => s + "!"
const numToExclaim = pipe(toStr, addBang)
assert("pipe result", numToExclaim(42) === "42!")

const sub = (a: int, b: int) => a - b
const flippedSub = flip(sub)
assert("flip result", flippedSub(3, 10) === 7)
```

---

## 6. Closures

```varn
function makeAdder(n: int): (a: int) => int {
    return (x: int) => x + n       // captura 'n' del ámbito exterior
}
const add5 = makeAdder(5)
const add10 = makeAdder(10)
assert("closure add5",  add5(3) === 8)     // tests/07-closures.vn:7
assert("closure add10", add10(3) === 13)
assert("closure independent", add5(0) !== add10(0))

// Contador con estado mutable
function makeCounter(): () => int {
    let c = 0
    return () => {
        c = c + 1
        return c
    }
}
const cnt = makeCounter()
assert("counter first",  cnt() === 1)
assert("counter second", cnt() === 2)
```

---

## 7. Funciones Recursivas

```varn
function fib(n: int): int {
    if (n <= 1) return n
    return fib(n - 1) + fib(n - 2)
}
assert("fib 10", fib(10) === 55)   // tests/22-recursion.vn:20

function power(base: int, exp: int): int {
    if (exp === 0) return 1
    return base * power(base, exp - 1)
}
assert("power 2^10", power(2, 10) === 1024)
```

### Recursión mutua

```varn
function isEven2(n: int): bool {
    if (n === 0) return true
    return isOdd2(n - 1)
}
function isOdd2(n: int): bool {
    if (n === 0) return false
    return isEven2(n - 1)
}
assert("mutual even", isEven2(4))    // tests/22-recursion.vn:3-10
assert("mutual odd",  isOdd2(7))
```

---

## 8. Funciones Asíncronas (`async`)

```varn
import { sleep, spawn, parallel } from "std:task"

async function asyncAdd(a: int, b: int): int {
    return a + b
}
// tests/21-async.vn:4

async function asyncChain(): str {
    const x = await asyncAdd(3, 4)
    const y = await asyncAdd(x, 10)
    return `result=${y}`
}
```

Ver [`async_concurrency.md`](async_concurrency.md) para cobertura completa.

---

## 9. Generadores (`function*`)

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
const g0 = gen.next()
assert("gen value 0", g0.value === 0)
assert("gen done 0",  !g0.done)
```

Ver [`generators.md`](generators.md) para cobertura completa.

---

## 10. Tagged Template Functions

Funciones que reciben strings e interpolaciones de un template literal:

```varn
function customTag(strings: str[], ...values: dynamic[]): str {
    let res = ""
    for (let i = 0; i < strings.length; i = i + 1) {
        res = res + strings[i]
        if (i < values.length) { res = res + "[" + values[i] + "]" }
    }
    return res
}
// tests/86-tagged-templates.vn:6

let name = "Varn"
let version = 1
let output = customTag`Language: ${name}, Version: ${version}!`
assertEqual(output, "Language: [Varn], Version: [1]!")
```

---

## 11. Memoización con Genéricos

```varn
function memoize<T>(f: (a:int) => T): (int) => T {
    const cache: T[] = []
    const seen: bool[] = []
    return (n: int) => {
        if (n < seen.length && seen[n]) return cache[n]
        const result = f(n)
        while (cache.length <= n) { cache.push(result); seen.push(false) }
        cache[n] = result
        seen[n] = true
        return result
    }
}
// tests/36-advanced-generics.vn:133

let callCount = 0
const cached = memoize((n) => { callCount = callCount + 1; return n })
cached(5); cached(5); cached(5)
assert("memoize called once", callCount === 1)
```

---

## 12. Acumuladores Genéricos

```varn
function makeAccumulator<T>(init: T, combine: (T, T) => T): (v: T) => T {
    let state = init
    return (v: T) => {
        state = combine(state, v)
        return state
    }
}
// tests/36-advanced-generics.vn:158

const sumAcc = makeAccumulator<int>(0, (a, b) => a + b)
sumAcc(5); sumAcc(3)
const total = sumAcc(2)
assert("accumulator sum", total === 10)

const strAcc = makeAccumulator<str>("", (a, b) => a + b)
strAcc("foo"); strAcc("bar")
const built = strAcc("!")
assert("accumulator str", built === "foobar!")
```

---

## 13. Firmas de Tipos de Función

```varn
// Función que recibe una función
function compose<A, B, C>(f: (a:B) => C, g: (b:A) => B): (c:A) => C

// Lambda con tipo anotado explícito
const add5: (a: int) => int = makeAdder(5)

// Rest parameters
function rolesDeco(...r: str[]) { /* r es str[] */ }
```
