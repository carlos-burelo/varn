# ADR-0026 — Taxonomía de los tipos de datos

Estado: APROBADO EN DISEÑO (2026-11-07), ENMENDADO (2026-11-09) bajo las Leyes 20 y 21 (§11). Etapas 1 a 4 implementadas, incluido el diccionario; la 5 en parte (`Hashable`, claves `float`, literal de `Map`; faltan `readonly T[]` y las vistas); 6 y 7 pendientes (§8); 8 hecha.

## 1. Problema

El sistema de tipos conflate naturalezas distintas bajo un mismo símbolo, y cada regla nueva choca con otra:

- `TypeKind::Object` significa a la vez el tipo anotado `{x:int}`, el tipo inferido de un literal y una interfaz genérica ya expandida. Aceptar clase → interfaz pero no clase → `{x:int}` es imposible de expresar.
- El record es una bolsa mutable con identidad; la tupla y el struct son valores inmutables. Tres productos con tres semánticas.
- `Map<float,V>`, `Map<{x:int},V>` y `Map<Clase,V>` compilan sin restricción de clave; la VM decide en runtime cómo hashear (`varn-vm/src/exec/hashable_keys.rs`): una decisión dinámica sobre un hecho estático (Leyes 6 y 17).
- Los accesos a miembros de records e interfaces van por nombre (`Resolution::ByName`, `DynReason::{Shape, Interface}`): medido 2,5× más lento que un slot fijo.
- La invariancia se decide en ocho sitios con `arrays::same_element` en lugar de usar una única relación de equivalencia (§4).
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

Todos los productos comparten semántica: inmutables, `with`, desestructuración, patrón en `match`. Son **tipo valor**, con `==` estructural, solo si todos sus miembros lo son: un primitivo, o una tupla, record, struct o enum cuyos miembros (payloads) son tipo valor. Un producto que contiene un array, un Map, un Set o una clase no es tipo valor, y `==` sobre él es `VN3010`. Solo difieren en cómo se nombran:

- **Tupla**: posiciones, anónima, estructural por aridad y elementos.
- **Record**: nombres de campo, anónimo, **estructural y cerrado**. Dos records son el mismo tipo si tienen exactamente los mismos campos con tipos idénticos. Sin subtipado por ancho ni por profundidad. Un record no tiene métodos propios; el comportamiento va en `extension`.
- **Struct**: nombrado, nominal, puede tener métodos, genéricos e `implements`.

Layout canónico de record y tupla: campos en orden alfabético / posicional (Ley 4), **decidido en el backend** al fijar la forma (etapa 6), no en el id del tipo: ordenar los campos en el tipo cambiaba el texto de los mensajes. La comprobación de tipos de un record es independiente del orden. Todo record con los mismos campos comparte forma y offsets. Al ser inmutable, su representación (inline o en heap) es una decisión del compilador y se comparte sin copiar.

Un **literal** de record puede omitir los campos opcionales del tipo esperado (`{ retries: 3 }` donde se pide `{ retries: int; verbose?: bool }`). Es una regla del literal, que se comprueba contra el tipo esperado, y no de la asignabilidad entre records: dos tipos record son asignables solo si tienen los mismos campos con la misma opcionalidad y tipos equivalentes, en cualquier orden. Una variable `{ retries: int }` no pasa a `{ retries: int; verbose?: bool }`. No hay conversión ni copia implícita (§4.3, Ley 17).

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

Una propiedad de interfaz es de **solo lectura salvo que se declare `mut`**. Es un contrato de lo que el consumidor puede hacer: sin `mut`, un miembro más estrecho en el implementador (un campo `Dog` donde la interfaz pide `Animal`) es seguro y se acepta (covarianza); con `mut` el consumidor puede escribir, y la propiedad es invariante. `readonly` dentro de una `interface` es un error; `mut` sobre un método también.

Acceso a un miembro de interfaz: selector estable (hash de nombre y firma), tabla por clase y caché en línea. La lectura sale tipada, porque el tipo del miembro es el mismo para todo implementador.

`interface` solo define contratos. `type` solo define alias transparentes: `type X = T` hace que `X` y `T` sean el mismo tipo; un alias nunca crea un tipo ni cambia su naturaleza.

### 3.4 Hashable (clave de Map y Set)

Capacidad estática calculada una sola vez por el checker y transportada por el TIR (Ley 6):

| Hashable | No Hashable |
|---|---|
| `int`, `float`, `bool`, `char`, `str`, `bigint`, `decimal` | arrays, Map, Set (contenido mutable) |
| tuplas, structs y enums cuyos miembros (payloads) son Hashable | funciones |
| **clases, por identidad** | records, hasta que la etapa 6 les dé forma canónica |

La igualdad de una clave `float` es `SameValueZero`: NaN es la misma clave que NaN y `-0.0` la misma que `0.0`, normalizado al entrar en la colección. Un enum es Hashable solo si lo son todos los payloads de todas sus variantes: `Result<int, str>` lo es y `Result<int[], str>` no. Una clase como clave es segura porque la identidad no cambia al mutar el objeto, y su igualdad es `sameValue`, la misma identidad con la que es clave; sus métodos `hash` y `equals` no intervienen. `Map<T[], V>` y similares son un error de compilación (`VN3037`) con diagnóstico que indica la alternativa. El código genérico escribe `K extends Hashable`. No hay `IdentityMap`.

## 4. Relaciones entre tipos (una por pregunta)

1. **Equivalencia**: dos tipos son equivalentes si cada uno es asignable al otro. Es la única relación que decide la invariancia, y se define una vez. Con records cerrados y sin conversiones implícitas coincide con la identidad de tipo. Al ser record e interfaz tipos distintos, no confunde un record con una interfaz del mismo contenido.
2. **Subtipado / asignabilidad**: reglas por naturaleza (§3). Una clase, struct o interfaz **no** es asignable a un record; un record sí lo es a una interfaz que cumpla.
3. **Conversión**: solo explícita (`as`). `dynamic` solo sale con `as T` y comprobación en runtime. Sin coerciones implícitas (Ley 17).

Un literal se **comprueba contra el tipo esperado** (propagación bidireccional) en lugar de inferirse y compararse después.

Varianza **declarada** en el parámetro de tipo (`out T`, `in T`); sin anotación el parámetro es **invariante**. El binder comprueba cada declaración contra el uso real (`VN4118`); un argumento genérico o una clase base cuenta con la varianza declarada de su posición. La varianza ya no se infiere del cuerpo.

Una **construcción de variante** de un enum (`Opt.None`) cuyos argumentos de tipo no se pueden resolver (`never`) toma esos argumentos del tipo esperado del mismo enum; un valor ya guardado no se ensancha.

Los productores de records (literales, `CONFORM`/`DECODE` del borde dinámico, natives con retorno record) entregan siempre la forma canónica.

## 5. Sintaxis final

`✚` nuevo. `✎` cambia. Sin marca: ya existe y no cambia.

### 5.1 Separadores
- Miembros de una declaración de tipo (`interface`, `struct`, `class`, tipo `{...}`): `;`, `,` o salto de línea; el separador es léxico y no cambia el significado.
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
const w = { ...defaults, ...extra }                                  ✎ entre spreads gana el último
const { id, name: nombre, ...rest } = u
const r = { id, name }
let m = u
m.name = "x"                                                         ✎ m = m with { name: "x" }
u.name = "x"                                                         error: u es const (deep)

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
const m3: Map<str, int> = { a: 1, [key]: 2 }                        ✚ literal tipado por el Map esperado
"a" in m3                                                           ✚ m3.has("a")
m.set("c", 3)    m.get("a")     m.has("a")    m.delete("a")    m.size
for (const (k, v) of m) { ... }

const s = new Set([1, 2, 3])                                        ✚
s.add(4)    s.has(2)    s.delete(2)    s.size
for (const e of s) { ... }
```
`m.get(k)` devuelve `V?`. `arr[i]` devuelve `T` y falla en runtime fuera de rango (error de programación, no un caso normal).

### 5.5 Contratos y extensiones
```
interface Named { name: str }                                         ✎ solo lectura por defecto
interface Slot { mut name: str }                                    ✚ escribible a través de la interfaz
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
| repetir un campo explícito, o explícito más spread, en un literal | `with` (comprueba que el campo exista y conserva el tipo) |
| mutar un record o struct a través de un alias | `place.campo = v` reescrito a `place = place with { campo: v }` sobre `let`, `var`, parámetro, campo de clase o elemento; `const` es profundo |
| `TypeKind::Object` unificado | `Record` e `Interface`, cada uno con su payload |
| `DynReason::{Shape, Interface}`, `Resolution::ByName` para estos casos | forma canónica y selector |
| `arrays::same_element` repetido en ocho sitios | una única función de equivalencia (§4.1) |
| `Hashable { hash(): int }` de `core:capabilities` y el despacho de `hash`/`equals` en la VM | `Hashable` calculado por el compilador (§3.4) |
| un record asignable a otro por añadir campos opcionales | omisión de campos opcionales solo en el literal (§3.1) |
| hashing dinámico por inspección en la VM | `Hashable` estático |
| aceptación clase / struct / interfaz → record | error de compilación |

## 7. Fuera de alcance (ADRs propios)

- **Uniones sin etiqueta `A | B`.** Hoy caen a `DynReason::Union`; necesitan test de tipo en runtime. Propuesta: solo para literales y `null`; los datos alternativos usan `enum`.
- `T[N]` de longitud fija.
- Decoradores y macros.

## 8. Plan por etapas (cada una verde en `tests/main.vn`, un commit atómico por cambio)

1. **HECHA** (`562fabc7`). **Cimiento de tipos**: `Record` e `Interface` como variantes; la expansión genérica de una interfaz conserva la naturaleza.
2. **HECHA** (`6ffc2d08`..`56af75d0`). **Reglas del checker**: asignabilidad de §3 y §4; tipado esperado de `??`/`||` y de variantes; propiedades de interfaz de solo lectura con `mut`; varianza `out`/`in` declarada (`VN4118`). Un test negativo por cada prohibición; `std` y tests adaptados.
3. **HECHA** (`91558975`, `e61c69ce`, `5c78dc2e`). **Record inmutable**: error al mutar un campo de record (`VN3005`); un literal no repite un campo (`VN3035`); separadores `;` en tipos. Medido antes: 7 sitios en 3 tests y 1 en `std`.
4. **HECHA salvo el diccionario**. `===`/`!==` (859 usos migrados), `instanceof` → `is` (93), `typeof` valor → `x::type` (58), `switch` → `match` (1 test). Por el camino: `is` ya no responde «verdadero» a tipos que no sabe probar (`VN3036`) y `continue` dentro de un `match` ya ejecuta el paso del `for`. Pendiente: dict `{[k:str]:V}` → `Map<str,V>` (130 usos).
5. **PARCIAL** (`ed009cb0`..`e3e01334`, `c2db6040`, `92c9215b`). **Clave estática para Map y Set**: `Hashable` es una restricción intrínseca que calcula el compilador. `Map<K extends Hashable, V>` y `Set<T extends Hashable>` rechazan con `VN3037` lo que no lo es; el código genérico escribe `K extends Hashable`. Son Hashable: `int`, `float` (`SameValueZero`), `bool`, `char`, `str`, `bigint`, `decimal`, enums, clases (por identidad) y tuplas o structs de ellos. El literal `{ k: v }` con un `Map` esperado construye ese Map. La capacidad `Hashable { hash(): int }` de `core:capabilities` se borró, y la VM ya no invoca `hash` ni `equals` de una clase: una clase es clave por su identidad, nunca por sus métodos. **Los records no son Hashable todavía**: la VM los compara y hashea por identidad, y lo serán cuando la etapa 6 les dé forma canónica. Pendiente: que la VM deje de decidir en runtime la representación de la clave (`map_keys.rs`), `new Map(entradas)` / `new Set(elementos)`, `readonly T[]` y las vistas.
6. **TIR y emisor**: forma canónica del record, `Resolution::ShapeSlot`, selector de interfaz; borrar `ByName` y `DynReason::{Shape, Interface}`.
7. **Borde dinámico**: `CONFORM`/`DECODE` y natives entregan forma canónica.
8. **HECHA.** Extensiones sobre records.
9. **HECHA** (enmienda, §11). Coma en tipos, spread entre fuentes, actualización de campo de valor por su lugar, `float` como clave, literal de `Map`.
10. **HECHA** (`543c2ee7`). Vistas de solo lectura: `readonly T[]` (`ReadonlyArray<T>`), `ReadonlyMap<K, out V>`, `ReadonlySet<T>`. Una colección es asignable a su vista y no al revés; la vista no tiene `push`, `set` ni `add`; baja al tipo del backend de su colección. Una vista devuelve vistas, por lo que su covarianza es sólida.
11. **PENDIENTE**. Extensiones genéricas: `extension Nombre<T> on readonly T[]`, con parámetros de tipo en la declaración y propagación de las extensiones declaradas en un módulo core. Hoy solo existen extensiones sobre un tipo concreto. Con ellas entran en la vista `includes`, `indexOf`, `lastIndexOf` y `flat`, que no caben como miembros de una interfaz `out T` porque reciben `T` o devuelven un tipo condicional sobre `T`, invariante por correcto.

## 9. Verificación

- Corpus completo en los tres niveles (`verify.ps1`): `FAILED: 0`.
- Un test negativo por cada prohibición de §3, §4 y §6.
- `vn debug -p bytecode tests/main.vn`: `GetProperty` por nombre sobre records e interfaces cae a ~0. Línea base: 1348 `GetProperty` frente a 1821 `GetFixedField`.
- Microbench de lectura sobre record e interfaz ≈ lectura sobre clase (hoy ≈ 2,5× más lenta).
- Antes de la etapa 3, medir en el corpus cuántos sitios mutan un campo de record.

## 11. Enmienda (Leyes 20 y 21): cómo lo habría hecho Anders sin retrocompatibilidad

Criterio: TypeScript es el puente de sintaxis, no el modelo de semántica. Varn conserva lo que se resuelve en compilación (Ley 20) y se aleja de TypeScript solo donde la semántica estática lo exige, con la comparación escrita.

| Punto | Decisión | Comparación y razón |
|---|---|---|
| Record y struct inmutables | Se mantienen como valores. `place.campo = v` se reescribe en compilación a `place = place with { campo: v }`, recursivo hasta una variable, un campo de clase o un elemento. El binding debe ser `let`, `var` o parámetro; `const` es profundo. El lugar no puede contener llamadas (se lee y se escribe) | C#: el tipo valor mutable es una trampa y `record` es inmutable con `with`; refusa mutar la copia de un getter (CS1612). Swift: `var p = P(); p.x = 1` copia y `let` lo prohíbe, el modelo adoptado. Kotlin: solo `copy`. Rust: `let mut`. TypeScript: el objeto es una referencia compartida y `const` no protege sus campos; esa es la distancia, a cambio de que un record pueda ser clave de `Map` y no tenga alias |
| Spread de literales | Entre fuentes spread gana la última. Un campo explícito repetido, o explícito más spread, sigue siendo `VN3035` | `with` comprueba que el campo exista y conserva el tipo (C# `with`, Kotlin `copy`); `{ ...u, nmae: 1 }` en JavaScript crea un record distinto sin avisar. Fusionar configuraciones con spread es el idioma de TypeScript y `with` no lo expresa |
| Separador en tipos | `;`, `,` o salto de línea | TypeScript los acepta; Rust usa coma; Kotlin y Swift salto de línea. Es léxico y no crea una segunda ruta semántica |
| `float` como clave | Hashable con `SameValueZero` | JavaScript `Map` usa `SameValueZero`; C# `Double.Equals` y Java `Double.equals` igualan NaN consigo mismo. Rust lo rechaza solo porque `Hash` y `Eq` son rasgos. La VM ya compara claves por `sameValue` |
| Dict → `Map` | Literal `{ k: v }` tipado por el `Map` esperado, `m[k]`, `m[k] = v` y `k in m` | Kotlin `mapOf`, Swift `[k: v]`: el literal toma su tipo del contexto. Cumple la condición de la Ley 20: misma comodidad que el diccionario |
| Interfaz de solo lectura salvo `mut` | Se mantiene | C# (`{ get; }` frente a `{ get; set; }`), Swift (`{ get }` frente a `{ get set }`) y Kotlin (`val` frente a `var`) declaran la escritura en el contrato; da covarianza segura. TypeScript añadió `readonly` tarde |
| `===`, `switch`, `instanceof`, `typeof` como valor | Se mantienen las sustituciones (`==` estricto, `match`, `is`, `x::type`) | Decisión del propietario: Varn no reconstruye TypeScript. `==` no coacciona, así que no hay dos significados; `match` exhaustivo sustituye a `switch` con más poder; `is` cubre `instanceof` y los tipos que no son clases |
| Tuplas con paréntesis | Se mantienen | Es la sintaxis de C#, Kotlin y Swift |
| Uniones `A \| B` | Siguen fuera de alcance, y son el siguiente ADR | La Ley 20 pide conservarlas y resolverlas en compilación: unión anónima con etiqueta insertada por el compilador (Rust, Swift) y estrechamiento estático, sin `DynReason::Union` |

## 10. Puntos que aún requieren comprobación

- Que `extension … on { x: int }` sea legal sobre un record no se ha verificado en el parser actual.
- Que eliminar `switch`, `instanceof` y `typeof` valor no pierda ningún uso que `match` e `is` no cubran.
- `==` sobre clases es `VN3010`, y queda así: la igualdad de una clase es `sameValue` (identidad), la misma que la hace clave. Una igualdad de valor propia de una clase es un método (`Equatable<T>` de `core:capabilities`), nunca `==`.
