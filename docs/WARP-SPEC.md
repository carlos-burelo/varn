# Especificación Formal del Lenguaje Varn (WARP-SPEC)

Este documento constituye la especificación formal y referencia completa de la sintaxis, el sistema de tipos y la semántica visible del lenguaje de programación **Varn**.

---

## Tabla de Contenidos

- [1. Principios de Diseño](#1-principios-de-diseño)
- [2. Sistema de Tipos](#2-sistema-de-tipos)
  - [Tipos Primitivos](#tipos-primitivos)
  - [Tipos Compuestos y Complejos](#tipos-compuestos-y-complejos)
- [3. Declaraciones y Variables](#3-declaraciones-y-variables)
- [4. Funciones y Closures](#4-funciones-y-closures)
  - [Argumentos Nombrados](#argumentos-nombrados)
- [5. Programación Orientada a Objetos](#5-programación-orientada-a-objetos)
  - [Clases e Herencia](#clases-e-herencia)
  - [Interfaces y Tipado Estructural](#interfaces-y-tipado-estructural)
- [6. Genéricos y Extensibilidad](#6-genéricos-y-extensibilidad)
  - [Métodos de Extensión (`extension`)](#métodos-de-extensión-extension)
- [7. Control de Flujo y Pattern Matching](#7-control-de-flujo-y-pattern-matching)
- [8. Operador Pipeline (`|>`)](#8-operador-pipeline-)
- [9. Async/Await, Generadores e Isolates](#9-asyncawait-generadores-e-isolates)
- [10. Decoradores y Metadatos](#10-decoradores-y-metadatos)

---

## 1. Principios de Diseño

1. **Una Sola Semántica Clara**: Cada constructo sintáctico debe tener un comportamiento determinista único.
2. **Lo Explícito sobre lo Implícito**: Minimizar la magia sintáctica o las conversiones de tipos automáticas no deseadas.
3. **Verificación Estática Total**: El compilador y el type-checker deben verificar la corrección antes de la ejecución.
4. **Cero Deuda de Compatibilidad Legada**: No mantener sintaxis obsoleta por razones históricas.

---

## 2. Sistema de Tipos

### Tipos Primitivos

| Tipo | Descripción | Ejemplo |
|---|---|---|
| `int` | Entero con signo de 64/48 bits | `42`, `-100` |
| `float` | Flotante de 64 bits (IEEE 754) | `3.14159` |
| `decimal` | Decimal de alta precisión | `100.50d` |
| `str` | Cadena UTF-8 inmutable | `"Hola Varn"` |
| `char` | Carácter Unicode | `'a'`, `'🚀'` |
| `bool` | Valor booleano | `true`, `false` |
| `null` | Ausencia explícita de valor | `null` |
| `void` | Retorno vacío de función | `void` |
| `never` | Tipo Bottom (nunca retorna) | Invocaciones a `throw` |
| `unknown` | Tipo opaco que requiere narrowing | Respuestas de APIs dinámicas |

### Tipos Compuestos y Complejos

```Varn
T[]           // Array homogéneo
T | U         // Tipo Unión
T & U         // Tipo Intersección
[T, U, V]     // Tupla
T?            // Sugar sintáctico para T | null
Generic<T>    // Tipo parametrizado genérico
(x: T) => U   // Firma de función
```

---

## 3. Declaraciones y Variables

```Varn
const x: int = 42          // Variable inmutable
let contador: int = 0      // Variable mutable
const nombre = "Varn"      // Inferencia de tipo automática a `str`
```

---

## 4. Funciones y Closures

```Varn
function sumar(a: int, b: int): int {
    return a + b
}

// Closure anónimo
const duplicar = (n: int): int => n * 2

// Función genérica
function identidad<T>(valor: T): T {
    return valor
}
```

### Argumentos Nombrados

Las funciones admiten llamadas con argumentos nombrados en cualquier orden:

```Varn
function crearUsuario(nombre: str, edad: int, ciudad: str = "Desconocida"): str {
    return `${nombre} (${edad}) de ${ciudad}`
}

// Llamada posicional, nombrada fuera de orden o mixta:
crearUsuario(edad: 25, nombre: "Bob")
```

---

## 5. Programación Orientada a Objetos

### Clases e Herencia

```Varn
abstract class Animal {
    nombre: str
    constructor(n: str) { this.nombre = n }
    abstract hablar(): str
}

class Perro extends Animal {
    raza: str
    constructor(n: str, r: str) {
        super(n)
        this.raza = r
    }
    override hablar(): str {
        return "¡Guau!"
    }
}
```

### Interfaces y Tipado Estructural

```Varn
interface Imprimible {
    imprimir(): void
}

class Documento implements Imprimible {
    imprimir(): void {
        print("Imprimiendo documento...")
    }
}
```

---

## 6. Genéricos y Extensibilidad

### Métodos de Extensión (`extension`)

Permiten añadir nuevos métodos a tipos existentes sin modificar su definición original:

```Varn
extension StringUtils on str {
    esVacio(): bool {
        return this.length === 0
    }
}

print("".esVacio()) // true
```

---

## 7. Control de Flujo y Pattern Matching

```Varn
const valor = 2

const resultado = match (valor) {
    1 => "Uno",
    2 | 3 => "Dos o Tres",
    _ => "Otro"
}
```

---

## 8. Operador Pipeline (`|>`)

Permite encadenar funciones de izquierda a derecha usando el placeholder `_`:

```Varn
function duplicar(n: int): int = n * 2
function restar(a: int, b: int): int = a - b

const res = 10 |> duplicar |> restar(_, 5) // (10 * 2) - 5 = 15
```

---

## 9. Async/Await, Generadores e Isolates

```Varn
import { TaskGroup, spawnIsolate } from "std:task"

async function procesar(): void {
    using group = TaskGroup<int>()
    group.spawn(async () => 10)
    const res = await group.join()
}
```

---

## 10. Decoradores y Metadatos

```Varn
import { MetaKey } from "std:reflect"

const RouteKey = MetaKey.create<str>()

function Ruta(path: str) {
    return (cls: ClassRef) => { RouteKey.set(cls, path) }
}

@Ruta("/api/v1/usuarios")
class UsuarioController {}
```
