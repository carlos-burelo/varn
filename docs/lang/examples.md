# Varn Language — Ejemplos Integrados

Colección de ejemplos completos extraídos directamente de los archivos de prueba `tests/*.vn`. Cada sección corresponde a un tema del lenguaje y muestra el código real que pasa en la suite de 89 test suites.

---

## 01 — Aritmética (`tests/01-arithmetic.vn`)

```varn
assert("int add",          1 + 2 === 3)
assert("int sub negative", 3 - 5 === -2)
assert("int mul zero",     42 * 0 === 0)
assert("int div",          10 / 2 === 5)
assert("int mod",          17 % 5 === 2)
assert("int mod negative", -7 % 3 === -1)
assert("int power",        2 ** 10 === 1024)
assert("float add",        0.1 + 0.2 > 0.29 && 0.1 + 0.2 < 0.31)
assert("unary neg",        -(5) === -5)
assert("bitwise and",      (12 & 10) === 8)
assert("bitwise or",       (12 | 3) === 15)
assert("bitwise xor",      (12 ^ 10) === 6)
assert("bitwise not",      ~0 === -1)
assert("left shift",       1 << 4 === 16)
assert("right shift",      64 >> 2 === 16)
```

---

## 02 — Booleanos (`tests/02-boolean.vn`)

```varn
assert("and short-circuit", (false && (1/0 === 0)) === false)
assert("or short-circuit",  true  || (1/0 === 0) === true)
assert("not not",           !!true === true)
assert("triple eq str",     "abc" === "abc")
assert("triple neq str",    "abc" !== "def")
assert("triple eq null",    null === null)
assert("triple neq null",   null !== 0)
```

---

## 03 — Cadenas de Texto (`tests/03-strings.vn`)

```varn
const s = "  Hello, World!  "
assert("trim",           s.trim() === "Hello, World!")
assert("toUpperCase",    "hello".toUpperCase() === "HELLO")
assert("includes",       "typescript".includes("script"))
assert("startsWith",     "hello world".startsWith("hello"))
assert("indexOf found",  "abcabc".indexOf("c") === 2)
assert("slice",          "hello world".slice(6, 11) === "world")
assert("replace",        "foo bar foo".replace("foo", "baz") === "baz bar foo")
assert("replaceAll",     "foo bar foo".replaceAll("foo", "baz") === "baz bar baz")
assert("split",          "a,b,c".split(",").length === 3)
assert("repeat",         "ab".repeat(3) === "ababab")
assert("padStart",       "5".padStart(3, "0") === "005")
assert("length",         "hello".length === 5)
assert("charCode A",     "A".charCode() === 65)
assert("isEmpty true",   "".isEmpty())
assert("capitalize",     "hello".capitalize() === "Hello")
assert("lines",          "a\nb\nc".lines().length === 3)
assert("str.from int",   str.from(42) === "42")
assert("str.EMPTY",      str.EMPTY === "")
assert("str.fromCharCode", str.fromCharCode(72, 105) === "Hi")
assert("str.join sep",   str.join(["a", "b", "c"], "-") === "a-b-c")
```

---

## 04 — Template Literals (`tests/04-templates.vn`)

```varn
const tval = 42
assert("simple template",   `answer = ${tval}` === "answer = 42")
assert("expr in template",  `${tval * 2}` === "84")
assert("nested template",   `prefix_${"mid"}_suffix` === "prefix_mid_suffix")
assert("template + concat", `a${1 + 1}b` + "c" === "a2bc")
```

---

## 05 — Arrays (`tests/05-arrays.vn`)

```varn
const nums = [3, 1, 4, 1, 5, 9, 2, 6]
assert("length",  nums.length === 8)
assert("indexOf", nums.indexOf(5) === 4)
assert("includes",nums.includes(9))

const doubled = nums.map((n) => n * 2)
assert("map value", doubled[0] === 6)

const evens = nums.filter((n) => n % 2 === 0)
assert("filter", evens.length === 3)

const sum = nums.reduce((acc, n) => acc + n, 0)
assert("reduce sum", sum === 31)

const found = nums.find((n) => n > 5)
assert("find first", found === 9)

const mut = [1, 2, 3]
mut.push(4)
assert("push value", mut[3] === 4)
const popped = mut.pop()
assert("pop value", popped === 4)

const spread = [...[1, 2], ...[3, 4]]
assert("spread value", spread[2] === 3)

// Semantic identity: Array<T> ↔ T[]
let explicitGeneric: Array<int> = [10, 20, 30]
let explicitBracket: int[] = explicitGeneric
assert("Array<T> to T[]", explicitBracket[1] === 20)
```

---

## 06 — Destructuring (`tests/06-destructuring.vn`)

```varn
const obj1 = { a: 1, b: 2, c: 3 }
const { a, b } = obj1
assert("obj destr basic", a === 1 && b === 2)

const { a: renamed } = obj1
assert("obj destr rename", renamed === 1)

const { name = "default", age = 0 } = { name: "Alice" }
assert("obj destr default name", name === "Alice")
assert("obj destr default age",  age === 0)

const [f1, f2, f3] = [10, 20, 30]
const [head, ...tail] = [1, 2, 3, 4]
assert("rest head", head === 1)
assert("rest tail", tail.length === 3)

const base = { x: 1, y: 2 }
const ext  = { ...base, z: 3 }
assert("obj spread z", ext.z === 3)
```

---

## 07 — Closures (`tests/07-closures.vn`)

```varn
function makeAdder(n: int): (a: int) => int {
    return (x: int) => x + n
}
const add5 = makeAdder(5)
assert("closure add5",        add5(3) === 8)
assert("closure independent", add5(0) !== makeAdder(10)(0))

function makeCounter(): () => int {
    let c = 0
    return () => { c = c + 1; return c }
}
const cnt1 = makeCounter()
assert("counter1 first",  cnt1() === 1)
assert("counter1 second", cnt1() === 2)

function compose<A, B, C>(f: (a:B) => C, g: (b:A) => B): (c:A) => C {
    return (x: A) => f(g(x))
}
const double = (n: int) => n * 2
const inc    = (n: int) => n + 1
const doubleInc = compose(double, inc)
assert("compose", doubleInc(4) === 10)
```

---

## 08 — Null Safety (`tests/08-null-safety.vn`)

```varn
interface Profile { name: str; address?: Address }

const p1: Profile? = { name: "Alice", address: { city: "NY" } }
const p3: Profile? = null

assert("?. non-null chain", p1?.address?.city === "NY")
assert("?. null root",      p3?.name === null)
assert("?? left non-null",  (42 ?? 0) === 42)
assert("?? left null",      (null ?? 99) === 99)
assert("?. + ??",           (p3?.name ?? "anon") === "anon")
assert("?? chained",        (null ?? null ?? 7) === 7)
```

---

## 09 — Control de Flujo (`tests/09-control-flow.vn`)

```varn
function classify(n: int): str {
    return n < 0 ? "neg" : n === 0 ? "zero" : "pos"
}

// for con break
function firstAbove(arr: int[], threshold: int): int {
    let i = 0; let result = -1
    while (i < arr.length) {
        if (arr[i] > threshold) { result = arr[i]; break }
        i = i + 1
    }
    return result
}

// for con continue
function sumOdds(max: int): int {
    let total = 0
    for (let i = 0; i <= max; i = i + 1) {
        if (i % 2 === 0) continue
        total = total + i
    }
    return total
}
assert("for continue", sumOdds(10) === 25)

// for-of
let forOfSum = 0
for (const n of [10, 20, 30]) { forOfSum = forOfSum + n }
assert("for-of sum", forOfSum === 60)

// for-in
const obj2 = { x: 1, y: 2, z: 3 }
let keyCount = 0
for (const k in obj2) { keyCount = keyCount + 1 }
assert("for-in key count", keyCount === 3)
```

---

## 10 — Match (`tests/10-match.vn`)

```varn
function describeNum(n: int): str {
    return match (n) {
        0 => "zero",
        1 => "one",
        2 | 3 => "two or three",
        _ => "other"
    }
}
assert("match 2",     describeNum(2) === "two or three")
assert("match other", describeNum(99) === "other")
```

---

## 11 — Manejo de Errores (`tests/11-errors.vn`)

```varn
// try/catch
try {
    throw new TypeError("bad type")
} catch (e) {
    assert("catch TypeError name", e.name === "TypeError")
}

// Error personalizado
class ValidationError extends Error {
    field: str
    constructor(field: str, msg: str) {
        super(msg)
        this.name = "ValidationError"
        this.field = field
    }
}

// finally
function withFinally(): int {
    try { return 42 } finally { finallyRan = true }
}
assert("finally return value", withFinally() === 42)
assert("finally ran",          finallyRan)
```

---

## 16 — Enums (`tests/16-enums.vn`)

```varn
enum Direction { North, South, East, West }
enum HttpMethod { GET = 0, POST = 1, PUT = 2, DELETE = 3 }

assert("enum value",    Direction.North === Direction.North)
assert("enum neq",      Direction.North !== Direction.South)
assert("enum explicit", HttpMethod.POST.rawValue === 1)
assert("enum rawValue", Direction.East.rawValue === 2)

function describeDir(d: Direction): str {
    return match (d) {
        Direction.North => "going north",
        Direction.South => "going south",
        Direction.East  => "going east",
        Direction.West  => "going west"
    }
}
```

---

## 19 — Pipeline (`tests/19-pipeline.vn`)

```varn
assert("pipe simple",      5 |> double2 === 10)
assert("pipe placeholder", 7 |> addN(_, 3) === 10)
assert("pipe multi _",     4 |> addN(_, _) === 8)
assert("pipe clamp lo",    -5 |> clamp2(0, 10, _) === 0)
const piped = (3 |> double2) |> double2
assert("pipe chained", piped === 12)
```

---

## 20 — Generadores (`tests/20-generators.vn`)

```varn
function* range_gen(start: int, end: int) {
    let i = start
    while (i < end) { yield i; i = i + 1 }
}
const gen = range_gen(0, 5)
assert("gen value 0", gen.next().value === 0)
assert("gen done 0",  !gen.next().done)

function* fib_gen() {
    let a = 0; let b = 1
    while (true) {
        yield a
        const tmp = a + b; a = b; b = tmp
    }
}
const fibs: int[] = []
const fib2 = fib_gen()
for (let i = 0; i < 8; i = i + 1) { fibs.push(fib2.next().value) }
assert("fib gen [7]", fibs[7] === 13)
```

---

## 21 — Async/Await (`tests/21-async.vn`)

```varn
import { sleep, TaskGroup, spawn, parallel } from "std:task"

async function asyncAdd(a: int, b: int): int { return a + b }

async function runAsync(): void {
    assert("async add", await asyncAdd(5, 6) === 11)

    const all = await parallel([asyncSquare(2), asyncSquare(3), asyncSquare(4)])
    assert("parallel[0]", all[0] === 4)

    using group = TaskGroup<int>()
    const g1 = group.spawn(asyncAdd(7, 8))
    const joined = await group.join()
    assert("group joined[0]", joined[0] === 15)
}
await runAsync()
```

---

## 25 — Extension Methods (`tests/25-extensions.vn`)

```varn
extension StringUtils on str {
    shout(): str { return this + "!" }
    times(n: int): str {
        let result = ""; let i = 0
        while (i < n) { result = result + this; i = i + 1 }
        return result
    }
}
extension IntUtils on int {
    isEven(): bool { return this % 2 === 0 }
    clamp(lo: int, hi: int): int {
        if (this < lo) return lo
        if (this > hi) return hi
        return this
    }
}

assert("str shout",    "hello".shout() === "hello!")
assert("str times",    "ab".times(3) === "ababab")
assert("int isEven",   (4).isEven() === true)
assert("int clamp lo", (2).clamp(5, 10) === 5)
```

---

## 41 — Enums Avanzados (`tests/41-advanced-enums.vn`)

```varn
// Payload variant
enum Token {
    Identifier(name: str),
    Number(value: float),
    Plus, Minus
}
const t1 = Token.Number(123)
assert("payload value", t1.value === 123)

// Shared fields + constructor
enum HttpStatus {
    Ok(200, "OK"),
    NotFound(404, "Not Found");
    code: int; message: str
    constructor(code: int, message: str) { this.code = code; this.message = message }
}
assert("shared field", HttpStatus.Ok.code === 200)

// Enum with methods
enum Shape {
    Circle(radius: float),
    Rectangle(width: float, height: float);
    area(): float {
        return match (this) {
            Circle(radius)           => 3.14159 * radius * radius,
            Rectangle(width, height) => width * height
        }
    }
}
assert("circle area", Shape.Circle(10.0).area() === 314.159)

// Generic enum
enum Result<T, E> { Ok(value: T), Err(error: E) }
const resOk = Result.Ok<int, str>(42)
const valOk = match (resOk) { Ok(val) => val, Err(err) => 0 }
assert("generic enum Ok", valOk === 42)
```

---

## 47 — Isolates (`tests/47-isolates-multithread.vn`)

```varn
import { spawnIsolate, channel, Sender, Receiver } from "std:task"

export async function workerMain(rx: Receiver<ValueMsg>, tx: Sender<int>) {
    const data = await rx.receive()
    await tx.send(data.value * 2)
}

const in1 = channel<ValueMsg>(1)
const out1 = channel<int>(1)
const h1 = await spawnIsolate(workerMain, [in1.rx, out1.tx])
await in1.tx.send({ value: 10 })
assert("response from basic worker", await out1.rx.receive() === 20)
await h1.join()
```

---

## 69 — Tuples y Records (`tests/69-tuples-records.vn`)

```varn
let t1 = #[1, "hello", true]
assert("tuple elem 0", t1[0] == 1)
assert("tuple length", t1.length == 3)
let t2 = #[1, "hello", true]
assert("tuple eq true", t1 == t2)

let r1 = #{ name: "Varn", version: 1 }
assert("record field", r1.name == "Varn")
let r2 = #{ name: "Varn", version: 1 }
assert("record eq", r1 == r2)

let nested1 = #{ point: #[10, 20], meta: #{ active: true } }
let nested2 = #{ point: #[10, 20], meta: #{ active: true } }
assert("nested eq true", nested1 == nested2)
```

---

## 86 — Tagged Templates (`tests/86-tagged-templates.vn`)

```varn
function customTag(strings: str[], ...values: dynamic[]): str {
    let res = ""
    for (let i = 0; i < strings.length; i = i + 1) {
        res = res + strings[i]
        if (i < values.length) { res = res + "[" + values[i] + "]" }
    }
    return res
}

let name = "Varn"
let version = 1
let output = customTag`Language: ${name}, Version: ${version}!`
assertEqual(output, "Language: [Varn], Version: [1]!")

// Safe SQL with tagged templates
let rows = db.sql`SELECT * FROM users WHERE role = ${targetRole} AND active = ${isActive}`
```

---

## 87 — Primary Constructors y `??=` (`tests/87-primary-constructors-nullish-assign.vn`)

```varn
// C# style primary constructor
class User(public id: int, public name: str, private role: str = "guest") {
    greet(): str { return `User #${this.id}: ${this.name} [${this.role}]` }
}
let u1 = new User(1, "Alice", "admin")
assertEqual(u1.greet(), "User #1: Alice [admin]")

// TypeScript style parameter properties
class Hero {
    constructor(public id: int, public name: str, private power: float = 95.5) {}
}

// ??= operator
let a: int? = null
a ??= 42
assertEqual(a, 42)
a ??= 999    // no cambia
assertEqual(a, 42)
```

---

## 88 — Range Indexing (`tests/88-range-indexing.vn`)

```varn
let text = "Hello, World!"
let greeting = text[0..5]
assertEqual(greeting, "Hello")

let numbers = [10, 20, 30, 40, 50, 60]
let slice = numbers[1..4]    // [20, 30, 40]
assertEqual(slice.length, 3)
assertEqual(slice[0], 20)

let slice2 = numbers[1..=3]   // inclusivo
assertEqual(slice2.length, 3)

let fruits = ["apple", "banana", "cherry", "date"]
let start = 1; let end = 4
let picked = fruits[start..end]
assertEqual(picked[0], "banana")
```

---

## 89 — Raw Strings y `with` (`tests/89-raw-strings-and-with-expressions.vn`)

```varn
// Raw String Literals
let jsonStr = """{
  "name": "Varn",
  "path": "C:\Program Files\Varn"
}""";
expect(jsonStr.includes("\"name\": \"Varn\"")).toBe(true)

let text = """Hello "world" and 'quotes' without escapes!""";
assertEqual(text, "Hello \"world\" and 'quotes' without escapes!")

// with expressions
class User(public id: int, public name: str, public role: str = "guest") {}

let u1 = new User(10, "Alice", "admin")
let u2 = u1 with { name: "Bob", role: "editor" }
assertEqual(u2.id, 10)        // preservado
assertEqual(u2.name, "Bob")   // sobreescrito
assertEqual(u1.name, "Alice") // u1 inmutable

// with en objetos planos
let baseConfig = { host: "localhost", port: 3000, debug: true }
let prodConfig = baseConfig with { port: 8080, debug: false }
assertEqual(prodConfig.host, "localhost")
assertEqual(prodConfig.port, 8080)
assertEqual(baseConfig.port, 3000)   // original intacto
```
