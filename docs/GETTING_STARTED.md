# Primeros Pasos con Varn

## Qué es Varn

Lenguaje compilado, estáticamente tipado, con VM register-based optimizada. Sintaxis propia inspirada en TypeScript pero sin ser TypeScript.

## Instalación

Consulta [INSTALL.md](INSTALL.md).

## Ejecutar el primer programa

```Varn
// hola.vn
print("Hola, mundo!")
```

```sh
vn hola.vn
```

## Ejecutar la suite de tests

```sh
vn tests/main.vn
# PASSED: 534 / FAILED: 0
```

## Comandos principales

| Comando | Descripción |
|---------|-------------|
| `vn run <file>` | Ejecutar programa |
| `vn check <file>` | Type-check sin ejecutar |
| `vn build <file>` | Compilar a `.wrc` |
| `vn bench <file>` | Benchmark con métricas VM |
| `vn disasm <file>` | Ver bytecode |
| `vn inspect <file>` | Inspeccionar AST/tipos/bytecode |
| `vn repl` | REPL interactivo |
| `vn add <alias> <origin>` | Añadir dependencia |
| `vn install` | Instalar dependencias del proyecto |

## Ejemplo de lenguaje

```Varn
// Variables
const x = 42
const name: str = "Varn"
let count = 0

// Functions
function add(a: int, b: int): int {
    return a + b
}

// Closures / lambdas
const double = (n: int) => n * 2
const greet = (name: str) => `Hello, ${name}!`

// Named arguments (can pass out of order)
function describe(name: str, age: int): str {
    return `${name} is ${age} years old`
}
print(describe(age: 25, name: "Bob"))  // Bob is 25 years old

// Classes
class Animal {
    name: str
    constructor(n: str) { this.name = n }
    speak(): str { return "..." }
    toString(): str { return `Animal(${this.name})` }
}

class Dog extends Animal {
    constructor(n: str) { super(n) }
    override speak(): str { return "Woof" }
}

// Abstract classes
abstract class Shape {
    abstract area(): float
    describe(): str { return `shape with area ${this.area()}` }
}

// Pattern matching
const val = 2
match (val) {
    1 => print("one"),
    2 | 3 => print("two or three"),
    _ => print("other"),
}

// Pipeline
const result = 5 |> double |> double
print(result)  // 20

// Generators
function* range(n: int) {
    let i = 0
    while (i < n) {
        yield i
        i = i + 1
    }
}

// Async
async function fetchData(): str {
    return await http.get("https://api.example.com/data")
}

// Extensions
extension StringExt on str {
    capitalize(): str {
        if (this.length === 0) { return this }
        return this[0].toUpperCase() + this.slice(1)
    }
}
print("hola".capitalize())  // Hola
```

## Importar stdlib

```Varn
import { readFile } from "std:fs"
import { now } from "std:time"
import { sha256 } from "std:crypto"
```

## Importar paquetes externos

```Varn
import { utils } from "pkg:mylib"
```

Requiere configuración en `varn.json`:
```json
{
  "name": "mi-proyecto",
  "deps": {
    "mylib": "github.com/user/mylib@^1.0.0"
  }
}
```

Instalar: `vn install`

## Siguiente

1. [varn-SPEC.md](varn-SPEC.md) — referencia del lenguaje
2. [ARCHITECTURE.md](ARCHITECTURE.md) — arquitectura interna
3. [LBI_ARCHITECTURE.md](LBI_ARCHITECTURE.md) — sistema de bindings nativos
4. [ROADMAP.md](ROADMAP.md) — hoja de ruta
