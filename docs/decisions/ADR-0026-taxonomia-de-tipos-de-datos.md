# ADR-0026 — Taxonomía de los tipos de datos

Estado: APROBADO EN DISEÑO (2026-11-07). Implementación pendiente, por etapas (§8).

## 1. Problema

El sistema de tipos conflate naturalezas distintas bajo un mismo símbolo, y cada regla nueva choca con otra:

- `TypeKind::Object` significa a la vez el tipo anotado `{x:int}`, el tipo inferido de un literal y una interfaz genérica ya expandida. Aceptar clase → interfaz pero no clase → `{x:int}` es imposible de expresar.
- El record es una bolsa mutable con identidad; la tupla y el struct son valores inmutables. Tres productos con tres semánticas.
- `Map<float,V>`, `Map<{x:int},V>` y `Map<Clase,V>` compilan sin restricción de clave; la VM decide en runtime cómo hashear (`varn-vm/src/exec/hashable_keys.rs`): una decisión dinámica sobre un hecho estático (Leyes 6 y 17).
- Los accesos a miembros de records e interfaces van por nombre (`Resolution::ByName`, `DynReason::{Shape, Interface}`): medido 2,5× más lento que un slot fijo.
- La invariancia se aproxima con compatibilidad en ambos sentidos (`arrays::same_element`), que falla cuando dos tipos son asignables en una sola dirección.
- Hay dos o tres formas de decir lo mismo: `==` y `===`, `switch` y `match`, `instanceof` e `is`, `{[k:str]:V}` y `Map<str,V>`, spread con sobrescritura y `with`.

## 2. Principio

Cada tipo tiene **una sola naturaleza**. Ninguna sustitución genérica, alias ni inferencia la cambia. Un símbolo, un significado.

Tres familias:

| Familia | Semántica | Igualdad / hash | Mutabilidad |
|---|---|---|---|
| **Valor** | copia lógica; el compilador elige la representación | estructurales | inmutable, se "modifica" con `with` |
| **Entidad** | identidad | por identidad | mutable |
| **Contrato** | sin instancias | n/a | n/a |

Lecciones de otros lenguajes que sostienen la decisión:

| Lección | Origen |
|---|---|
| Los datos con igualdad por valor son lo más usado | C# `record`, Kotlin `data class`, F# records, Java `record` |
| Un tipo de valor mutable es una trampa | C# (`readonly struct`) |
| Las claves de un diccionario deben ser comparables en compilación | Go `comparable`, Rust `Hash + Eq` |
| Los contratos definidos por el consumidor desacoplan | Go (interfaces estructurales con itabs) |
| La covarianza de arrays y la bivarianza de parámetros son agujeros de solidez | C# arrays, TypeScript |
| Un `==` ambiguo entre identidad y valor es una fuente de errores | Java `==` frente a `equals` |
| Dos conceptos con un solo símbolo envejecen mal | TypeScript: `interface` frente a `type`, `x?:` frente a `T \| undefined` |

## 3. Taxonomía

### 3.1 Valor

| | posicional | con nombres de campo | nominal |
|---|---|---|---|
| **producto** | **tupla** `(int, str)` | **record** `{ x: int }` | **struct** |
| **suma** | | | **enum** (con payload) |

Más los primitivos (`int`, `float`, `decimal`, `bigint`, `bool`, `char`, `str`, `null`).

Todos los productos comparten semántica: inmutables, `==` y hash estructurales, `with`, desestructuración, patrón en `match`. Solo difieren en cómo se nombran:

- **Tupla**: posiciones, anónima, estructural por aridad y elementos.
- **Record**: nombres de campo, anónimo, **estructural y cerrado**. Dos records son el mismo tipo si tienen exactamente los mismos campos con tipos idénticos. Sin subtipado por ancho ni por profundidad. Un record no tiene métodos propios; el comportamiento va en `extension`.
- **Struct**: nombrado, nominal, puede tener métodos, genéricos e `implements`.

Layout canónico de record y tupla: campos en orden alfabético / posicional (Ley 4). Todo record con los mismos campos comparte forma y offsets. Al ser inmutable, su representación (inline o en heap) es una decisión del compilador y se comparte sin copiar.

`x?: T` en un record es un campo **omisible en el literal** (vale `null`). `x: T?` es un campo obligatorio que puede valer `null`. Son dos obligaciones distintas, no dos formas de lo mismo.

### 3.2 Entidad

| Tipo | Naturaleza |
|---|---|
| **clase** | nominal, `extends` único, `implements` verificado; mutable; identidad |
| **array** `T[]` | mutable; invariante en `T` |
| **Map** `Map<K, V>` | mutable; orden de inserción; invariante en `K` y `V` |
| **Set** `Set<T>` | mutable; orden de inserción; invariante en `T` |

Vistas de solo lectura (`readonly T[]`, `ReadonlyMap<K,V>`, `ReadonlySet<T>`): covariantes. La iteración de Map y Set es siempre en orden de inserción, nunca en orden de hash (Ley 4).

### 3.3 Contrato

**Interfaz**: contrato estructural, sin instancias. La satisfacen clases, structs y records con los miembros pedidos, sin `implements` (`implements` queda como comprobación opcional en la declaración). Una interfaz genérica instanciada **sigue siendo interfaz**.

Acceso a un miembro de interfaz: selector estable (hash de nombre y firma), tabla por clase y caché en línea. La lectura sale tipada, porque el tipo del miembro es el mismo para todo implementador.

`interface` solo define contratos. `type` solo define alias transparentes: `type X = T` hace que `X` y `T` sean el mismo tipo; un alias nunca crea un tipo ni cambia su naturaleza.

### 3.4 Hashable (clave de Map y Set)

Capacidad estática calculada una sola vez por el checker y transportada por el TIR (Ley 6):

| Hashable | No Hashable |
|---|---|
| `int`, `bool`, `char`, `str`, `bigint`, `decimal`, enums | `float` (NaN ≠ NaN, `0.0` ≡ `-0.0`) |
| tuplas, records y structs cuyos miembros son Hashable | arrays, Map, Set (contenido mutable) |
| **clases, por identidad** | funciones |

Una clase como clave es segura porque la identidad no cambia al mutar el objeto. `Map<float, V>`, `Map<T[], V>` y similares son un error de compilación con diagnóstico que indica la alternativa. No hay `IdentityMap`.

## 4. Relaciones entre tipos (una por pregunta)

1. **Identidad**: igualdad del id canónico del tipo. Se usa para invariancia. Reemplaza a `same_element`.
2. **Subtipado / asignabilidad**: reglas por naturaleza (§3). Una clase, struct o interfaz **no** es asignable a un record; un record sí lo es a una interfaz que cumpla.
3. **Conversión**: solo explícita (`as`). `dynamic` solo sale con `as T` y comprobación en runtime. Sin coerciones implícitas (Ley 17).

Un literal se **comprueba contra el tipo esperado** (propagación bidireccional) en lugar de inferirse y compararse después.

Varianza **declarada** en el parámetro de tipo (`out T`, `in T`) y comprobada en la definición.

Los productores de records (literales, `CONFORM`/`DECODE` del borde dinámico, natives con retorno record) entregan siempre la forma canónica.

## 5. Sintaxis final

`✚` nuevo. `✎` cambia. Sin marca: ya existe y no cambia.

### 5.1 Separadores
- Miembros de una declaración de tipo (`interface`, `struct`, `class`, tipo `{...}`): `;` o salto de línea. ✎ el tipo `{ x: int, y: int }` con coma deja de aceptarse.
- Valores (literales, tuplas, argumentos, patrones): `,`.

### 5.2 Enlaces y ausencia
```
const x = 1
let n: int = 0
T?    x?.y    x ?? d    x ??= d
if (let v = find(k)) { ... }
```

### 5.3 Valor
```
type Pair = (int, str)
const t: Pair = (1, "a")
const (a, b) = t
t[0]

type User = { id: int; name: str; email?: str; age: int? }          ✎ inmutable
const u: User = { id: 1, name: "Ana" }
const v = u with { name: "Eva" }
const w = { ...u, ...extra }                                         ✎ repetir un campo es error
const { id, name: nombre, ...rest } = u
const r = { id, name }
u.name = "x"                                                         error

struct Point { x: int; y: int }
struct Pair<A, B> { first: A; second: B }
struct Money { amount: decimal; currency: str
    plus(o: Money): Money { return new Money(this.amount + o.amount, this.currency) } }
const p = new Point(1, 2)
const q = p with { x: 5 }

enum Level { Low, High }
enum Shape { Circle(r: float), Rect(w: float, h: float), Unit }
const area = match (shape) {
    Circle(r) => 3.14 * r * r,
    Rect(w, h) => w * h,
    Unit => 0.0
}
```
Patrones de `match`: literal, `|`, `_`, `(a, b)`, `{ x, y }`, `Circle(r)`, binding. Siempre exhaustivo.

### 5.4 Entidad
```
abstract class Shape { abstract area(): float }
class Account extends Base implements Auditable {
    id: int
    private balance: decimal = 0
    static count: int = 0
    constructor(public owner: str) { super() }
    get total(): decimal { return this.balance }
    set total(v: decimal) { this.balance = v }
    override toString(): str { return this.owner }
}

const xs: int[] = [1, 2, 3]
xs[0]    xs.length    xs.push(4)
const ro: readonly int[] = xs                                       ✚

const m = new Map<str, int>()
const m2 = new Map([("a", 1), ("b", 2)])                            ✚
m.set("c", 3)    m.get("a")     m.has("a")    m.delete("a")    m.size
for (const (k, v) of m) { ... }

const s = new Set([1, 2, 3])                                        ✚
s.add(4)    s.has(2)    s.delete(2)    s.size
for (const e of s) { ... }
```
`m.get(k)` devuelve `V?`. `arr[i]` devuelve `T` y falla en runtime fuera de rango (error de programación, no un caso normal).

### 5.5 Contratos y extensiones
```
interface Named { readonly name: str }
interface Printable { print(): str }
interface Source<out T> { next(): T? }                              ✚
interface Sink<in T> { put(x: T): void }                            ✚

extension StringUtils on str { shout(): str { return this + "!" } }
extension Geometry on { x: int; y: int } {                          ✚ sobre un record
    norm(): float { return sqrt(this.x * this.x + this.y * this.y) }
}
```

### 5.6 Funciones, genéricos, control
```
function add(a: int, b: int): int { return a + b }
const inc = (n: int): int => n + 1
type Binary = (a: int, b: int) => int
function f(x: int, y: int = 2, ...rest: int[]): void { ... }
function pick<T extends Animal>(x: T): T { return x }
class Box<out T> { constructor(public readonly value: T) {} }       ✚

if / else   while   do-while   for (;;)   for (const x of it)
break   continue   return   throw
try { } catch (e) { } finally { }       const v = riskyCall()?
using win = Window.open(...)
async function g(): int { return await h() }       function* gen(): int { yield 1 }
import { a } from "std:test"    export    namespace N { }
```

## 6. Qué se elimina (Ley 8)

| Sale | Lo reemplaza |
|---|---|
| `{ [k: str]: V }` (tipo dict) | `Map<str, V>` |
| `===` y `!==` | `==` y `!=` |
| `switch` | `match` |
| `instanceof` | `is` |
| `typeof x` como valor | `is` y `match`; `typeof` en posición de tipo se mantiene |
| coma como separador en tipos | `;` / salto de línea |
| spread que sobrescribe un campo | `with` |
| mutación de campo de record | `with`, o una `class` si hace falta estado mutable |
| `TypeKind::Object` unificado | `Record` e `Interface`, cada uno con su payload |
| `DynReason::{Shape, Interface}`, `Resolution::ByName` para estos casos | forma canónica y selector |
| `arrays::same_element` como mecanismo de invariancia | identidad de tipo |
| hashing dinámico por inspección en la VM | `Hashable` estático |
| aceptación clase / struct / interfaz → record | error de compilación |

## 7. Fuera de alcance (ADRs propios)

- **Uniones sin etiqueta `A | B`.** Hoy caen a `DynReason::Union`; necesitan test de tipo en runtime. Propuesta: solo para literales y `null`; los datos alternativos usan `enum`.
- `T[N]` de longitud fija.
- Decoradores y macros.

## 8. Plan por etapas (cada una verde en `tests/main.vn`, un commit atómico por cambio)

1. **Cimiento de tipos** (`varn-core`, `varn-sem`): `Record` e `Interface` como variantes con payload propio; la expansión genérica de una interfaz conserva la naturaleza; identidad canónica por naturaleza.
2. **Reglas del checker**: asignabilidad de §3 y §4; literal contra el tipo esperado; `same_element` → identidad; varianza `out`/`in`. Un test negativo por cada prohibición. Arreglar `std` y tests rotos (Ley 18).
3. **Record inmutable**: error al mutar un campo de record; `with` sobre record; spread sin sobrescritura; separadores en tipos; migración de `std` y tests.
4. **Eliminaciones léxicas**: `===`/`!==`, `switch`, `instanceof`, `typeof` como valor, dict `{[k:str]:V}` → `Map<str,V>`. Primero verificar en el corpus que `match` e `is` cubren todo uso.
5. **Hashable estático** para Map y Set; borrar el hashing dinámico de la VM; `new Map(entradas)` / `new Set(elementos)`; `readonly T[]` y las vistas.
6. **TIR y emisor**: forma canónica del record, `Resolution::ShapeSlot`, selector de interfaz; borrar `ByName` y `DynReason::{Shape, Interface}`.
7. **Borde dinámico**: `CONFORM`/`DECODE` y natives entregan forma canónica.
8. **Extensiones sobre records.**

## 9. Verificación

- Corpus completo en los tres niveles (`verify.ps1`): `FAILED: 0`.
- Un test negativo por cada prohibición de §3, §4 y §6.
- `vn debug -p bytecode tests/main.vn`: `GetProperty` por nombre sobre records e interfaces cae a ~0. Línea base: 1348 `GetProperty` frente a 1821 `GetFixedField`.
- Microbench de lectura sobre record e interfaz ≈ lectura sobre clase (hoy ≈ 2,5× más lenta).
- Antes de la etapa 3, medir en el corpus cuántos sitios mutan un campo de record.

## 10. Puntos que aún requieren comprobación

- Que `extension … on { x: int }` sea legal sobre un record no se ha verificado en el parser actual.
- Que eliminar `switch`, `instanceof` y `typeof` valor no pierda ningún uso que `match` e `is` no cubran.
- `==` sobre clases: hoy es `VN3010` (se usa `sameValue`). Se mantiene; se reconsidera si choca con `Hashable` por identidad.
