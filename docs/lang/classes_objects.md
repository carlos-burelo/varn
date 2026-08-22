# Varn Language — Clases y Objetos

> Fuentes: `tests/11-errors.vn`, `tests/12-classes.vn`, `tests/13-interfaces.vn`, `tests/23-objects.vn`, `tests/24-record.vn`, `tests/25-extensions.vn`, `tests/27-decorators.vn`, `tests/28-extensions-comprehensive.vn`, `tests/36-advanced-generics.vn`, `tests/87-primary-constructors-nullish-assign.vn`, `tests/89-raw-strings-and-with-expressions.vn`.

---

## 1. Declaración de Clases

### Forma estándar

```varn
class Animal {                         // tests/12-classes.vn:27
    name: str
    constructor(n: str) { this.name = n }
    speak() { return "..." }
    toString() { return `Animal(${this.name})` }
}
```

### Clases con visibilidad de campos

```varn
class Temperature {                    // tests/12-classes.vn:12
    private _celsius: float
    constructor(c: float) { this._celsius = c }
    get celsius() { return this._celsius }
    set celsius(v: float) { this._celsius = v }
    get fahrenheit() { return this._celsius * 1.8 + 32.0 }
}

const t = new Temperature(100.0)
assert("getter celsius",    t.celsius === 100.0)
assert("getter fahrenheit", t.fahrenheit === 212.0)
t.celsius = 0.0
assert("setter works",      t.celsius === 0.0)
```

---

## 2. Constructores Primarios (Primary Constructors)

### Estilo C#/Kotlin: parámetros en la declaración de clase

```varn
class User(public id: int, public name: str, private role: str = "guest") {
    greet(): str {
        return `User #${this.id}: ${this.name} [${this.role}]`
    }
    getRole(): str { return this.role }
}
// tests/87-primary-constructors-nullish-assign.vn:6

let u1 = new User(1, "Alice", "admin")
assert("id",    u1.id === 1)
assert("name",  u1.name === "Alice")
assert("greet", u1.greet() === "User #1: Alice [admin]")

let u2 = new User(2, "Bob")   // usa el default "guest"
assert("default role", u2.getRole() === "guest")
```

```varn
class Point(x: int, y: int) {    // sin modificador de acceso: campos públicos
    sum(): int { return this.x + this.y }
}
let pt = new Point(10, 25)
assert("sum", pt.sum() === 35)
```

### Estilo TypeScript: parámetros con modificadores en el constructor

```varn
class Hero {
    constructor(public id: int, public name: str, private power: float = 95.5) {}
    getPower(): float { return this.power }
}
// tests/87-primary-constructors-nullish-assign.vn:24

let h = new Hero(101, "Superman")
assert("id",    h.id === 101)
assert("power", h.getPower() === 95.5)
```

---

## 3. Herencia

```varn
class Dog extends Animal {             // tests/12-classes.vn:33
    constructor(n: str) { super(n) }
    override speak() { return "Woof" }
}
class PoliceDog extends Dog {
    badge: int
    constructor(n: str, badge: int) {
        super(n)
        this.badge = badge
    }
    override speak() { return super.speak() + "!" }
}

const pd = new PoliceDog("K9", 42)
assert("police dog speak",     pd.speak() === "Woof!")
assert("instanceof PoliceDog", pd instanceof PoliceDog)
assert("instanceof Dog",       pd instanceof Dog)
assert("instanceof Animal",    pd instanceof Animal)
```

---

## 4. Clases Abstractas

```varn
abstract class Shape {               // tests/12-classes.vn:67
    abstract area(): float
    describe(): str { return `shape with area ${this.area()}` }
}
class Circle extends Shape {
    r: float
    constructor(r: float) { this.r = r }
    override area(): float { return 3.14159 * this.r * this.r }
}
class Rect extends Shape {
    w: float
    h: float
    constructor(w: float, h: float) { this.w = w; this.h = h }
    override area(): float { return this.w * this.h }
}

const circ = new Circle(1.0)
assert("circle area",    circ.area() > 3.14 && circ.area() < 3.15)
assert("abstract describe", circ.describe().startsWith("shape with area"))
```

---

## 5. Métodos y Campos Estáticos

```varn
class IdGen {                         // tests/12-classes.vn:1
    private static _next: int = 1
    static next(): int {
        const id = IdGen._next
        IdGen._next = IdGen._next + 1
        return id
    }
    static reset(): void { IdGen._next = 1 }
}

assert("static method 1", IdGen.next() === 1)
assert("static method 2", IdGen.next() === 2)
IdGen.reset()
assert("static reset",    IdGen.next() === 1)
```

---

## 6. Clases Genéricas

```varn
class Box<T> {                        // tests/14-generics.vn:1
    value: T
    constructor(v: T) { this.value = v }
    get(): T { return this.value }
    map<U>(f: (T) => U): Box<U> { return new Box<U>(f(this.value)) }
}

class Node<T> {                       // tests/23-objects.vn:15
    value: T
    next: Node<T> | null
    constructor(v: T) { this.value = v; this.next = null }
}
```

---

## 7. Extension Methods

Las extensiones añaden métodos a un tipo existente sin modificar su definición original.

```varn
extension StringUtils on str {         // tests/25-extensions.vn:1
    shout(): str { return this + "!" }
    times(n: int): str {
        let result = ""
        let i = 0
        while (i < n) { result = result + this; i = i + 1 }
        return result
    }
    wordCount(): int { /* ... */ }
}

assert("str shout", "hello".shout() === "hello!")
assert("str times", "ab".times(3) === "ababab")
```

Extensiones con `get`/`set`:

```varn
extension SlotAccessor on Slot {
    get label(): str { return "Slot(" + this.value + ")" }
    set label(s: str) { this.value = s.length }
}
assert("slot getter", slot.label === "Slot(5)")
```

Extensiones sobre primitivos:

```varn
extension IntUtils on int {
    isEven(): bool { return this % 2 === 0 }
    clamp(lo: int, hi: int): int {
        if (this < lo) return lo
        if (this > hi) return hi
        return this
    }
}
assert("int isEven", (4).isEven() === true)
assert("int clamp",  (2).clamp(5, 10) === 5)
```

Extensiones sobre clases propias:

```varn
extension PointOps on Point {          // tests/28-extensions-comprehensive.vn:43
    distanceTo(other: Point): float {
        let dx: float = this.x - other.x
        let dy: float = this.y - other.y
        return (dx * dx + dy * dy) ** 0.5
    }
    isOrigin(): bool { return this.x === 0 && this.y === 0 }
    negate(): Point { return new Point(-this.x, -this.y) }
}

const p1 = new Point(3, 4)
const p2 = new Point(0, 0)
assert("distance", p1.distanceTo(p2) > 4.9 && p1.distanceTo(p2) < 5.1)
assert("negate",   p1.negate().x === -3)
```

---

## 8. Decoradores

Los decoradores son funciones aplicadas a clases o métodos en tiempo de declaración.

### Decoradores de clase

```varn
import { ClassRef } from "std:reflect"   // tests/27-decorators.vn:1

let deco_log: str[] = []
function trackClass(cls: ClassRef): void {
    deco_log.push(cls.name)
}

@trackClass
class DecoratedA {}
assert("class deco called", deco_log.length === 1)
assert("class deco name",   deco_log[0] === "DecoratedA")
```

### Decoradores con argumentos

```varn
function markDeco(label: str) {
    return (cls: ClassRef) => {
        deco_log.push(label + ":" + cls.name)
    }
}

@markDeco("outer")
@markDeco("inner")     // los decoradores se aplican de dentro hacia fuera
class DecoratedB {}
assert("inner first",  deco_log[1] === "inner:DecoratedB")
assert("outer second", deco_log[2] === "outer:DecoratedB")
```

### Metadatos con `MetaKey`

```varn
const RouteKey = MetaKey.create<str>()
function routeDeco(path: str) {
    return (cls: ClassRef) => { RouteKey.set(cls, path) }
}

@routeDeco("/api/items")
class ItemController {}

assert("meta route", RouteKey.get(ItemController) === "/api/items")
```

### Decoradores de métodos

```varn
function logMethod(fn: FunctionRef, ctx: MethodContext): FunctionRef {
    const name = ctx.name
    return (...args: int[]) => {
        method_log.push(name + ":static=" + ctx.isStatic)
        return fn(...args)
    }
}

class DecoratedCalc {
    @logMethod
    mul(a: int, b: int): int { return a * b }
}
assert("method deco result", (new DecoratedCalc()).mul(3, 5) === 15)
```

---

## 9. `with` Expressions (Mutación no-destructiva)

Clona un objeto o instancia de clase sobreescribiendo solo los campos especificados; el original permanece inmutable.

### En instancias de clase

```varn
class User(public id: int, public name: str, public role: str = "guest") {}
// tests/89-raw-strings-and-with-expressions.vn

let u1 = new User(10, "Alice", "admin")
let u2 = u1 with { name: "Bob", role: "editor" }

assert("u2 id",    u2.id === 10)         // preservado de u1
assert("u2 name",  u2.name === "Bob")    // sobreescrito
assert("u2 role",  u2.role === "editor") // sobreescrito
assert("u1 inmutable", u1.name === "Alice")
```

### En objetos planos

```varn
let baseConfig = { host: "localhost", port: 3000, debug: true }
let prodConfig = baseConfig with { port: 8080, debug: false }

assert("host preservado", prodConfig.host === "localhost")
assert("port nuevo",      prodConfig.port === 8080)
assert("baseConfig intacto", baseConfig.port === 3000)
```

### Cadenas de `with`

```varn
class ServerNode(public host: str, public port: int, public isMaster: bool = false) {}

let node1 = new ServerNode("10.0.0.1", 5000, false)
let node2 = node1 with { port: 5001 }
let node3 = node2 with { isMaster: true }

assert("node3 host",     node3.host === "10.0.0.1")   // heredado de node1 via node2
assert("node3 port",     node3.port === 5001)
assert("node3 isMaster", node3.isMaster === true)
```

---

## 10. Errores y Excepciones Personalizadas

```varn
class ValidationError extends Error {   // tests/11-errors.vn:44
    field: str
    constructor(field: str, msg: str) {
        super(msg)
        this.name = "ValidationError"
        this.field = field
    }
}

try {
    throw new ValidationError("email", "invalid format")
} catch (e) {
    if (e instanceof ValidationError) {
        assert("custom error name",    e.name === "ValidationError")
        assert("custom error message", e.message === "invalid format")
        assert("custom error field",   e.field === "email")
        assert("instanceof Error",     e instanceof Error)
    }
}
```

---

## 11. Patrón Builder (Método Chaining)

```varn
class Builder {                       // tests/23-objects.vn:2
    private parts: str[]
    constructor() { this.parts = [] }
    add(s: str): Builder {
        this.parts.push(s)
        return this
    }
    build(): str { return this.parts.join(" ") }
}

const result = new Builder().add("hello").add("from").add("builder").build()
assert("builder pattern", result === "hello from builder")
```
