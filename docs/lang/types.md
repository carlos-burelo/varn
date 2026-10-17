# Varn Language — Sistema de Tipos

> Fuentes: `tests/08-null-safety.vn`, `tests/12-classes.vn`, `tests/13-interfaces.vn`, `tests/14-generics.vn`, `tests/15-unions.vn`, `tests/16-enums.vn`, `tests/24-record.vn`, `tests/26-numeric-coercion.vn`, `tests/30-nullable-types.vn`, `tests/34-char-type.vn`, `tests/35-decimal-bigint.vn`, `tests/36-advanced-generics.vn`, `tests/41-advanced-enums.vn`, `tests/69-tuples-records.vn`, `tests/79-control-flow-narrowing.vn`, `tests/87-primary-constructors-nullish-assign.vn`.

---

## 1. Tipos Primitivos

| Tipo | Descripción | Ejemplo literal |
|------|-------------|----------------|
| `int` | Entero con signo de 64 bits; `+ - * / % **` y `-x` lanzan `IntegerOverflow` fuera de rango | `42`, `-7`, `0` |
| `float` | IEEE 754 binary64 (`-0`, `NaN`, `±Infinity`) | `3.14`, `1.0`, `-0.5` |
| `decimal` | Decimal de precisión arbitraria; `+ - *` exactos, `/` a 34 dígitos significativos (half-even) | `1.5d`, `99.25d` |
| `bigint` | Entero de precisión arbitraria; `/` trunca y `%` toma el signo del dividendo, como `int` | `100n`, `0xffn` |
| `bool` | Booleano | `true`, `false` |
| `char` | Carácter Unicode | `'a'`, `'Z'`, `'!'` |
| `str` | Cadena de texto inmutable | `"hello"`, `"""raw"""` |
| `null` | Ausencia de valor | `null` |

### 1.1 Tipos numéricos

Varn tiene exactamente cuatro: `int`, `float`, `bigint`, `decimal`
(`docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md` §2–§11, ADR-0015). No existen
`i8`…`u64`, `f32`, `double` ni `number`: un ancho físico más estrecho es
decisión del compilador, no un tipo.

- **División**: `int / int` es `int` truncado hacia cero; `x / 0` y `x % 0`
  lanzan `DivisionByZero`; `int.MIN_VALUE / -1` lanza `IntegerOverflow`;
  `int.MIN_VALUE % -1` es `0`. `%` toma el signo del dividendo; `x.mod(y)` es
  euclídeo. `float / 0.0` da `±Infinity` o `NaN` (IEEE), nunca lanza.
- **API de enteros**: `wrappingAdd/Sub/Mul`, `saturatingAdd/Sub/Mul`,
  `checkedAdd/Sub/Mul` (→ `int?`), `div`, `floorDiv`, `ceilDiv`, `rem`, `mod`.

Todos son tipos canónicos únicos; **nunca** se aceptan aliases como `string`, `boolean`, `integer`.

---

## 2. Tipos Compuestos

### 2.1 Arrays

```varn
const nums: int[] = [3, 1, 4, 1, 5]       // tests/05-arrays.vn
let explicitGeneric: Array<int> = [10, 20, 30]  // tests/05-arrays.vn
let explicitBracket: int[] = explicitGeneric     // Asignable mutuamente
let nested: int[][] = [[1, 2], [3, 4]]           // 2D array
```

`Array<T>` y `T[]` son semánticamente idénticos y asignables bidireccionalmente.

### 2.2 Tuples (`#[…]`)

Colecciones heterogéneas inmutables con igualdad estructural:

```varn
let t1 = #[1, "hello", true]    // tests/69-tuples-records.vn
assertTrue("tuple elem 0", t1[0] == 1)
assertTrue("tuple length", t1.length == 3)

let t2 = #[1, "hello", true]
assertTrue("tuple eq true", t1 == t2)   // igualdad estructural profunda
```

### 2.3 Records (`#{…}`)

Objetos inmutables con igualdad estructural:

```varn
let r1 = #{ name: "Varn", version: 1 }   // tests/69-tuples-records.vn
assertTrue("record field name",    r1.name == "Varn")
assertTrue("record index name",    r1["name"] == "Varn")

let r2 = #{ name: "Varn", version: 1 }
assertTrue("record eq true", r1 == r2)       // igualdad estructural
```

### 2.4 Diccionarios (`{ [key: K]: V }`) y mapas (`Map<K, V>`)

Contenedores asociativos dinámicos mutables (`tests/17-map-set.vn`,
`tests/24-record.vn`):

```varn
// Diccionario: firma de índice { [key: K]: V } + literal de objeto
const scores: { [key: str]: int } = {   // tests/24-record.vn
    "alice": 95,
    "bob": 87,
    "charlie": 72
}
assertTrue("map get alice",   scores["alice"] === 95)
assertTrue("map get bob",     scores["bob"] === 87)
assertTrue("map get charlie", scores["charlie"] === 72)

scores["dave"] = 60               // mutación indexada
assertTrue("map set dave",    scores["dave"] === 60)
```

`Map<K, V>` es una colección aparte (`tests/17-map-set.vn`):

```varn
const m = new Map<str, int>()
m.set("a", 1)
assertTrue("map get", m.get("a") === 1)
assertTrue("map size", m.size === 1)
```

> [!NOTE]
> No son intercambiables: `{ [key: K]: V }` baja a `BackendTy::Dict` (objeto con claves dinámicas) y `Map<K, V>` a `BackendTy::Map`. Asignar un literal de objeto a un `Map<K, V>` es error de tipos (VN3001).
> La sintaxis `Record<K, V>` está estrictamente prohibida: la palabra `Record` se reserva exclusivamente para registros inmutables por valor (`#{}`).

### 2.5 Matriz Comparativa Canónica de Tipos Contenedor

| Tipo | Sintaxis de Creación | Sintaxis de Acceso | Igualdad | Mutabilidad |
|---|---|---|---|---|
| **`Object`** | `{ name: "Alice", age: 30 }` | `obj.name` (offset de compilación fijo) | Referencia (`===`) | Mutable |
| **`Record`** | `#{ name: "Alice", age: 30 }` | `rec.name` | Estructural profunda (`==`) | Inmutable |
| **`Tuple`** | `#[ 1, "hello" ]` | `tup[0]` | Estructural profunda (`==`) | Inmutable |
| **`Map<K, V>`** | `new Map<K, V>()` | `m[k]`, `m.get(k)`, `m.set(k, v)` | Referencia (`===`) | Mutable |
| **`{ [key: K]: V }`** | `{ "a": 1 }` | `d[k]` | Referencia (`===`) | Mutable |

### 2.6 Set

```varn
const set = new Set<str>()
set.add("x")
set.add("x")     // duplicado ignorado
assertTrue("set size dedup", set.size === 2)
```

---

## 3. Tipos Nulables

El sistema de tipos distingue entre un tipo no-nulo y su versión nulable:

```varn
const a: str? = null       // str | null  (tests/30-nullable-types.vn)
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
type StringOrInt = str | int    // tests/15-unions.vn
type MaybeStr = str | null

function processValue(v: StringOrInt): str {
    if (v instanceof str) {
        return "string: " + v
    } else {
        return "number: " + v
    }
}
assertTrue("union str branch", processValue("hello") === "string: hello")
assertTrue("union int branch", processValue(42) === "number: 42")
```

### Tipos unión de clases

```varn
type Shape2 = Square2 | Circle2    // tests/15-unions.vn

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
enum Direction { North, South, East, West }    // tests/16-enums.vn

const dir: Direction = Direction.North
assertTrue("enum value",    dir === Direction.North)
assertTrue("enum rawValue", Direction.East.rawValue === 2)
```

### 5.2 Enums con valores explícitos

```varn
enum HttpMethod { GET = 0, POST = 1, PUT = 2, DELETE = 3 }   // tests/16-enums.vn
assertTrue("enum explicit", HttpMethod.POST.rawValue === 1)
```

### 5.3 Enums con payload (ADTs)

```varn
enum Token {                          // tests/41-advanced-enums.vn
    Identifier(name: str),
    Number(value: float),
    String(value: str),
    Plus,
    Minus
}

const t1 = Token.Number(123)
const t2 = Token.Identifier(name: "hello")   // named argument
assertTrue("payload value", t1.value === 123)
assertTrue("named payload", t2.name === "hello")
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
assertTrue("shared field code Ok",     HttpStatus.Ok.code === 200)
assertTrue("shared field message",     HttpStatus.Ok.message === "OK")
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
assertTrue("circle area", Shape.Circle(10.0).area() === 314.159)
assertTrue("rect area",   Shape.Rectangle(10.0, 20.0).area() === 200.0)
```

### 5.6 Enums genéricos

```varn
enum Result<T, E> {
    Ok(value: T),
    Err(error: E)
}
const resOk = Result.Ok<int, str>(42)
```
// `tests/41-advanced-enums.vn`. Enums que implementan interfaces: NO-VERIFICADO.

---

## 6. Interfaces

```varn
interface Printable {            // tests/13-interfaces.vn
    toString(): str
}
```
// Implementación múltiple, campos opcionales y resto de interfaces:
// solo lo citado aquí es contrato; ver `tests/13-interfaces.vn`.

---

## 7. Clases

Ver también [`classes_objects.md`](classes_objects.md).

```varn
class Animal {                        // tests/12-classes.vn
    name: str
    constructor(n: str) { this.name = n }
    speak() { return "..." }
}
class Dog extends Animal {
    constructor(n: str) { super(n) }
    override speak() { return "Woof" }
}
```

Contratos que se comprueban en la declaración:

- `override` debe poder ocupar el lugar de lo que redefine: mismo tipo
  de retorno o uno más estrecho, parámetros iguales o más anchos. Si no,
  error `VN4116`.
- `implements I` exige que la clase satisfaga `I`: error `VN4117` si
  falta un miembro o su tipo no encaja.
- Una clase que no es `abstract` implementa todo miembro abstracto que
  hereda: error `VN4107`.
- Un miembro `static` se alcanza por la clase (`Counter.total`), nunca
  por una instancia ni por `this`; uno de instancia, nunca por la clase.
  Error `VN4106` en los dos sentidos.
- El constructor de una subclase llama a `super(...)`: error `VN4108` si
  falta. Sus argumentos se validan contra el constructor del padre.
- Dos miembros de una clase no comparten nombre (`VN4110`), salvo el par
  getter y setter. Estático e instancia son espacios distintos.
- Una propiedad con getter y sin setter no se asigna, y una con setter y
  sin getter no se lee (`VN3005`).
- `super` solo llama métodos del padre (`super.nombre(...)`), y solo
  directamente en un método o constructor: `super.prop` es `VN4109` y
  `super` dentro de una flecha o función anidada es `VN4114`.

---

## 8. Generics

```varn
class Box<T> {                        // tests/14-generics.vn
    value: T
    constructor(v: T) { this.value = v }
    get(): T { return this.value }
    map<U>(f: (T) => U): Box<U> { return new Box<U>(f(this.value)) }
}

function identity<T>(v: T): T { return v }
function swap<A, B>(a: A, b: B): B { return b }

const intBox = new Box<int>(10)
assertTrue("generic box get", intBox.get() === 10)
const strBox = intBox.map((n) => `value=${n}`)
assertTrue("generic box map", strBox.get() === "value=10")
```

Interfaces genéricas:

```varn
interface Comparable<T> {
    compareTo(other: T): int
}
```

### 8.1 Argumentos de tipo de `new`

`new` deduce los argumentos de tipo de los argumentos del constructor,
igual que una llamada genérica (tests/190-generic-instantiation-from-new.vn):

```varn
const b = new Box(1)              // Box<int>
const p = new Pair(1, "uno")      // Pair<int, str>
const t = new Tagged(5, 7)        // label: L? con un int fija L = int
```

- Lo que no se deduce toma el valor por defecto declarado
  (`class Cell<T = int>`; `new Cell()` es `Cell<int>`).
- Si queda un parámetro de tipo abierto es error `VN3201`: se escribe
  (`new Stack<int>()`) o se da el tipo esperado (`const s: Stack<int> = new Stack()`).
  `new Map()` y `new Set()` no son excepción.
- El constructor se valida siempre contra los tipos ya instanciados.

### 8.2 Varianza automática

Sin anotaciones: el compilador mide dónde aparece cada parámetro dentro de
los miembros y decide (tests/199-containers-are-invariant.vn,
tests/errors/generator-narrowing-rejected.vn):

- solo salidas (`get(): T`, getters, campos `readonly`) → covariante:
  `Task<int>` entra donde `Task<int?>`
- solo entradas (parámetros) → contravariante
- ambas o campo mutable → invariante: `Box<int>` no entra donde `Box<str>`,
  `Generator<int>` no entra donde `Generator<int | str>` (su `return(v?: T)`
  acepta `T`: ensancharlo rompería el cuerpo)
- sin usos (phantom) → covariante
- arrays y genéricos anidados cuentan como ambas posiciones (invariantes);
  uniones, tuplas y objetos `readonly` son transparentes
- `dynamic` escapa en ambas direcciones (explícito)

Si una asignación falla por invarianza, el culpable es siempre un miembro
concreto que usa `T` en mala posición. Válvula de escape: el cast `as`
existente. La medición es determinista y viaja con el símbolo (igual al
importar que al declarar).

---

## 9. Conversiones numéricas

Implícitas solo `int → bigint` e `int → decimal` (exactas). Un **literal**
entero adopta `float`/`decimal`/`bigint` del contexto si es exactamente
representable (`|n| ≤ 2^53` para `float`). Todo lo demás se escribe con `as`.

```varn
const i: int = 42
const d: decimal = i          // implícito: exacto
const bi: bigint = i          // implícito: exacto
const f: float = 5            // literal: adopta float
const g: float = i as float   // variable: explícito
const h = 2.5 * 2             // literal: float * float
// const bad: float = i       // error: int → float no es implícito
// const mix = i + 2.5        // error: int + float
```

| `as` | Comportamiento |
|---|---|
| `int as float` | redondeo al `float` más cercano |
| `float as int` | trunca hacia cero; `NaN`, `±Infinity` o fuera de rango lanzan `IntegerOverflow` |
| `bigint as int`, `decimal as int` | exacto / trunca; fuera de rango lanza `IntegerOverflow` |
| `bigint as float`, `decimal as float` | el `float` más cercano |
| `float as bigint` | trunca; `NaN`/`±Infinity` lanzan `IntegerOverflow` |
| `float as decimal` | valor binario exacto del `float` (`0.1 as decimal` no es `0.1d`) |
| `bigint as decimal`, `decimal as bigint` | exacto / trunca |
| `dynamic as int`, `dynamic as float` | convierte según el valor en runtime |

Con `dynamic` la aritmética mezclada `int ⊕ float` se resuelve en runtime a
`float`; `int / int` sigue siendo `int`.

---

## 10. Control-Flow Type Narrowing

El checker estrecha automáticamente el tipo dentro de bloques condicionales:

```varn
function processOptional(val: str?): int {
    if (val !== null) {
        return val.length   // val se estrecha a 'str' aquí (tests/79-control-flow-narrowing.vn)
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

### 10.1 Veracidad: solo `null` y `false` son falsos

No hay tabla de valores falsos. `0`, `0.0`, `NaN`, `""` y `[]` son
verdaderos (tests/186-truthiness-and-logical-values.vn).

- Con `x: T?`, `if (x)` es exactamente `x != null`: la rama verdadera ve
  `T` y la falsa ve `null`.
- Un valor cuyo tipo no admite `null` ni `false` (`int`, `str`, un array,
  una instancia) no puede ser condición: error `VN3027`. Se compara
  explícitamente (`n != 0`, `s.length > 0`).
- `a || b` devuelve `a` si no es `null`/`false`; `a && b` devuelve `b` solo
  en ese caso. `count || 10` conserva un `0`.

### 10.2 Índices y claves

- `xs[i]`, `s[i]`, `bytes[i]` y `rango[i]` tienen tipo `T`: el índice
  afirma que el elemento existe. Fuera de rango, o negativo, lanza
  `RangeError`, también al escribir (tests/184-string-index.vn). Un array
  crece con `push`; el acceso que puede fallar es `at(i)`, que devuelve `T?`.
- `m[k]` sobre `Map<K, V>` o `{ [key: K]: V }` tiene tipo `V?`: que la
  clave falte es un caso normal (tests/187-keyed-index-and-optional-members.vn).
  La escritura `m[k] = v` pide `V`.
- Un miembro o parámetro opcional (`tag?: str`) es `str?`: omitirlo y pasar
  `null` son lo mismo (tests/188-call-shape-and-optional-parameters.vn).

### 10.3 El estrechamiento sigue a la asignación

Un estrechamiento vale mientras la variable no cambie
(tests/192-narrowing-follows-assignment.vn):

- Asignar a una variable deshace lo que se sabía de ella. Tras
  `if (x == null) return`, hacer `x = null` devuelve `x` a `T?`.
- Un bucle olvida, antes de empezar, lo que su cuerpo reasigna: lo
  comprobado antes del bucle no vale para la segunda vuelta.
- Tras `x = v`, `x ??= v` o `x ||= v` con `v` no nulo, `x` es `T` en el
  resto del bloque.
- Es conservador: una asignación en una rama de un `if` invalida también
  la otra. No alcanza a miembros (`this.v ??= 3`) ni a asignaciones hechas
  dentro de un closure.

### 10.4 Retorno

- Una función, método, getter o flecha con tipo de retorno declarado
  retorna por toda ruta: error `VN4018` si el cuerpo puede terminar sin
  `return` ni `throw`. Vale también para `T?`: el `null` se escribe.
- Una función `never` no puede llegar a su final.
- La anotación de un generador nombra lo que produce. `function* g(): int`
  y `function* g(): Generator<int>` son lo mismo, y cada `yield` se
  comprueba contra ese tipo (tests/194-generator-declared-yield.vn).
- Una restricción `T extends X` se comprueba contra el argumento de tipo
  escrito y contra el que se deduce de la llamada (`VN3202`).

### 10.5 Operadores, patrones y namespaces

- `<`, `<=`, `>` y `>=` entre strings comparan sus puntos de código, en
  orden (tests/196-string-order.vn). `==` entre arrays u objetos es
  identidad.
- `-`, `+` y `~` unarios, y `++`/`--`, exigen un número (o un tipo que
  sobrecargue el operador). No hay conversión: `-"a"` es error.
- Un patrón de `match` debe poder coincidir con el valor: un literal de
  otro tipo, o una variante con más nombres que valores lleva, es
  `VN4015`.
- Desestructurar pide que el campo exista (`VN3004`), salvo que el patrón
  traiga valor por defecto; una tupla da a cada nombre el tipo de su
  posición y no admite más nombres que elementos (`VN3016`).
- Namespaces: ver §10.7.

- `switch`: cada caso corre solo, nunca cae al siguiente
  (tests/197-switch-cases.vn). `break` puede cerrar un caso como último
  enunciado y sale del `switch`, no del bucle que lo rodea; en otro punto
  del caso es `VN4005`. Un caso vacío o un `default` que no es el último
  son `VN4019`. Para compartir un cuerpo entre valores: `match` con
  `a | b`.
- `break` y `continue` actúan sobre el bucle más interno, o sobre el
  bucle que nombra su etiqueta (`fuera: for (...) { ... break fuera }`,
  tests/201-loop-labels.vn). Una etiqueta que no nombra un bucle que
  rodea al salto es error.
- Cada vuelta de un `for (let i ...)` tiene su propia `i`, igual que en
  `for-of`: una closure creada en la vuelta ve el valor de esa vuelta
  (tests/198-loop-variable-per-iteration.vn).
- `x!` afirma que `x` no es `null` y lo comprueba al ejecutar: si lo es,
  lanza. No es una forma de saltarse el tipo.
- Un argumento nombrado debe nombrar un parámetro, y una sola vez.
- Descartar un tipo con `typeof` deja los demás: tras
  `if (typeof x == "int") { return x }`, un `x: int | str` es `str`.

### 10.7 Namespaces

Un namespace es un ámbito con nombre. No existe al ejecutar: cada
miembro es un global del módulo y `N.miembro` se resuelve al compilar
(tests/195, 202 y 203).

- Solo se ve desde fuera lo que marca `export`. Lo demás es privado y lo
  leen las funciones y clases del propio namespace, por su nombre simple.
- **Un namespace no es un valor** (`VN4020`): `const g = N` o pasarlo como
  argumento es error. Sí lo es cada miembro: `const f = N.area`.
- **Un miembro no se asigna desde fuera** (`VN3005`). El estado privado
  de un namespace se cambia con sus funciones.
- Los nombres son del namespace, no del módulo: dos namespaces pueden
  declarar la misma función, variable, clase, enum, interfaz, alias o
  namespace anidado, y son cosas distintas.
- Un tipo se nombra por su namespace: `x: Geo.Box`, `Geo.Kind`,
  `class Crate extends Geo.Box`. Dentro del namespace basta el nombre
  simple.
- Entre módulos se comporta igual: `import { Geo }`, un renombre
  (`export { Geo as Shapes } from`), `export *` y `import * as L`
  (`L.Geo.area`) llegan al mismo namespace, con el mismo estado.

### 10.8 Lo exportado es constante

`export let` es error (`VN4021`), en un módulo y en un namespace, igual
que `export { x }` de un `let`. El módulo que importa recibe el valor,
no la variable: una exportación mutable se leería desde fuera sin ver
nunca sus cambios. El estado compartido se exporta como funciones que
lo leen y lo cambian.

### 10.9 Varianza, nulos y `super`

- `T[]`, `Map<K, V>`, `Set<T>` y los campos mutables son invariantes: un
  `Dog[]` no es un `Animal[]`, porque por la segunda vista se podría
  guardar un `Cat` (tests/199-containers-are-invariant.vn). Un literal
  sí se adapta al tipo que se le pide. `Task`, `Generator` y los campos
  `readonly` son covariantes.
- Dentro de una función genérica, `T` es opaco: no se le puede dar un
  valor concreto ni devolver otra cosa donde se prometió `T`.
- `??`, `?.` y `== null` sobre un valor que nunca es `null` son
  error (`VN3028`): el código dice que algo puede faltar y el tipo dice
  que no.
- `super.m()` y `super.x` llegan al padre también desde una flecha, un
  getter o el inicializador de un campo
  (tests/200-super-reaches-the-parent.vn).

### 10.6 Valores por defecto y spread

- El valor por defecto de un parámetro se comprueba contra su tipo, dentro
  del ámbito de la función (puede nombrar parámetros anteriores).
- Un literal entero vale donde se declara `float`, `decimal` o `bigint`, en
  un inicializador, un argumento, un valor por defecto, una asignación o un
  `yield`, y llega convertido: en `f(x: float = 1)`, `x` es siempre `float`.
  Una variable `int` no se convierte sola a `float`.
- `[...xs]` y `f(...xs)` aceptan un array o una tupla. Un `Set`, un `Map`,
  un `str`, un generador o un valor que puede ser `null` son error de
  compilación (tests/191-spread-sources.vn).

---

## 11. Tipo `dynamic`

Tipo dinámico para valores cuya naturaleza se determina en tiempo de ejecución:

```varn
function processTypeof(val: dynamic): str {    // tests/79-control-flow-narrowing.vn
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
const r1 = 0..5     // rango exclusivo [0, 5) (tests/18-ranges.vn)
const r2 = 1..=5    // rango inclusivo [1, 5]

assertTrue("range start",       r1.start === 0)
assertTrue("range end excl",    r1.end === 5)
assertTrue("range length excl", r1.length === 5)
assertTrue("range contains",    r1.contains(3))
assertTrue("range incl",        r2.contains(5))

const arr = (0..4).toArray()     // [0, 1, 2, 3]
const stepped = (0..10).step(3)  // [0, 3, 6, 9]
const r3 = Range.from(5, 10)
```

Indexación directa con rangos en strings y arrays:

```varn
let text = "Hello, World!"
let greeting = text[0..5]     // "Hello"  (tests/88-range-indexing.vn)
let greetingInc = text[0..=4] // "Hello"

let numbers = [10, 20, 30, 40, 50, 60]
let slice = numbers[1..4]     // [20, 30, 40]
let slice2 = numbers[1..=3]   // [20, 30, 40]
```

