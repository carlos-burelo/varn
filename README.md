# Varn

[![License](https://img.shields.io/badge/License-Apache--2.0-blue?style=for-the-badge)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust-orange?style=for-the-badge)](https://www.rust-lang.org/)

**Varn** es un lenguaje compilado con tipado estático y VM register-based de alto rendimiento escrita en Rust. Extensión de archivos: `.vn`.

---

## Características

- **VM Register-Based con NaN-Boxing** — valores en 64 bits, sin boxing en el hot path
- **Tipado estático expresivo** — uniones, genéricos, exhaustividad total en `match`
- **Async y concurrencia** — `async`/`await`, generadores, `TaskGroup`, `parallel`, `spawn` e isolates multi-thread
- **Sistema de paquetes** — `varn.json`, `varn.lock`, resolución semver sobre git
- **Compilado a `.vnc`** — artefacto portable, sin recompilación
- **Tooling** — `vn bench`, `vn debug -p bytecode`, `vn debug`, LSP

---

## El Lenguaje

### Variables, tipos, operadores

```Varn
const x: int = 42
let name: str = "Varn"
const flag: bool = true

// Operadores aritméticos
assert("power",    2 ** 10 === 1024)
assert("mod",      17 % 5 === 2)
assert("bitwise",  (12 & 10) === 8)
assert("shift",    1 << 4 === 16)

// String methods
const s = "  Hello, World!  "
assert("trim",        s.trim() === "Hello, World!")
assert("slice",       "hello world".slice(6, 11) === "world")
assert("replaceAll",  "foo bar foo".replaceAll("foo", "baz") === "baz bar baz")
assert("split",       "a,b,c".split(",")[1] === "b")
assert("padStart",    "5".padStart(3, "0") === "005")
```

### Control de flujo

```Varn
// for-of, for-in, while, break, continue
for (const n of [10, 20, 30]) {
    print(n)
}

let i = 0
while (i < 5) {
    if (i % 2 === 0) { i = i + 1; continue }
    print(i)
    i = i + 1
}

// Ternario anidado
function classify(n: int): str {
    return n < 0 ? "neg" : n === 0 ? "zero" : "pos"
}
```

### Match

```Varn
function describeNum(n: int): str {
    return match (n) {
        0       => "zero",
        1       => "one",
        2 | 3   => "two or three",
        _       => "other"
    }
}

// Match sobre enums
enum Direction { North, South, East, West }

function describeDir(d: Direction): str {
    return match (d) {
        Direction.North => "going north",
        Direction.South => "going south",
        Direction.East  => "going east",
        Direction.West  => "going west"
    }
}
```

### Funciones y closures

```Varn
function makeAdder(n: int): (a: int) => int {
    return (x: int) => x + n
}
const add5 = makeAdder(5)
assert("closure", add5(3) === 8)

// Composición genérica
function compose<A, B, C>(f: (B) => C, g: (A) => B): (A) => C {
    return (x: A) => f(g(x))
}
const double = (n: int) => n * 2
const inc    = (n: int) => n + 1
const doubleInc = compose(double, inc)
assert("compose", doubleInc(4) === 10)  // (4+1)*2 = 10
```

### Argumentos nombrados

```Varn
function describe(name: str, age: int, city: str | null = null): str {
    if (city == null) { city = "Unknown" }
    return name + " is " + age.toString() + " years old and lives in " + city
}

// En orden, fuera de orden, mixto positional+named
assert("in order",    describe(name: "Alice", age: 30, city: "London") == "Alice is 30 years old and lives in London")
assert("out of order", describe(age: 25, name: "Bob") == "Bob is 25 years old and lives in Unknown")
assert("mixed",        describe("Charlie", age: 40) == "Charlie is 40 years old and lives in Unknown")
```

### Clases y herencia

```Varn
abstract class Shape {
    abstract area(): float
    describe(): str { return `shape with area ${this.area()}` }
}

class Circle extends Shape {
    r: float
    constructor(r: float) { this.r = r }
    override area(): float { return 3.14159 * this.r * this.r }
}

class Animal {
    name: str
    constructor(n: str) { this.name = n }
    speak() { return "..." }
}
class Dog extends Animal {
    constructor(n: str) { super(n) }
    override speak() { return "Woof" }
}
class PoliceDog extends Dog {
    badge: int
    constructor(n: str, badge: int) { super(n); this.badge = badge }
    override speak() { return super.speak() + "!" }
}

assert("instanceof chain", pd instanceof Animal)  // true

// Getters y setters
class Temperature {
    private _celsius: float
    constructor(c: float) { this._celsius = c }
    get celsius() { return this._celsius }
    set celsius(v: float) { this._celsius = v }
    get fahrenheit() { return this._celsius * 1.8 + 32.0 }
}

// Estáticos
class IdGen {
    private static _next: int = 1
    static next(): int {
        const id = IdGen._next
        IdGen._next = IdGen._next + 1
        return id
    }
    static reset(): void { IdGen._next = 1 }
}
```

### Interfaces y tipado estructural

```Varn
interface Printable { toString(): str }
interface Serializable { serialize(): str }

class Config implements Printable, Serializable {
    key: str
    value: int
    constructor(k: str, v: int) { this.key = k; this.value = v }
    toString(): str { return `${this.key}=${this.value}` }
    serialize(): str { return `{"${this.key}":${this.value}}` }
}

// Opcionales en interfaces
interface Options {
    verbose?: bool
    maxRetries?: int
    tag: str
}
function runWith(opts: Options): str {
    const v = opts.verbose ?? false
    const r = opts.maxRetries ?? 3
    return `${opts.tag}:v=${v}:r=${r}`
}
assert("defaults", runWith({ tag: "job1" }) === "job1:v=false:r=3")
```

### Generics

```Varn
class Box<T> {
    value: T
    constructor(v: T) { this.value = v }
    get(): T { return this.value }
    map<U>(f: (T) => U): Box<U> { return new Box<U>(f(this.value)) }
}

class Either<L, R> {
    static left<L, R>(v: L): Either<L, R> { ... }
    static right<L, R>(v: R): Either<L, R> { ... }
    mapRight<T>(f: (R) => T): Either<L, T> { ... }
}

// HOF genérico
function pipe<A, B, C>(f: (A) => B, g: (B) => C): (A) => C {
    return (x: A) => g(f(x))
}
function memoize<T>(f: (int) => T): (int) => T { ... }
```

### Tipos unión

```Varn
type StringOrInt = str | int
type MaybeStr = str | null

function processValue(v: StringOrInt): str {
    if (v instanceof str) { return "string: " + v }
    else { return "number: " + v }
}

// Clases en unión
type Shape2 = Square2 | Circle2
function totalArea(shapes: Shape2[]): float {
    let total = 0.0
    for (const s of shapes) { total = total + s.area() }
    return total
}
```

### Enums

```Varn
enum Direction { North, South, East, West }
enum HttpMethod { GET = 0, POST = 1, PUT = 2, DELETE = 3 }

assert("rawValue",  Direction.East.rawValue === 2)
assert("explicit",  HttpMethod.POST === 1)
```

### Extensiones

```Varn
extension StringUtils on str {
    shout(): str { return this + "!" }
    times(n: int): str {
        let result = ""
        let i = 0
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

// Getters/setters en extensiones
extension SlotAccessor on Slot {
    get label(): str { return "Slot(" + this.value + ")" }
    set label(s: str) { this.value = s.length }
}

assert("shout",    "hello".shout() === "hello!")
assert("times",    "ab".times(3) === "ababab")
assert("isEven",   (4).isEven() === true)
assert("clamp",    (15).clamp(5, 10) === 10)
```

### Pipeline y placeholders

```Varn
function double(n: int): int { return n * 2 }
function addN(n: int, x: int): int { return n + x }
function clamp(lo: int, hi: int, v: int): int { ... }

assert("pipe simple",      5 |> double === 10)
assert("pipe placeholder", 7 |> addN(_, 3) === 10)
assert("pipe multi _",     4 |> addN(_, _) === 8)
assert("pipe clamp",       15 |> clamp(0, 10, _) === 10)
assert("pipe chained",     (3 |> double) |> double === 12)
```

### Generadores

```Varn
function* range_gen(start: int, end: int) {
    let i = start
    while (i < end) { yield i; i = i + 1 }
}

const gen = range_gen(0, 5)
assert("next value", gen.next().value === 0)
assert("done false", !gen.next().done)

function* fib_gen() {
    let a = 0; let b = 1
    while (true) {
        yield a
        const tmp = a + b; a = b; b = tmp
    }
}
```

### Async/Await

```Varn
import { sleep, TaskGroup, spawn, parallel } from "std:task"

async function asyncAdd(a: int, b: int): int { return a + b }

async function runAsync(): void {
    // await básico
    const r = await asyncAdd(5, 6)
    assert("async add", r === 11)

    // parallel
    const all = await parallel([
        asyncSquare(2),
        asyncSquare(3),
        asyncSquare(4),
    ])
    assert("parallel", all[0] === 4 && all[1] === 9)

    // TaskGroup con using (liberación determinista)
    using group = TaskGroup<int>()
    group.spawn(asyncAdd(7, 8))
    group.spawn(asyncAdd(9, 10))
    const joined = group.join()
    assert("group join", joined[0] === 15)

    // spawn individual
    const handle = spawn(asyncAdd(1, 2))
    assert("spawn", await handle === 3)
}

await runAsync()
```

### Isolates y multi-threading

```Varn
import { spawnIsolate } from "std:task"

export async function workerMain(port: dynamic) {
    const msg = await port.receive()
    port.send(msg.value * 2)
}

const port = await spawnIsolate(workerMain, [])
port.send({ value: 21 })
assert("isolate reply", await port.receive() === 42)
```

- `spawn` y `parallel` corren tareas Varn sobre el scheduler async.
- `spawnIsolate` lanza un worker en otro hilo (VM y heap propios); la comunicación es por canales tipados (`channel<T>` → `Sender`/`Receiver`).
- Los isolates no comparten heap mutable; cruzan el boundary solo valores sendables.

### Decoradores

```Varn
import { MetaKey, MethodContext } from "std:reflect"

function trackClass(cls: ClassRef): void { deco_log.push(cls.name) }

@trackClass
class DecoratedA {}

// Stacked decorators (aplicados de abajo a arriba)
@markDeco("outer")
@markDeco("inner")
class DecoratedB {}

// MetaKey — metadata tipada en clases
const RouteKey = MetaKey.create<str>()

function routeDeco(path: str) {
    return (cls: ClassRef) => { RouteKey.set(cls, path) }
}

@routeDeco("/api/items")
class ItemController {}

assert("meta route", RouteKey.get(ItemController) === "/api/items")

// Method decorators
function logMethod(fn: FunctionRef, ctx: MethodContext): FunctionRef {
    return (...args: dynamic[]) => {
        log.push(ctx.name + ":static=" + ctx.isStatic)
        return fn(...args)
    }
}

class DecoratedCalc {
    @logMethod
    mul(a: int, b: int): int { return a * b }
}
```

### Paquetes

```Varn
// varn.json
{
  "name": "my-app",
  "version": "1.0.0",
  "dependencies": {
    "mathlib": "github.com/user/mathlib@^1.2.3"
  }
}

// imports por alias — origen vive solo en varn.json
import { add } from "pkg:mathlib"
import { format } from "pkg:mathlib/utils"
```

---

## Instalación

Requiere **Rust stable**.

```bash
git clone https://github.com/tu-usuario/Varn
cd Varn
cargo build --bin vn --release
```

Añade a PATH:

```bash
cp target/release/vn ~/.local/bin/   # Linux/macOS
```

---

## Comandos

Referencia completa: [CLI_REFERENCE.md](docs/CLI_REFERENCE.md)

### Ejecutar

```bash
vn program.vn                    # implícito run
vn run program.vn
vn run program.vn -- arg1 arg2   # argumentos al script
vn run program.vnc               # ejecutar compilado
```

### Type checking

```bash
vn check program.vn
vn check -v program.vn
```

### Evaluar inline

```bash
vn eval "print(1 + 2)"
vn eval "function double(x: int) = x * 2; print(double(21))"
vn debug -e "print('hello')"
```

### Compilar a `.vnc`

```bash
vn build program.vn              # → program.vnc junto al fuente
vn build program.vn -o out.vnc   # path explícito
```

El `.vnc` contiene el grafo completo de bytecode. No incluye stdlib (embedded en el runtime). Ejecuta directo sin recompilar.

### Benchmark

```bash
vn bench program.vn              # read + lex + parse + check + compile + optimize + execute
vn bench program.vn --runs 100
vn bench program.vnc             # solo load + execute
vn bench --show-output program.vn
```

Mediciones reales del estado actual del repo (`cargo run --bin vn -- bench ... --runs 10`, 2026-06-05):

- `tests/45-simple-file-test.vn`: p50 end-to-end `912 µs`, throughput `1096.4 runs/s`
- `tests/21-async.vn`: p50 end-to-end `1.901 ms`, throughput `525.9 runs/s`
- `tests/47-isolates-multithread.vn`: p50 end-to-end `33.16 ms`, throughput `30.2 runs/s`

El benchmark actual también reporta `Module precompilation (cold startup)`, breakdowns de parser/checker, hotspots de opcodes, perfil de VM, GC y JIT. Los números viejos de `<1 ms` ya no representan la realidad general del lenguaje ni del benchmark actual.

`tests/main.vn` es una suite de integración, no un benchmark canónico único. Para comparar rendimiento usa programas focalizados y, si vas a publicar cifras, indica archivo, build (`dev`/`release`) y número de runs.

#### Varn vs Node/Bun (2026-07-25)

Head-to-head en la misma máquina (release) contra `bun` 1.3.4 (JavaScriptCore) y
`node` v24.4.1 (V8), sobre los ports JS pareados de `benchmarks/js/`.

| bench       | backend Varn en el hot path | Varn    | Bun    | Node   | resultado    |
|-------------|-----------------------------|---------|--------|--------|--------------|
| **fib(35)** | **Cranelift** (`fib`)       | 34.5 ms | 39.7 ms| 56.0 ms| **gana 1.15× / 1.62×** |
| matrix 150  | template (`<module>`)       | 30.9 ms | 4.1 ms | 3.6 ms | pierde 7.5× / 8.6× |
| array_ops   | template (`<module>`)       | 12.9 ms | 1.9 ms | 3.2 ms | pierde 6.8× / 4.0× |
| gc_alloc    | template (`<module>`)       | 72.0 ms | 16.3 ms| 7.3 ms | pierde 4.4× / 9.9× |
| dto         | intérprete (`<module>`)     | 82.0 ms | 5.7 ms | 3.1 ms | pierde 14× / 26× |
| math loop   | template (`benchMath`)      | 25.6 ms | 6.4 ms | 5.2 ms | pierde 4.0× / 4.9× |

**Una sola variable explica la tabla: si el código caliente rutea por Cranelift o
no.** `fib` es el único bench cuya función caliente compila a clif — y ahí Varn
gana a JSC y a V8. En todos los demás el trabajo vive en código *top-level* o en
una función que hace *bail*, y cae al template JIT o al intérprete.

Causas exactas de bail hoy (`VARN_CLIF_TRACE=1`):

| bench      | quién bailea      | motivo                                          |
|------------|-------------------|-------------------------------------------------|
| matrix     | `<module>`        | `unsupported opcode DefineGlobalIdx`            |
| array_ops  | `<module>`        | `unsupported opcode DefineGlobalIdx`            |
| gc_alloc   | `<module>`        | `unsupported opcode LoadModule`                 |
| dto        | `<module>`        | >250 palabras de bytecode: ni llega al JIT      |
| math       | `benchMath`       | `move across float/int representation`          |
| math       | wrappers `std:math` | `float register rN written by unsupported op Call` |

Los cuatro primeros son **el mismo bloqueador**: el código a nivel de módulo no
rutea. Cerrarlo mueve matrix, array_ops, gc_alloc y dto de golpe — es la palanca
de mayor ROI abierta. El caso de `math` es distinto y más barato: la lowering de
floats admite pocos escritores de registro `Float`
(`clif::floats::is_supported_float_writer`), y `Call` no está entre ellos.

Metodología (importa: esta máquina termaliza fuerte y una tanda A-luego-B da
resultados falsos): 5 rondas **rotando el orden de los tres runtimes** en cada
ronda, y se reporta el **mínimo** de cada uno, no la media. Varn usa la columna
`min` de la fase `execute` de `vn bench <f>.vn`; los ports JS usan
`performance.now()` best-of-10 con 3 warmups. Comparar mínimo contra mínimo.

Nota sobre la tabla anterior (2026-07-23): sus cifras de Bun/Node se tomaron con
la máquina en throttling y estaban ~3× infladas, lo que hacía ver las pérdidas
más pequeñas de lo que son.

### Debug e inspección

```bash
vn debug program.vn               # todas las fases
vn debug -p ast program.vn        # solo AST
vn debug -p check program.vn      # tipos inferidos
vn debug -p bytecode program.vn   # bytecode desensamblado
vn debug -e "function f(x: int) = x * 2"  # código inline
```

Fases disponibles: `tokens`, `ast`, `check`, `bytecode`, `symbols`, `binds`, `types[:N]`, `expr`, `modules`, `graph`, `caps`, `scope`, `errors`, `trace`, `info`, `lsp[:sub]` y `all`.

### REPL

```bash
vn repl
vn repl --debug-bytecode
```

### Paquetes

```bash
vn pkg add mathlib github.com/user/mathlib@^1.2.3
vn pkg remove mathlib
vn pkg install      # desde lockfile, offline si cacheado
vn pkg update       # re-resuelve contra tags remotos
```

### Proyecto

```bash
vn init                          # directorio actual
vn init my-project
vn init my-project --name "Mi App"
vn doctor                        # diagnóstico del entorno
vn cache clean                   # limpia cache local del proyecto
vn lsp                           # servidor LSP por stdio
vn completions bash              # completions de shell
```

### Referencia completa

Consulta [docs/CLI_REFERENCE.md](docs/CLI_REFERENCE.md) para ver la ayuda consolidada de todos los comandos, flags y ejemplos.

### Debug

```bash
vn debug -p ast program.vn
vn debug -p check program.vn
vn debug -p bytecode program.vn
vn debug program.vn
vn run --trace program.vn        # trace de ejecución instrucción a instrucción

# Fases: tokens, ast, check, bytecode, symbols, binds, types[:N], expr, modules, graph, caps, scope, errors, trace, info, lsp[:sub], all
```

---

## Estructura de proyecto

```
my-project/
├── main.vn
├── varn.json          ← manifest (nombre, versión, dependencias)
└── .vn/
    ├── varn.lock      ← lockfile reproducible (commitear)
    ├── packages/      ← paquetes instalados por alias
    │   └── mathlib/
    │       └── varn.json
    ├── cache/         ← bytecode cache automático (.bin)
    ├── .env           ← variables de entorno (gitignored)
    └── .gitignore
```

---

## Arquitectura

```
fuente .vn
    │
    ├── varn-lexer      → tokens
    ├── varn-parser     → AST
    ├── varn-checker    → type checking + resolución de módulos
    ├── varn-opt        → HIR → SSA → passes → bytecode (FunctionProto)
    ├── varn-backend    → liveness, register allocation, slot kinds
    └── varn-vm         → VM register-based, NaN-boxing, GC generacional, IC
            │
            ├── varn-jit        → JIT x86-64 (compilación eager)
            ├── varn-builtins   → stdlib nativa en Rust
            └── varn-runtime    → scheduler async (Tokio) + isolates
```

| Crate | Rol |
|-------|-----|
| `varn-core` | AST, `OpCode`, `ModuleId` — sin deps internas |
| `varn-lexer` | Tokenizer |
| `varn-parser` | Parser → AST |
| `varn-checker` | Type checker, resolución de módulos |
| `varn-opt` | **El compilador**: HIR → SSA → passes → bytecode |
| `varn-backend` | Post-passes de bytecode: liveness, regalloc, slot kinds |
| `varn-vm` | VM register-based, NaN-boxing, GC generacional, Inline Cache |
| `varn-jit` | JIT x86-64 |
| `varn-pipeline` | Orquesta las fases + caché de bytecode |
| `varn-types` | `VmValue`, `Chunk`, `FunctionProto`, `ObjData`, `Shape` |
| `varn-builtins` | Stdlib nativa |
| `varn-modules` | Resolución de paquetes, manifests |
| `varn-pm` | Package manager (add/install/update/remove) |
| `varn-cli` | Binario `vn`, pipeline completo |
| `varn-debug` | Inspección de fases, profiling, bytecode |
| `varn-op-macros` | Proc macros para bindings nativos |

### VM — características

- **NaN-Boxing**: null, bool, int (48 bits), float, strings de ≤5 bytes y punteros — todo en 64 bits, sin boxing
- **GC generacional**: nursery con promoción al old-gen, más mark-and-sweep tricolor y write barrier
- **Objetos en una sola allocation**: cabecera y campos comparten el bloque `Rc` (cola DST dimensionada a la shape)
- **Inline Cache**: polimórfico, hasta 8 entradas por site, indexado por shape id y compartido con el JIT
- **JIT x86-64**: compila eager al construir el closure; lo que no compila, se interpreta
- **Fast-path calls**: rutas rápidas para closures y natives cuando el call-site lo permite
- **Upvalues**: closures con captura correcta (open/closed)
- **Async**: VM suspendible, `await` pausa el frame, runtime lo reanuda
- **Isolates**: ejecución en hilos separados con paso de mensajes

---

## Testing

```bash
cargo run --bin vn -- tests/main.vn
```

```
════════════════════════════════════════
Modules executed in suite: 48

PASSED: 686
FAILED: 0
ALL TESTS PASSED
```

Panorama real del corpus `tests/` a fecha `2026-06-05`:

- La suite por defecto `tests/main.vn` importa `48` módulos y hoy pasa completa.
- `tests/41-advanced-enums.vn`, `tests/42-stdlib-comprehensive-test.vn` y `tests/47-isolates-multithread.vn` ya están reintegrados en la suite principal.

---

## Licencia

Apache License 2.0 — ver [LICENSE](LICENSE).
