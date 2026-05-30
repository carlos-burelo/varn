# Especificación del Lenguaje Varn

Referencia de la semántica visible del lenguaje.

## Principios de diseño

1. Una sola semántica por constructo.
2. Lo explícito por encima de lo implícito.
3. Tipos que el compilador puede verificar con precisión.
4. Errores que enseñan, no solo castigan.
5. Sin rutas silenciosas, sin compatibilidad heredada innecesaria.
6. El host no invade la sintaxis pública.

---

## Sistema de tipos

### Primitivos

| Tipo | Descripción |
|------|-------------|
| `int` | Entero 64-bit |
| `float` | Flotante 64-bit IEEE 754 |
| `decimal` | Decimal preciso (rust_decimal) |
| `str` | String UTF-8 |
| `char` | Carácter Unicode |
| `bool` | `true` / `false` |
| `null` | Ausencia de valor |
| `void` | Sin retorno |
| `never` | Bottom type, nunca retorna |
| `unknown` | Exige narrowing antes de uso |

### Compuestos

```Varn
T[]           // array
T | U         // unión
T & U         // intersección
[T, U, V]     // tupla
T?            // nullable (azúcar para T | null)
Generic<T>    // genérico
(x: T) => U  // función
{ name: str } // objeto estructural
```

### Reglas clave
- `T?` = `T | null`. El checker trabaja con la forma normalizada.
- `never` es bottom real — ningún valor lo habita.
- `unknown` exige narrowing explícito antes de operar.
- `type` es alias estructural. `newtype` es nominal.

---

## Declaraciones

### Variables

```Varn
const x = 42          // inmutable, tipo inferido
const y: float = 3.14  // con anotación
let count = 0          // mutable
```

### Funciones

```Varn
function add(a: int, b: int): int { return a + b }
function greet(name: str): void { print(`Hola ${name}`) }
function identity<T>(x: T): T { return x }

// Named arguments — llamada en cualquier orden
function describe(name: str, age: int): str { return `${name} tiene ${age}` }
describe(age: 25, name: "Bob")
```

### Clases

```Varn
class Animal {
    name: str
    sound: str = "..."
    constructor(n: str) { this.name = n }
    speak(): str { return `${this.name} says ${this.sound}` }
    get info(): str { return `Animal: ${this.name}` }
}

abstract class Shape {
    abstract area(): float
    describe(): str { return `Area: ${this.area()}` }
}

class Dog extends Animal {
    constructor(n: str) {
        super(n)
        this.sound = "Woof"
    }
    override speak(): str { return `${super.speak()}!` }
    static create(n: str): Dog { return new Dog(n) }
}
```

### Interfaces

```Varn
interface Printable {
    print(): void
}
interface Serializable {
    serialize(): str
}

class Report implements Printable, Serializable {
    print(): void { print(this.serialize()) }
    serialize(): str { return "Report()" }
}
```

### Genéricos

```Varn
class Box<T> {
    value: T
    constructor(v: T) { this.value = v }
    get(): T { return this.value }
    map<U>(f: (T) => U): Box<U> { return new Box<U>(f(this.value)) }
}

function identity<T>(v: T): T { return v }

function swap<A, B>(a: A, b: B): B { return b }
```

### Extensions

```Varn
extension StringExt on str {
    capitalize(): str {
        if (this.length === 0) { return this }
        return this[0].toUpperCase() + this.slice(1)
    }
    get wordCount(): int { return this.split(" ").length }
}

print("hola mundo".capitalize())  // Hola mundo
print("hola mundo".wordCount)     // 2
```

### Enums

```Varn
enum Direction { North, South, East, West }
enum Status { Active = 1, Inactive = 0, Pending = 2 }

function describeDir(d: Direction): str {
    return match (d) {
        Direction.North => "north",
        Direction.South => "south",
        Direction.East  => "east",
        Direction.West  => "west"
    }
}
```

### Namespaces

```Varn
namespace Utils {
    export function clamp(val: int, min: int, max: int): int {
        if (val < min) { return min }
        if (val > max) { return max }
        return val
    }
}
print(Utils.clamp(15, 0, 10))  // 10
```

---

## Control de flujo

```Varn
// if/else
if (x > 0) {
    print("positive")
} else if (x < 0) {
    print("negative")
} else {
    print("zero")
}

// while
while (count < 10) { count = count + 1 }

// for..of
for (const item of array) { print(item) }
for (const i of 0..10) { print(i) }

// match
const result = match (status) {
    Status.Active   => "active",
    Status.Inactive => "inactive",
    Status.Pending  => "pending",
}

// multi-pattern
match (val) {
    1       => print("one"),
    2 | 3   => print("two or three"),
    _       => print("other"),
}

// try/catch/finally
try {
    const data = riskyOp()
} catch (e) {
    print(`Error: ${e.message}`)
} finally {
    cleanup()
}

// throw
throw new Error("something went wrong")
```

---

## Pipeline operator

```Varn
function double(n: int): int { return n * 2 }
function addN(n: int, x: int): int { return n + x }

const result  = 5 |> double |> double          // 20
const result2 = 7 |> addN(_, 3) |> double      // 20  (_ is placeholder)
```

---

## Async

```Varn
async function fetchUser(id: int): User {
    const resp = await http.get(`/users/${id}`)
    return resp.json<User>()
}

// spawn
const task = spawn(fetchUser(1))

// parallel
const [a, b] = await parallel([fetchUser(1), fetchUser(2)])

// using with async
async function processFile(path: str): void {
    using file = await fs.open(path)
    const content = await file.readAll()
    print(content)
}
```

---

## Generators

```Varn
function* fibonacci() {
    let a = 0
    let b = 1
    while (true) {
        yield a
        const tmp = a + b
        a = b
        b = tmp
    }
}

const gen = fibonacci()
print(gen.next().value)  // 0
print(gen.next().value)  // 1
print(gen.next().value)  // 1

for (const n of fibonacci()) {
    if (n > 100) { break }
    print(n)
}
```

---

## Decoradores

```Varn
function logMethod(fn: FunctionRef, ctx: MethodContext): FunctionRef {
    const name = ctx.name
    return (...args: dynamic[]) => {
        print(`Calling ${name}`)
        return fn(...args)
    }
}

function trackClass(cls: ClassRef): void {
    print(`Class ${cls.name} defined`)
}

@trackClass
class Service {
    @logMethod
    process(data: str): str { return data.toUpperCase() }
}
```

---

## Sistema de módulos

```Varn
// Stdlib
import { readFile, writeFile } from "std:fs"
import { now, sleep } from "std:time"
import { sha256 } from "std:crypto"
import { sqrt, PI } from "std:math"

// Módulos locales
import { utils } from "./utils"
import { Model } from "../models/user"

// Paquetes externos (requiere varn.json + vn pkg install)
import { client } from "pkg:mylib"
```

---

## `using` — Gestión de recursos

```Varn
interface Disposable {
    dispose(): void
}

interface AsyncDisposable {
    async asyncDispose(): void
}

// using llama dispose() al salir del bloque
using db = createConnection()
// ...
// db.dispose() automático aquí
```

---

## Pattern matching avanzado

```Varn
// type narrowing
match (value) {
    is str => print(`string: ${value}`),
    is int => print(`int: ${value}`),
    is null => print("null"),
    _ => print("other"),
}

// destructuring in match
match (point) {
    { x: 0, y: 0 } => print("origin"),
    { x, y }       => print(`${x}, ${y}`),
}
```
