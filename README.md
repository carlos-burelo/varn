# Varn

[![License](https://img.shields.io/badge/License-Apache--2.0-blue?style=for-the-badge)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust-orange?style=for-the-badge)](https://www.rust-lang.org/)

**Varn** es un lenguaje compilado con tipado estático y VM register-based de alto rendimiento escrita en Rust. Extensión de archivos: `.vn`.

---

## Características

- **VM Register-Based con NaN-Boxing** — valores en 64 bits, sin boxing en el hot path
- **Tipado estático expresivo** — uniones, genéricos, exhaustividad total en `match`
- **Async nativo** — `async`/`await`, generadores, `TaskGroup`, `parallel`, `spawn`
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
    const joined = await group.join()
    assert("group join", joined[0] === 15)

    // spawn individual
    const handle = spawn(asyncAdd(1, 2))
    assert("spawn", await handle === 3)
}

await runAsync()
```

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
vn eval --debug all "print('hello')"
```

### Compilar a `.vnc`

```bash
vn build program.vn              # → program.vnc junto al fuente
vn build program.vn -o out.vnc   # path explícito
```

El `.vnc` contiene el grafo completo de bytecode. No incluye stdlib (embedded en el runtime). Ejecuta directo sin recompilar.

### Benchmark

```bash
vn bench program.vn              # fases: read/lex/parse/check/compile/execute
vn bench program.vn --runs 100
vn bench program.vnc             # solo load + execute (compara contra .vn)
vn bench --show-output program.vn
```

Output típico:
```
Benchmark · tests/main.vn  (10 runs)
Source  43 lines  1.1 KB  88 tokens

Phase        min      p50     mean      max       σ      total
──────── ──────── ──────── ──────── ──────── ──────── ─────────
read      26.8 µs  43.4 µs  43.1 µs  87.6 µs  18 µs    431 µs
lex       15.3 µs  16.4 µs  16.9 µs  21.2 µs  1.67 µs  169 µs
parse     16.5 µs  23.4 µs  26.8 µs  59.9 µs  12.4 µs  268 µs
check     74 µs    78.2 µs  81.9 µs  113 µs   10.9 µs  819 µs
compile   6.6 µs   7 µs     7.38 µs  9.5 µs   821 ns   73.8 µs
execute   1.723 ms 1.909 ms 1.929 ms 2.101 ms 127 µs   19.29 ms
──────── ──────── ──────── ──────── ──────── ──────── ─────────
total     1.862 ms 2.077 ms 2.105 ms 2.393 ms          21.05 ms

Throughput: 475.1 runs/s
```

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
vn lsp                           # servidor LSP por stdio
vn completions bash              # completions de shell
```

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
    ├── varn-compiler   → bytecode (FunctionProto / Chunk)
    └── varn-vm         → register-based VM, NaN-boxing, IC
            │
            ├── varn-builtins   → stdlib nativa en Rust
            └── varn-runtime    → async runtime
```

| Crate | Rol |
|-------|-----|
| `varn-core` | AST, `OpCode`, `ModuleId` — sin deps internas |
| `varn-lexer` | Tokenizer |
| `varn-parser` | Parser → AST |
| `varn-checker` | Type checker, resolución de módulos |
| `varn-compiler` | Codegen → `FunctionProto` / bytecode |
| `varn-vm` | VM register-based, NaN-boxing, Inline Cache |
| `varn-types` | `VmValue`, `Chunk`, `FunctionProto`, `Value` |
| `varn-builtins` | Stdlib nativa |
| `varn-modules` | Resolución de paquetes, manifests |
| `varn-pm` | Package manager (add/install/update/remove) |
| `varn-cli` | Binario `vn`, pipeline completo |
| `varn-debug` | Inspección de fases, profiling, bytecode |
| `varn-op-macros` | Proc macros para bindings nativos |

### VM — características

- **NaN-Boxing**: null, bool, int, float, puntero — todo en 64 bits, sin boxing
- **Inline Cache**: property access cacheado por forma de objeto (~60% hit rate)
- **Fast-path calls**: 60%+ de llamadas sin overhead de frame completo
- **Upvalues**: closures con captura correcta (open/closed)
- **Async**: VM suspendible, `await` pausa el frame, runtime lo reanuda

---

## Testing

```bash
cargo run --bin vn -- tests/main.vn
```

```
════════════════════════════════════════
PASSED: 534
FAILED: 0
ALL TESTS PASSED
```

---

## Licencia

Apache License 2.0 — ver [LICENSE](LICENSE).
