# Varn Language — Sistema de Tipos

> Fuentes: `tests/08-null-safety.vn`, `tests/12-classes.vn`, `tests/13-interfaces.vn`, `tests/14-generics.vn`, `tests/15-unions.vn`, `tests/16-enums.vn`, `tests/24-record.vn`, `tests/26-numeric-coercion.vn`, `tests/30-nullable-types.vn`, `tests/34-char-type.vn`, `tests/35-decimal-bigint.vn`, `tests/36-advanced-generics.vn`, `tests/41-advanced-enums.vn`, `tests/69-tuples-records.vn`, `tests/79-control-flow-narrowing.vn`, `tests/87-primary-constructors-nullish-assign.vn`.

---

## 1. Tipos Primitivos

| Tipo | Descripción | Ejemplo literal |
|------|-------------|----------------|
| `int` | Entero de 64 bits con signo | `42`, `-7`, `0` |
| `float` | Flotante de 64 bits | `3.14`, `1.0`, `-0.5` |
| `decimal` | Decimal de precisión arbitraria | `1.5d`, `99.25d` |
| `bigint` | Entero de precisión arbitraria | `100n`, `1n` |
| `bool` | Booleano | `true`, `false` |
| `char` | Carácter Unicode | `'a'`, `'Z'`, `'!'` |
| `str` | Cadena de texto inmutable | `"hello"`, `"""raw"""` |
| `null` | Ausencia de valor | `null` |

Todos son tipos canónicos únicos; **nunca** se aceptan aliases como `string`, `boolean`, `integer`.

---

## 2. Tipos Compuestos

### 2.1 Arrays

```varn
const nums: int[] = [3, 1, 4, 1, 5]       // tests/05-arrays.vn:2
let explicitGeneric: Array<int> = [10, 20, 30]  // tests/05-arrays.vn:67
let explicitBracket: int[] = explicitGeneric     // Asignable mutuamente
let nested: int[][] = [[1, 2], [3, 4]]           // 2D array
```

`Array<T>` y `T[]` son semánticamente idénticos y asignables bidireccionalmente.

### 2.2 Tuples (`#[…]`)

Colecciones heterogéneas inmutables con igualdad estructural:

```varn
let t1 = #[1, "hello", true]    // tests/69-tuples-records.vn:4
assert("tuple elem 0", t1[0] == 1)
assert("tuple length", t1.length == 3)

let t2 = #[1, "hello", true]
assert("tuple eq true", t1 == t2)   // igualdad estructural profunda
```

### 2.3 Records (`#{…}`)

Objetos inmutables con igualdad estructural:

```varn
let r1 = #{ name: "Varn", version: 1 }   // tests/69-tuples-records.vn:22
assert("record field name",    r1.name == "Varn")
assert("record index name",    r1["name"] == "Varn")

let r2 = #{ name: "Varn", version: 1 }
assert("record eq true", r1 == r2)       // igualdad estructural
```

### 2.4 `Record<K, V>` (Mapa tipado de clave→valor)

```varn
const scores: Record<str, int> = {   // tests/24-record.vn:2
    "alice": 95,
    "bob": 87,
}
assert("record get alice", scores["alice"] === 95)
```

### 2.5 Map y Set

```varn
const map = new Map<int>()    // tests/17-map-set.vn:2
map.set("a", 1)
assert("map size", map.size === 3)
assert("map get",  map.get("b") === 2)
assert("map has",  map.has("c"))
map.delete("b")
map.clear()

const set = new Set<str>()
set.add("x")
set.add("x")     // duplicado ignorado
assert("set size dedup", set.size === 2)
```

---

## 3. Tipos Nulables

El sistema de tipos distingue entre un tipo no-nulo y su versión nulable:

```varn
const a: str? = null       // str | null  (tests/30-nullable-types.vn:3)
const b: str? = "hello"
const f: int?[] = [1, null, 3]   // array de int nulables
const h: int[]? = null            // array nulo
const j: int?[]? = null           // array nulo de int nulables
```

**Atajos de tipo:**

| Sintaxis | Equivale a |
|----------|-----------|
| `T?` | `T \| null` |
| `T?[]` | `(T \| null)[]` |
| `T[]?` | `T[] \| null` |
| `T?[]?` | `(T \| null)[] \| null` |

---

## 4. Uniones de Tipos

```varn
type StringOrInt = str | int    // tests/15-unions.vn:2
type MaybeStr = str | null

function processValue(v: StringOrInt): str {
    if (v instanceof str) {
        return "string: " + v
    } else {
        return "number: " + v
    }
}
assert("union str branch", processValue("hello") === "string: hello")
assert("union int branch", processValue(42) === "number: 42")
```

### Tipos unión de clases

```varn
type Shape2 = Square2 | Circle2    // tests/15-unions.vn:33

function totalArea(shapes: Shape2[]): float {
    let total = 0.0
    for (const s of shapes) {
        total = total + s.area()
    }
    return total
}
```

---

## 5. Enums

### 5.1 Enums simples

```varn
enum Direction { North, South, East, West }    // tests/16-enums.vn:2

const dir: Direction = Direction.North
assert("enum value",    dir === Direction.North)
assert("enum rawValue", Direction.East.rawValue === 2)
```

### 5.2 Enums con valores explícitos

```varn
enum HttpMethod { GET = 0, POST = 1, PUT = 2, DELETE = 3 }
assert("enum explicit", HttpMethod.POST.rawValue === 1)

enum Status {
    Pending,
    Success = 10,
    Error        // rawValue = 11 (auto-increments)
}
```

### 5.3 Enums con payload (ADTs)

```varn
enum Token {                          // tests/41-advanced-enums.vn:27
    Identifier(name: str),
    Number(value: float),
    String(value: str),
    Plus,
    Minus
}

const t1 = Token.Number(123)
const t2 = Token.Identifier(name: "hello")   // named argument
assert("payload value", t1.value === 123)
assert("named payload", t2.name === "hello")
```

### 5.4 Enums con campos compartidos y constructor

```varn
enum HttpStatus {
    Ok(200, "OK"),
    NotFound(404, "Not Found");

    code: int
    message: str

    constructor(code: int, message: str) {
        this.code = code
        this.message = message
    }
}
assert("shared field code Ok",     HttpStatus.Ok.code === 200)
assert("shared field message",     HttpStatus.Ok.message === "OK")
```

### 5.5 Enums con métodos y pattern matching en `this`

```varn
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
assert("rect area",   Shape.Rectangle(10.0, 20.0).area() === 200.0)
```

### 5.6 Enums genéricos

```varn
enum Result<T, E> {
    Ok(value: T),
    Err(error: E)
}
const resOk = Result.Ok<int, str>(42)
```

### 5.7 Enums que implementan interfaces

```varn
enum CustomDisplay implements Display {
    A, B;
    toString(): str { return "CustomDisplay." + this.name }
}
assert("interface method", CustomDisplay.A.toString() === "CustomDisplay.A")
```

---

## 6. Interfaces

```varn
interface Printable {            // tests/13-interfaces.vn:2
    toString(): str
}
interface Serializable {
    serialize(): str
}
interface Options {
    verbose?: bool        // campo opcional
    maxRetries?: int
    tag: str
}
```

Implementación múltiple:

```varn
class Config implements Printable, Serializable {
    key: str
    value: int
    constructor(k: str, v: int) { this.key = k; this.value = v }
    toString(): str { return `${this.key}=${this.value}` }
    serialize(): str { return `{"${this.key}":${this.value}}` }
}
```

Varn usa **tipado estructural**: si un tipo satisface la interfaz, puede usarse donde se espera la interfaz.

---

## 7. Clases

Ver también [`classes_objects.md`](classes_objects.md).

```varn
class Animal {                        // tests/12-classes.vn:27
    name: str
    constructor(n: str) { this.name = n }
    speak() { return "..." }
}
class Dog extends Animal {
    constructor(n: str) { super(n) }
    override speak() { return "Woof" }
}
```

---

## 8. Generics

```varn
class Box<T> {                        // tests/14-generics.vn:1
    value: T
    constructor(v: T) { this.value = v }
    get(): T { return this.value }
    map<U>(f: (T) => U): Box<U> { return new Box<U>(f(this.value)) }
}

function identity<T>(v: T): T { return v }
function swap<A, B>(a: A, b: B): B { return b }

const intBox = new Box<int>(10)
assert("generic box get", intBox.get() === 10)
const strBox = intBox.map((n) => `value=${n}`)
assert("generic box map", strBox.get() === "value=10")
```

Interfaces genéricas:

```varn
interface Comparable<T> {
    compareTo(other: T): int
}
```

---

## 9. Coerción Numérica (Widening implícito)

```varn
const i: int = 42
const f: float = i       // int → float: widening implícito (tests/26-numeric-coercion.vn:6)
const d: decimal = i     // int → decimal: implícito
const bi: bigint = i     // int → bigint: implícito

function takesFloat(x: float): float { return x + 1.0 }
assert("int arg to float param", takesFloat(5) === 6.0)

const a: int = 10
const b: float = 2.5
const c: float = a + b    // int se amplía a float para la operación
assert("int + float = float", c === 12.5)
```

**Narrowing (restricción)** requiere cast explícito con `as`.

---

## 10. Control-Flow Type Narrowing

El checker estrecha automáticamente el tipo dentro de bloques condicionales:

```varn
function processOptional(val: str?): int {
    if (val !== null) {
        return val.length   // val se estrecha a 'str' aquí (tests/79-control-flow-narrowing.vn:7)
    }
    return 0
}

function processTruthy(val: str?): bool {
    if (val) {              // truthy → val es str
        return val.length > 0
    }
    return false
}

function speak(a: Animal): str {
    if (a instanceof Dog) {
        return a.bark()     // a se estrecha a Dog
    }
    return a.name
}
```

---

## 11. Tipo `dynamic`

Tipo dinámico para valores cuya naturaleza se determina en tiempo de ejecución:

```varn
function processTypeof(val: dynamic): str {    // tests/79-control-flow-narrowing.vn:38
    if (typeof val === "str") {
        return val.toUpperCase()
    } else if (typeof val === "int") {
        return "number:" + val
    }
    return "other"
}
```

---

## 12. Rangos

```varn
const r1 = 0..5     // rango exclusivo [0, 5) (tests/18-ranges.vn:2)
const r2 = 1..=5    // rango inclusivo [1, 5]

assert("range start",       r1.start === 0)
assert("range end excl",    r1.end === 5)
assert("range length excl", r1.length === 5)
assert("range contains",    r1.contains(3))
assert("range incl",        r2.contains(5))

const arr = (0..4).toArray()     // [0, 1, 2, 3]
const stepped = (0..10).step(3)  // [0, 3, 6, 9]
const r3 = Range.from(5, 10)
```

Indexación directa con rangos en strings y arrays:

```varn
let text = "Hello, World!"
let greeting = text[0..5]     // "Hello"  (tests/88-range-indexing.vn:10)
let greetingInc = text[0..=4] // "Hello"

let numbers = [10, 20, 30, 40, 50, 60]
let slice = numbers[1..4]     // [20, 30, 40]
let slice2 = numbers[1..=3]   // [20, 30, 40]
```
