# Varn Language — Expresiones

> Fuentes: `tests/01-arithmetic.vn`, `tests/04-templates.vn`, `tests/05-arrays.vn`, `tests/06-destructuring.vn`, `tests/07-closures.vn`, `tests/08-null-safety.vn`, `tests/10-match.vn`, `tests/14-generics.vn`, `tests/18-ranges.vn`, `tests/19-pipeline.vn`, `tests/29-option-destructuring.vn`, `tests/37-complex-closures.vn`, `tests/38-advanced-higher-order.vn`, `tests/41-advanced-enums.vn`, `tests/69-tuples-records.vn`, `tests/86-tagged-templates.vn`, `tests/87-primary-constructors-nullish-assign.vn`, `tests/88-range-indexing.vn`, `tests/89-raw-strings-and-with-expressions.vn`.

---

## 1. Expresiones Literales

Ver [`lexical.md`](lexical.md) para la especificación completa de literales.

```varn
42          // int
3.14        // float
1.5d        // decimal
100n        // bigint
true        // bool
'a'         // char
"hello"     // str
"""raw"""   // raw string
null        // null
```

---

## 2. Variables

```varn
let x = 42          // mutable
const y = "hello"   // inmutable (alias: const)
var z = true        // mutable (alias: var)

let typed: int = 100
const explicit: str = "world"
```

---

## 3. Operadores y Precedencia

De mayor a menor precedencia:

| Nivel | Operadores |
|-------|-----------|
| 1 | `()` (agrupación), `[]`, `.`, `?.`, `!` |
| 2 | `**` (exponenciación, derecha-a-izquierda) |
| 3 | `-` (unario), `~`, `!` |
| 4 | `*`, `/`, `%` |
| 5 | `+`, `-` |
| 6 | `<<`, `>>` |
| 7 | `&` |
| 8 | `^` |
| 9 | `\|` |
| 10 | `<`, `<=`, `>`, `>=` |
| 11 | `===`, `!==`, `==`, `!=` |
| 12 | `&&` |
| 13 | `\|\|` |
| 14 | `??` |
| 15 | `?:` (ternario) |
| 16 | `\|>` (pipeline) |
| 17 | `=`, `??=` |

```varn
assert("precedence *+",    2 + 3 * 4 === 14)    // tests/01-arithmetic.vn:12
assert("precedence parens",(2 + 3) * 4 === 20)
```

---

## 4. Literals de Array

```varn
const nums = [3, 1, 4, 1, 5, 9]     // tests/05-arrays.vn:2
const spread = [...[1, 2], ...[3, 4]]   // spread
let explicitGeneric: Array<int> = [10, 20, 30]

// Acceso por índice
assert("index", nums[0] === 3)

// Acceso por rango
let slice = nums[1..4]    // [1, 4, 1]  (tests/88-range-indexing.vn)
let sliceInc = nums[1..=4]
```

---

## 5. Literals de Objeto

```varn
const obj1 = { a: 1, b: 2, c: 3 }    // tests/06-destructuring.vn:2
const base = { x: 1, y: 2 }
const ext  = { ...base, z: 3 }        // spread en objeto
```

---

## 6. Tuplas y Records (Inmutables)

```varn
let t = #[1, "hello", true]       // Tuple  (tests/69-tuples-records.vn:4)
let r = #{ name: "Varn", v: 1 }   // Record

// Igualdad estructural profunda con ==
let t2 = #[1, "hello", true]
assert("tuple eq", t == t2)
let r2 = #{ name: "Varn", v: 1 }
assert("record eq", r == r2)
```

---

## 7. Llamadas a Funciones y Métodos

```varn
// Positional
assert("int add", 1 + 2 === 3)
identity<int>(42)        // con parámetro de tipo explícito

// Named arguments
describe(name: "Alice", age: 30, city: "London")   // tests/40-named-arguments.vn:11
describe(age: 25, name: "Bob")                      // fuera de orden
describe("Charlie", age: 40)                         // mezcla posicional y nombrado

// Method chaining
new Builder().add("hello").add("world").build()
```

---

## 8. Destructuring

### De objetos

```varn
const obj1 = { a: 1, b: 2, c: 3 }
const { a, b } = obj1                         // extracción directa
const { a: renamed } = obj1                   // renombrado
const { name = "default", age = 0 } = { name: "Alice" }  // default values
const { point: { x: nx, y: ny } } = nested    // nested destructuring
```

### De arrays

```varn
const [f1, f2, f3] = [10, 20, 30]
const [skip1, _, skip3] = [100, 200, 300]    // _ descarta elemento
const [head, ...tail] = [1, 2, 3, 4]         // rest
const { coords: [cx, cy] } = { coords: [5, 6] }  // array en objeto
```

---

## 9. Closures y Lambdas

```varn
// Lambda expresión
const double = (n: int) => n * 2
const inc    = (n: int) => n + 1

// Lambda con bloque
const square = (n: int) => {
    return n * n
}

// Closure captura variables del ámbito
function makeAdder(n: int): (a: int) => int {
    return (x: int) => x + n   // captura 'n'
}
const add5 = makeAdder(5)
assert("closure add5", add5(3) === 8)   // tests/07-closures.vn:7

// Closure mutable
function makeCounter(): () => int {
    let c = 0
    return () => {
        c = c + 1
        return c
    }
}
const cnt1 = makeCounter()
assert("counter1 first",  cnt1() === 1)
assert("counter1 second", cnt1() === 2)
```

### Composición de funciones

```varn
function compose<A, B, C>(f: (a:B) => C, g: (b:A) => B): (c:A) => C {
    return (x: A) => f(g(x))
}
const doubleInc = compose(double, inc)
assert("compose", doubleInc(4) === 10)   // tests/07-closures.vn:30
```

---

## 10. Match Expressions

```varn
function describeNum(n: int): str {
    return match (n) {        // tests/10-match.vn:3
        0 => "zero",
        1 => "one",
        2 | 3 => "two or three",
        _ => "other"
    }
}
assert("match 2",     describeNum(2) === "two or three")
assert("match other", describeNum(99) === "other")
```

Match sobre enums con payload:

```varn
enum R { A(int), B(str) }

let x = R.A(42)
let v = match x {               // tests/29-option-destructuring.vn:5
    A(n) => n,
    B(_) => -1,
}
assert("match ok", v === 42)
```

Match en métodos de enum:

```varn
area(): float {
    return match (this) {
        Circle(radius)           => 3.14159 * radius * radius,
        Rectangle(width, height) => width * height
    }
}
```

---

## 11. Pipeline Operator (`|>`)

```varn
function double2(n: int): int { return n * 2 }
function addN(n: int, x: int): int { return n + x }

assert("pipe simple",      5 |> double2 === 10)            // tests/19-pipeline.vn:10
assert("pipe placeholder", 7 |> addN(_, 3) === 10)
assert("pipe multi _",     4 |> addN(_, _) === 8)
assert("pipe clamp lo",    -5 |> clamp2(0, 10, _) === 0)

const piped = (3 |> double2) |> double2
assert("pipe chained", piped === 12)
```

---

## 12. `with` Expressions

```varn
let u1 = new User(10, "Alice", "admin")
let u2 = u1 with { name: "Bob", role: "editor" }  // tests/89-raw-strings-and-with-expressions.vn:31
```

Ver [`classes_objects.md`](classes_objects.md) §9 para detalles completos.

---

## 13. Null Coalescing (`??`) y Optional Chaining (`?.`)

```varn
assert("?? left null",     (null ?? 99) === 99)      // tests/08-null-safety.vn:19
assert("?. non-null",      p1?.address?.city === "NY")
assert("?. + ??",          (p3?.name ?? "anon") === "anon")
assert("?? chained",       (null ?? null ?? 7) === 7)
```

---

## 14. Null-Coalescing Assignment (`??=`)

```varn
let a: int? = null
a ??= 42     // asigna solo si es null (tests/87-primary-constructors-nullish-assign.vn:75)
a ??= 999    // no hace nada: a ya es 42

let cfg = new AppConfig()
cfg.timeout ??= 3000     // propiedad de objeto
cfg.timeout ??= 8000     // no cambia

let items: dynamic[] = [null, "existing", null]
items[0] ??= "filled_0"  // índice de array
items[1] ??= "overwritten_1"   // no cambia: "existing"
```

---

## 15. Template Literals y Tagged Templates

```varn
// Interpolación estándar
const tval = 42
assert("template", `answer = ${tval}` === "answer = 42")  // tests/04-templates.vn:3

// Tagged template: función + backtick
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
assert("tagged template", output === "Language: [Varn], Version: [1]!")   // tests/86-tagged-templates.vn:37

// Tagged template con método de clase
let formatted = fmt.format`Player ${user} scored ${score} points`

// SQL seguro con tagged templates
let rows = db.sql`SELECT * FROM users WHERE role = ${targetRole} AND active = ${isActive}`
```

---

## 16. `instanceof` y `typeof`

```varn
assert("instanceof Dog",  pd instanceof Dog)           // tests/12-classes.vn:56
assert("instanceof Error", ve instanceof Error)

// typeof devuelve el nombre canónico del tipo como str
assert("typeof str",  typeof "abc" === "str")          // tests/79-control-flow-narrowing.vn:39
assert("typeof int",  typeof 42 === "int")
assert("typeof bool", typeof true === "bool")
assert("typeof class", typeof WebSocketClient === "class")
```

---

## 17. `is` (Type Assertion / Narrowing)

```varn
function describeChar(c: char): str {
    if (c is char) return "yes"   // tests/34-char-type.vn:38
    return "no"
}
function isDecimalType(v: decimal): bool { return v is decimal }
function isBigInt(v: bigint): bool { return v is bigint }
```

---

## 18. Cast Explícito (`as`)

```varn
let jsonObj = JSON.parse("{\"message\": \"hello\", \"status\": 200}")
let status = (jsonObj as dynamic).status    // tests/82-std-csv.vn:81
assertEqual(status, 200)
```

---

## 19. `new` — Creación de Instancias

```varn
const t = new Temperature(100.0)
const stk = new GenericStack<int>()
const list = new LinkedList<int>()
const either = Either.left<str, int>("error")
const db = new Database(":memory:")
```

---

## 20. Operador Ternario

```varn
const label = n < 0 ? "neg" : n === 0 ? "zero" : "pos"  // tests/09-control-flow.vn:3
```

---

## 21. Spread (`...`) en Expresiones

```varn
const spread = [...[1, 2], ...[3, 4]]          // array spread
const ext    = { ...base, z: 3 }               // object spread
function rolesDeco(...r: str[]) { /* rest params */ }   // rest parameters
```
