# Varn — Especificación del Sistema de Tipos y Plataforma Estándar

**Estado:** propuesta arquitectónica
**Objetivo:** establecer la base definitiva del sistema de tipos, representación numérica, layout, runtime y plataforma estándar de Varn.

---

# 1. Principios fundamentales

Varn debe distinguir tres niveles que no deben mezclarse:

```text
Language Type
    ↓
Semantic Type
    ↓
Physical Representation
```

Ejemplo:

```text
int
 ↓
signed 64-bit integer
 ↓
i64 register
```

Otro:

```text
Array<int>
 ↓
mutable homogeneous sequence of int
 ↓
contiguous i64 storage
```

La representación física puede cambiar mediante optimización sin modificar la semántica del programa.

Por tanto:

> Los tipos del lenguaje describen qué significa un valor. El compilador decide cómo representarlo.

Esto permite que Varn sea estáticamente predecible sin renunciar a optimizaciones agresivas.

---

# 2. Tipos numéricos

Varn tendrá exactamente cuatro tipos numéricos fundamentales:

```text
int
float
bigint
decimal
```

No existirán como tipos públicos:

```text
i8
i16
i32
i64
u8
u16
u32
u64
f32
f64
double
number
real
integer
```

## 2.1 `int`

`int` es el entero firmado canónico de Varn.

```text
representation: signed 64-bit two's complement
range: -2^63 .. 2^63-1
```

Ejemplo:

```varn
let x: int = 42
```

Características:

* exactitud entera;
* tamaño semánticamente fijo;
* representación nativa;
* operaciones optimizadas directamente por CPU;
* no requiere boxing;
* no depende de la arquitectura;
* overflow definido por la especificación.

### Overflow

Las operaciones normales de `int` son checked.

```varn
let x: int = MAX_INT
let y = x + 1
```

produce:

```text
IntegerOverflow
```

No existe wrapping silencioso.

No existe promoción automática a `bigint`.

No existe conversión automática a `float`.

El compilador puede eliminar el chequeo cuando demuestra que el overflow es imposible.

---

# 3. Operaciones enteras especializadas

El runtime puede proporcionar operaciones explícitas:

```text
wrappingAdd
wrappingSub
wrappingMul

saturatingAdd
saturatingSub
saturatingMul
```

Ejemplo:

```varn
let x = MAX_INT.wrappingAdd(1)
```

Esto permite programación de bajo nivel sin contaminar la semántica normal de `int`.

La existencia de estas operaciones no requiere exponer `u64`, `i32`, etc. como tipos del lenguaje.

---

# 4. `float`

`float` representa exactamente:

```text
IEEE 754 binary64
```

No existe `double`.

No existe `f64` como tipo público.

Por tanto:

```varn
let x: float = 3.141592653589793
```

significa binary64.

La implementación debe respetar las propiedades relevantes de IEEE 754:

```text
+0
-0
NaN
+Infinity
-Infinity
subnormal values
rounding
```

La especificación no debe intentar ocultar estas propiedades.

---

# 5. Conversión `int` / `float`

La conversión de `int` a `float` puede perder precisión.

Por tanto no debe considerarse un widening lossless.

Ejemplo:

```varn
let x: int = 9007199254740993
let y = x as float
```

La conversión es explícita porque puede producir pérdida de información.

El lenguaje no debe permitir conversiones implícitas arbitrarias:

```text
int → float
```

en expresiones generales.

Los literales numéricos que sean exactamente representables pueden participar en inferencia contextual.

---

# 6. `bigint`

`bigint` representa enteros con precisión arbitraria.

```varn
let x: bigint = 123456789012345678901234567890n
```

Características:

* precisión arbitraria;
* exactitud matemática;
* almacenamiento dinámico;
* coste variable;
* no forma parte del camino caliente de `int`.

El literal utiliza:

```text
n
```

como sufijo:

```varn
100n
```

---

# 7. Relación entre `int` y `bigint`

`int` y `bigint` pertenecen al mismo dominio matemático, pero son representaciones semánticas diferentes.

Una conversión:

```text
int → bigint
```

siempre es exacta.

Puede permitirse implícitamente cuando no introduzca ambigüedad.

La conversión inversa:

```text
bigint → int
```

requiere conversión explícita y comprobación de rango.

```varn
let x: int = value as int
```

Si no cabe:

```text
IntegerOverflow
```

Nunca debe truncarse silenciosamente.

---

# 8. `decimal`

`decimal` representa aritmética decimal de precisión arbitraria.

Ejemplo:

```varn
let price: decimal = 19.99d
```

El sufijo:

```text
d
```

identifica un literal decimal.

`decimal` existe para dominios donde la representación decimal forma parte de la semántica:

* dinero;
* contabilidad;
* impuestos;
* precios;
* cantidades decimales;
* cálculos financieros.

`decimal` no debe considerarse simplemente un `float` con más precisión.

Es otro dominio numérico.

---

# 9. Promoción numérica

No existe una jerarquía universal de conversión:

```text
int
 ↓
float
 ↓
decimal
```

Eso sería conceptualmente incorrecto.

Cada dominio tiene reglas propias.

La tabla fundamental es:

| Conversión         | Implícita         |
| ------------------ | ----------------- |
| `int → bigint`     | Sí, opcionalmente |
| `bigint → int`     | No                |
| `int → float`      | No                |
| `float → int`      | No                |
| `int → decimal`    | Puede ser sí      |
| `decimal → int`    | No                |
| `float → decimal`  | No                |
| `decimal → float`  | No                |
| `bigint → float`   | No                |
| `float → bigint`   | No                |
| `decimal → bigint` | No                |

El objetivo es que una conversión potencialmente lossy sea visible.

---

# 10. División

La división debe evitar ambigüedad histórica.

La operación:

```varn
5 / 2
```

mantiene el dominio de sus operandos.

Por tanto:

```text
int / int → int
float / float → float
decimal / decimal → decimal
bigint / bigint → bigint
```

La división entera debe tener semántica explícita y documentada, incluyendo:

* truncamiento;
* división por cero;
* overflow del caso mínimo dividido entre `-1`.

Las APIs especializadas pueden proporcionar variantes como:

```text
div
floorDiv
ceilDiv
rem
mod
```

---

# 11. Eliminación de numeric widths

Se eliminan del sistema de tipos público:

```text
i8
i16
i32
i64

u8
u16
u32
u64

f32
f64
```

Esto reduce la complejidad de:

* type checking;
* overload resolution;
* operadores;
* generics;
* inferencia;
* TIR;
* VM;
* JIT;
* ABI;
* FFI;
* serialización;
* documentación;
* stdlib.

Estos tamaños continúan existiendo en el backend.

---

# 12. Representaciones internas

La ausencia de tipos públicos no significa que el backend esté limitado.

El compilador puede utilizar:

```text
i8
i16
i32
i64
u8
u16
u32
u64
f32
f64
SIMD
```

como representaciones físicas.

Ejemplo:

```text
Array<int>
```

normalmente:

```text
i64[]
```

pero el optimizador podría utilizar:

```text
i32[]
```

si demuestra que todos los valores son representables.

Esto es una optimización de representación, no un cambio del tipo.

---

# 13. Tipos primitivos fundamentales

Varn tendrá:

```text
null
bool
int
float
bigint
decimal
char
str
```

Además:

```text
void
never
dynamic
```

como tipos especiales.

---

# 14. `char`

`char` representa un Unicode scalar value.

No representa:

* byte;
* UTF-8 code unit;
* grapheme cluster.

Una implementación puede utilizar una representación de 32 bits.

Las operaciones relacionadas con graphemes pertenecen a Unicode/stdlib.

---

# 15. `str`

`str` es una cadena inmutable.

La representación estándar será UTF-8.

Propiedades:

* inmutable;
* Unicode;
* compatible con zero-copy views;
* optimizable mediante representación compacta;
* GC-managed cuando corresponda.

Varn debe distinguir:

```text
str
```

de:

```text
StringView
```

---

# 16. `StringView`

Se introduce:

```text
StringView
```

como una vista no propietaria sobre una región UTF-8.

Está destinada a:

* parsers;
* protocolos;
* HTTP;
* serialización;
* archivos;
* búsqueda;
* tokenización;
* FFI.

Conceptualmente:

```text
str
 ↓
owned immutable string

StringView
 ↓
borrowed/non-owning UTF-8 view
```

La existencia de `StringView` evita copias innecesarias sin convertir la semántica de `str` en una estructura mutable.

---

# 17. `Bytes`

`Bytes` es la abstracción estándar para datos binarios.

No se expone `u8` como tipo público para utilizarlo en código normal.

Internamente:

```text
Bytes → contiguous byte storage
      → physical element representation = u8
```

Esto cubre:

* archivos;
* sockets;
* criptografía;
* compresión;
* protocolos;
* serialización;
* imágenes;
* WASM;
* FFI.

---

# 18. `Span<T>`

`Span<T>` se convierte en una abstracción fundamental.

Representa una vista contigua sobre elementos de tipo `T`.

Conceptualmente:

```text
Span<T>
    pointer/reference
    length
```

No posee necesariamente los elementos.

Usos:

* zero-copy;
* parsing;
* networking;
* serialization;
* SIMD;
* FFI;
* buffers;
* memoria nativa.

Ejemplos conceptuales:

```varn
Span<int>
Span<float>
Span<Byte>
```

---

# 19. Arrays

```text
Array<T>
```

es una colección mutable homogénea.

```varn
let values: Array<int> = [1, 2, 3]
```

La sintaxis:

```text
T[]
```

es equivalente semánticamente a:

```text
Array<T>
```

La representación puede ser especializada:

```text
Array<int>   → packed numeric storage
Array<float> → packed numeric storage
Array<str>   → references
Array<Class> → references
```

---

# 20. `TypedArray`

`TypedArray` deja de ser un tipo semántico público independiente.

Se convierte en un concepto de representación/stdlib cuando sea necesario.

En lugar de contaminar el sistema principal con:

```text
TypedArray<T>
```

la plataforma puede proporcionar estructuras especializadas para:

```text
binary data
SIMD
native buffers
FFI
```

El compilador puede utilizar almacenamiento especializado automáticamente.

---

# 21. Tuplas

```text
#[T1, T2, ...]
```

representa un producto estructural inmutable.

Ejemplo:

```varn
#[int, str, bool]
```

Las tuplas:

* son inmutables;
* tienen layout conocido;
* pueden almacenarse inline;
* permiten acceso estático;
* soportan igualdad estructural.

---

# 22. Records

```text
#{ name: str, age: int }
```

es un producto estructural inmutable.

Características:

* layout conocido;
* campos estáticos;
* igualdad estructural profunda;
* posibilidad de almacenamiento inline;
* excelentes candidatos para optimización de layout.

`Record<K,V>` queda prohibido.

`Record` queda reservado para la construcción:

```text
#{...}
```

---

# 23. Object

```text
{...}
```

representa un objeto mutable estructural.

Diferencia:

```text
Object
```

vs.

```text
Record
```

|           | Object                            | Record       |
| --------- | --------------------------------- | ------------ |
| mutable   | Sí                                | No           |
| igualdad  | referencia                        | estructural  |
| layout    | optimizable                       | conocido     |
| identidad | Sí                                | no semántica |
| uso       | entidades dinámicas/estructurales | datos        |

---

# 24. Map

`Map<K,V>` es una colección concreta.

No es equivalente a:

```text
{ [key: K]: V }
```

Estos conceptos deben separarse.

```text
Map<K,V>
```

es una estructura de datos.

```text
{ [key: K]: V }
```

es un tipo estructural indexable.

Esto evita mezclar semántica de colección con semántica de objetos.

---

# 25. Set

```text
Set<T>
```

es una colección basada en unicidad y hashing.

Su funcionamiento depende de capacidades como:

```text
Hashable
Equatable
```

y no de una lista fija de `TypeTag`.

---

# 26. Nullability

Se conserva:

```text
T?
```

como azúcar para:

```text
T | null
```

Ejemplos:

```text
str?
int?
Array<int>?
int?[]
int[]?
```

La distinción entre:

```text
Array<int?>
```

y:

```text
Array<int>?
```

es semánticamente obligatoria.

---

# 27. Unions

Se conserva:

```text
A | B
```

como unión estática.

Ejemplo:

```varn
str | int
```

Las uniones deben proporcionar narrowing mediante:

```text
control flow
type checks
instanceof
pattern matching
```

---

# 28. Intersection types

Se incorpora:

```text
A & B
```

como intersección de capacidades/tipos.

Esto es especialmente importante para el sistema estructural de interfaces.

Ejemplo conceptual:

```text
Readable & Closeable
```

permite expresar que un valor satisface ambas capacidades.

---

# 29. Literal types

Se incorporan internamente los tipos literales.

Ejemplo conceptual:

```text
"GET"
200
true
```

pueden funcionar como tipos singleton cuando el contexto lo requiere.

Esto habilita:

* discriminated unions;
* protocolos;
* pattern matching;
* APIs tipadas;
* estados finitos;
* exhaustiveness checking.

No significa que cada literal tenga que aparecer explícitamente en el código del usuario como una anotación de tipo.

---

# 30. Enums

Los enums pasan a formar parte del modelo general de sum types.

```text
SumType
├── Union
└── Enum
```

Los enums pueden tener:

* variantes simples;
* valores;
* payloads;
* múltiples campos;
* métodos;
* generics;
* interfaces.

Ejemplo:

```varn
enum Result<T, E> {
    Ok(T)
    Err(E)
}
```

---

# 31. Pattern matching

Pattern matching se convierte en una operación fundamental del sistema de tipos.

Debe trabajar conjuntamente con:

```text
Union
Enum
Literal types
Nullable
```

El compilador debe realizar exhaustiveness checking cuando sea posible.

---

# 32. Interfaces

Las interfaces continúan siendo estructurales.

Ejemplo:

```varn
interface Comparable<T> {
    compare(other: T): int
}
```

Un tipo satisface la interfaz si posee la estructura requerida.

No se requiere herencia explícita.

---

# 33. Capabilities estándar

La stdlib utilizará interfaces/capabilities para describir operaciones genéricas.

Base recomendada:

```text
Equatable<T>
Comparable<T>
Hashable
Cloneable
Default
Display
Debug
Iterable<T>
Iterator<T>
AsyncIterable<T>
Indexable<K,V>
```

Para operaciones numéricas:

```text
Add<T, R>
Sub<T, R>
Mul<T, R>
Div<T, R>
Neg<T, R>
```

Esto evita que el compilador tenga lógica como:

```text
if TypeTag == Int
else if TypeTag == Float
else if ...
```

para resolver operadores.

---

# 34. Operator overloading

Los operadores se resolverán mediante capacidades estáticas.

Conceptualmente:

```varn
a + b
```

se resuelve mediante una operación equivalente a:

```text
Add<A, B, R>
```

El compilador puede especializarla completamente.

Para:

```text
int + int
```

el resultado debe terminar como operación nativa.

Para un tipo definido por el usuario:

```text
Vector + Vector
```

se resuelve mediante su implementación de `Add`.

No se introduce dispatch dinámico cuando los tipos son estáticos.

---

# 35. `Type<T>`

Se incorpora el concepto de meta-tipo:

```text
Type<T>
```

Ejemplo conceptual:

```varn
let t = Type<int>
```

Esto permite construir reflection de manera tipada.

La reflexión no debe depender de cadenas arbitrarias cuando el compilador puede resolver la información estáticamente.

---

# 36. Reflection

Se conserva la idea:

```text
Obj::key
```

pero debe basarse en metadatos estáticos cuando sea posible.

Reflection debe proporcionar:

* tipo;
* campos;
* métodos;
* atributos;
* enum variants;
* layout;
* metadata;
* capacidades.

El acceso estático no debe convertirse automáticamente en reflexión dinámica.

---

# 37. Callable types

Las funciones continúan siendo valores tipados:

```text
(T) => U
```

Ejemplo:

```varn
let f: (int) => float
```

Debe existir una distinción interna entre:

```text
Function
NativeFunction
```

pero ambos pertenecen al mismo dominio callable.

---

# 38. Runtime types vs language types

La arquitectura debe eliminar la idea de que cada `TypeTag` corresponde directamente a un tipo del lenguaje.

`TypeTag` debe convertirse en una clasificación runtime.

Por ejemplo:

```text
RuntimeValueKind
```

puede contener:

```text
Null
Bool
Int
Float
BigInt
Decimal
String
Array
Map
Set
Object
Class
Function
...
```

Pero el tipo estático real:

```text
Map<str, Array<int>>
```

vive en el sistema de tipos, no en `RuntimeValueKind::Map`.

---

# 39. Nuevo modelo de Type

El compilador debe trabajar con un modelo estructurado equivalente a:

```text
Type
├── Primitive
├── Literal
├── Nullable
├── Union
├── Intersection
├── Tuple
├── Record
├── Object
├── Array
├── Map
├── Set
├── Span
├── Function
├── Class
├── Enum
├── Generic
├── TypeParameter
├── MetaType
└── Dynamic
```

El runtime no necesita conocer toda esta estructura.

---

# 40. `TypeTag`

`TypeTag` deja de ser la autoridad del sistema de tipos.

Su función queda limitada a clasificación compacta de runtime.

Por tanto, elementos como:

```text
Error
TypeError
RangeError
DateTime
Duration
UUID
Regex
TaskHandle
VmRef
NativeFn
```

no deben formar parte de la taxonomía fundamental del lenguaje.

Pueden existir como tipos de plataforma/runtime.

---

# 41. Errores

Se eliminan del type taxonomy:

```text
Error
TypeError
RangeError
```

como categorías especiales de `TypeTag`.

Se convierten en tipos normales de error/excepción de la plataforma.

Ejemplo conceptual:

```text
Error
├── RuntimeError
├── IntegerOverflow
├── DivisionByZero
├── RangeError
├── TypeError
├── IOError
├── NetworkError
└── ...
```

El compilador/runtime puede optimizar algunos de ellos, pero semánticamente son parte de la plataforma.

---

# 42. `void`, `Unit` y `never`

Se introducen tres conceptos separados.

## `Unit`

Representa un único valor:

```text
()
```

Es un tipo real.

## `void`

Indica que una función no produce un valor utilizable.

## `never`

Es el bottom type.

Representa una operación que nunca produce un valor:

```text
throw
infinite loop
process termination
```

Estos tres conceptos no deben mezclarse.

---

# 43. `dynamic`

Se conserva:

```text
dynamic
```

No se introduce `unknown`.

`dynamic` representa una frontera deliberadamente dinámica.

El compilador debe tratarlo como una salida del sistema estático.

Debe estar aislado para que:

```text
typed code
```

no se degrade a:

```text
dynamic code
```

por contaminación accidental.

---

# 44. Generators, Iterators, Streams y Tasks

Estos conceptos deben separarse.

```text
Iterator<T>
```

produce valores síncronamente.

```text
Generator<T>
```

representa una función suspendible/productora.

```text
Future<T>
```

representa un resultado eventual.

```text
Task<T>
```

representa una unidad de ejecución programada.

```text
TaskHandle<T>
```

representa control/identidad sobre una tarea.

```text
Stream<T>
```

representa una secuencia potencialmente asíncrona.

No deben colapsarse en `TypeTag::Task`, `Generator`, etc. como si fueran equivalentes.

---

# 45. Range

`Range` debe convertirse en genérico:

```text
Range<T>
```

y soportar límites:

```text
start
end
inclusive/exclusive
step
```

Ejemplos:

```varn
0..5
1..=5
```

El compilador debe poder especializar rangos numéricos sin crear arrays intermedios.

---

# 46. Range y zero-allocation

Una operación:

```varn
for i in 0..1_000_000 {
    ...
}
```

no debe crear necesariamente un objeto heap.

Puede compilar directamente a:

```text
counter
limit
branch
```

La semántica de `Range<T>` no obliga a una representación heap.

---

# 47. Type Layout

`FieldRepr` deja de ser la autoridad final.

Se introduce conceptualmente:

```text
TypeLayout
```

con información como:

```text
size
align
stride
abi
representation
gc_layout
fields
niches
```

Ejemplo:

```text
TypeLayout(int)
```

produce:

```text
size = 8
align = 8
representation = I64
```

Pero:

```text
TypeLayout(Array<int>)
```

contiene información diferente:

```text
element_layout
stride
ownership
gc metadata
```

---

# 48. GC layout

`is_gc_ref: bool` es insuficiente como modelo definitivo.

Un tipo puede contener:

```text
zero references
one reference
multiple references
nested references
conditional references
```

Por tanto:

```text
GCLayout
```

debe describir las posiciones de referencias.

Ejemplo conceptual:

```text
Record {
    count: int
    name: str
    metadata: Object
}
```

tendría un mapa de referencias equivalente a:

```text
[nonref, ref, ref]
```

El GC puede utilizar esta información directamente.

---

# 49. Niche optimization

El layout engine debe poder utilizar nichos de representación.

Por ejemplo:

```text
T?
```

no necesariamente requiere:

```text
payload + boolean
```

si `T` tiene una representación con valores inválidos disponibles.

Ejemplo clásico:

```text
nullable reference
```

puede utilizar:

```text
null pointer
```

como representación de `null`.

Esto debe ser una optimización del layout, no una propiedad codificada en el type checker.

---

# 50. Unions y layout

Las unions deben disponer de:

```text
discriminant
variant layout
niche analysis
```

El compilador puede eliminar el discriminante cuando sea posible mediante nichos.

Esto permite que:

```text
T | null
```

sea extremadamente barato.

---

# 51. Structs

`struct` puede reservarse como construcción futura para tipos de datos con semántica de valor y layout controlado.

No debe introducirse únicamente para resolver la ausencia de numeric widths.

Cuando llegue, debe integrarse con:

```text
TypeLayout
ABI
GCLayout
copy semantics
move semantics
FFI
```

---

# 52. Classes

Las clases continúan siendo nominales y con identidad.

```text
class User
```

representa una entidad con identidad de objeto.

Las clases pueden:

* heredar;
* implementar interfaces;
* tener métodos;
* ser mutables;
* utilizar dispatch virtual cuando corresponda.

La representación típica será referencia gestionada.

---

# 53. Record/Object/Class

La separación definitiva:

```text
Record
    immutable value

Object
    mutable structural value

Class
    mutable nominal identity-bearing object
```

Esto debe mantenerse estable durante toda la evolución del lenguaje.

---

# 54. Arrays vs Span

```text
Array<T>
```

posee almacenamiento.

```text
Span<T>
```

es una vista.

Por tanto:

```text
Array<T> → owner
Span<T>  → view
```

Esta distinción será central para evitar copias en APIs de alto rendimiento.

---

# 55. Plataforma estándar

La stdlib se divide en tres capas.

```text
Language Core
Standard Platform
Standard Library/Ecosystem
```

El core debe permanecer pequeño.

La plataforma puede ser grande.

---

# 56. `std:core`

Incluye:

```text
Option
Result
Error
Comparable
Equatable
Hashable
Cloneable
Display
Debug
Default
```

Además:

```text
iterators
ranges
collections primitives
numeric utilities
```

---

# 57. `std:collections`

Debe proporcionar:

```text
Array
Map
Set
Deque
Queue
Stack
PriorityQueue
BitSet
```

y estructuras especializadas cuando tengan una justificación real.

La implementación puede elegir:

```text
inline
heap
packed
node-based
hash-based
```

según el tipo y uso.

---

# 58. `std:strings`

Incluye:

```text
str
StringBuilder
StringView
Unicode
graphemes
normalization
case mapping
search
split
parse
format
```

UTF-8 es la representación estándar.

---

# 59. `std:bytes`

Incluye:

```text
Bytes
ByteBuffer
Span
binary readers
binary writers
endianness
```

Los detalles físicos:

```text
u8
u16
u32
u64
```

pueden existir dentro de APIs binarias sin convertirse en tipos generales del lenguaje.

---

# 60. `std:math`

Debe incluir:

```text
abs
min
max
clamp
floor
ceil
round
trunc
sqrt
pow
exp
log
sin
cos
tan
```

y operaciones especializadas para:

```text
int
float
decimal
bigint
```

cuando corresponda.

---

# 61. Numeric safety API

La plataforma debe proporcionar:

```text
checked
wrapping
saturating
overflow detection
```

Ejemplos conceptuales:

```text
checkedAdd
wrappingAdd
saturatingAdd
```

Esto mantiene el lenguaje principal limpio.

---

# 62. `std:time`

Debe contener:

```text
DateTime
Duration
Instant
Clock
TimeZone
Calendar
```

`DateTime` y `Duration` dejan de ser tipos fundamentales del runtime type algebra.

Son tipos estándar con implementaciones potencialmente optimizadas.

---

# 63. `std:uuid`

UUID debe ser una abstracción de plataforma:

```text
UUID
```

con:

```text
parse
format
generate
compare
```

Puede utilizar físicamente:

```text
128-bit storage
```

sin crear un `u128` público.

---

# 64. `std:regex`

Regex debe pertenecer a la plataforma:

```text
Regex
Match
Capture
```

No al núcleo semántico de `TypeTag`.

---

# 65. `std:io`

Debe proporcionar:

```text
Reader
Writer
ReaderAt
WriterAt
Seek
Buffer
File
Pipe
```

La API debe favorecer `Span` y `Bytes` para minimizar copias.

---

# 66. `std:fs`

Incluye:

```text
File
Directory
Path
Permissions
Metadata
Watcher
```

con operaciones async donde corresponda.

---

# 67. `std:process`

Incluye:

```text
Process
Command
Environment
ExitStatus
Signal
ChildProcess
```

---

# 68. `std:os`

Abstrae:

```text
platform
CPU
memory
environment
signals
native handles
```

Las diferencias entre Windows/Linux/macOS deben estar detrás de APIs estables.

---

# 69. `std:thread`

Debe proporcionar:

```text
Thread
Mutex
RwLock
Semaphore
Condvar
Atomic
Barrier
Once
```

---

# 70. `std:async`

Debe proporcionar:

```text
Future<T>
Task<T>
TaskHandle<T>
CancellationToken
Executor
Channel
Stream<T>
AsyncIterator<T>
```

El runtime debe poder mapear async I/O a las primitivas nativas de cada plataforma.

---

# 71. Async I/O

La plataforma debe abstraer:

```text
Windows → IOCP
Linux   → io_uring/epoll según necesidad
BSD     → kqueue
```

sin obligar al usuario a conocer el backend.

El compilador/runtime puede elegir el mecanismo apropiado.

---

# 72. `std:net`

Debe incluir:

```text
Tcp
Udp
Socket
Address
Dns
Tls
Http
WebSocket
Quic
```

La API debe integrarse directamente con:

```text
Bytes
Span
Future
Stream
Cancellation
```

para minimizar adaptadores.

---

# 73. `std:http`

HTTP debe ser una plataforma estándar de primera clase.

Debe incluir:

```text
Request
Response
Headers
Cookies
Url
Router
Middleware
Client
Server
```

No debe requerir un framework externo para construir un servidor HTTP serio.

---

# 74. `std:json`

JSON debe estar integrado con el sistema de tipos.

Debe poder serializar:

```text
Record
Class
Enum
Tuple
Array
Map
primitive types
```

y utilizar metadatos estáticos cuando estén disponibles.

---

# 75. Serialización

La plataforma debe proporcionar un sistema general:

```text
Serialize<T>
Deserialize<T>
```

con especialización estática.

Para tipos conocidos en compile time:

```text
no reflection dinámica
no mapas intermedios
no AST JSON innecesario
```

cuando no sea necesario.

---

# 76. Formatos estructurados

La plataforma puede incluir:

```text
JSON
TOML
YAML
CSV
CBOR
MessagePack
```

pero deben reutilizar el mismo modelo de serialización.

---

# 77. Binary serialization

Debe existir un sistema binario de bajo nivel que permita:

```text
readInt
readFloat
readBytes
readStruct
writeInt
writeFloat
writeBytes
```

sin convertir cada operación en allocations.

Las representaciones físicas necesarias pueden ser internas.

---

# 78. `std:db`

La plataforma debe proporcionar una abstracción de base de datos común.

Debe incluir:

```text
Connection
Transaction
Query
Statement
Row
Pool
Migration
```

---

# 79. SQL

SQL debe integrarse como una tecnología de primera clase, no mediante un ORM gigante.

La dirección arquitectónica recomendada es:

```text
typed query
 ↓
compile-time validation
 ↓
native parameter binding
```

La stdlib no debe convertir automáticamente toda base de datos en objetos.

---

# 80. `std:crypto`

Debe incluir primitivas criptográficas estándar:

```text
hash
HMAC
HKDF
AES
ChaCha20
Ed25519
X25519
random
```

Las implementaciones deben delegar en primitivas nativas/optimizadas cuando sea posible.

---

# 81. Seguridad

Debe existir una separación estricta entre:

```text
random
```

y:

```text
secureRandom
```

Nunca se debe permitir que una API genérica de random sea presentada como criptográficamente segura.

---

# 82. `std:logging`

Debe incluir:

```text
Logger
LogLevel
StructuredLog
Sink
```

El logging estructurado debe evitar concatenaciones innecesarias.

---

# 83. `std:observability`

Debe incluir:

```text
Metrics
Tracing
Span
Counter
Gauge
Histogram
```

Debe integrarse con:

```text
HTTP
DB
Task
Network
```

sin middleware externo obligatorio.

---

# 84. `std:cli`

La CLI debe tener:

```text
Command
Argument
Option
Subcommand
Help
Completion
Prompt
Progress
```

Esto debe permitir construir herramientas profesionales sin dependencia de terceros.

---

# 85. `std:config`

Debe proporcionar:

```text
Environment
Config
Secrets
Profiles
Validation
```

y formatos:

```text
JSON
TOML
YAML
```

cuando corresponda.

---

# 86. `std:test`

Testing debe ser parte de la plataforma.

Debe soportar:

```text
test
assert
property testing
fixtures
parameterized tests
benchmarks
snapshots
```

---

# 87. `std:bench`

Debe permitir:

```text
microbenchmark
throughput
latency
allocation measurement
memory measurement
```

y producir resultados consumibles por `vn bench`.

---

# 88. `std:diagnostics`

Debe integrar:

```text
stack traces
source locations
structured errors
profiling hooks
debug metadata
```

El runtime debe generar información útil sin exigir que cada aplicación incorpore un framework de debugging.

---

# 89. `std:reflection`

Debe proporcionar:

```text
Type<T>
fields
methods
attributes
enum variants
layout
```

pero el compilador debe eliminar reflection no utilizada cuando sea posible.

---

# 90. `std:ffi`

Debe proporcionar:

```text
C ABI
native libraries
function pointers
opaque handles
native buffers
callbacks
```

`Span` y `Bytes` deben ser ciudadanos de primera clase del FFI.

---

# 91. WASM

Varn debe poder mapear:

```text
int
float
Bytes
Span
```

a representaciones compatibles con WASM sin crear una capa semántica artificial.

---

# 92. SIMD

SIMD pertenece al backend y a una API especializada:

```text
std:simd
```

No se introducen docenas de tipos vectoriales en el lenguaje principal.

El compilador puede auto-vectorizar:

```varn
for value in values {
    ...
}
```

cuando sea seguro.

---

# 93. Optimización numérica

El compilador debe realizar:

```text
constant folding
constant propagation
range analysis
overflow analysis
dead code elimination
strength reduction
loop optimization
vectorization
bounds-check elimination
escape analysis
scalar replacement
```

El objetivo es que:

```varn
int
```

sea una abstracción de alto nivel que normalmente termine siendo:

```text
CPU integer register
```

sin coste de abstracción.

---

# 94. Bounds checks

Los arrays mantienen seguridad.

```varn
values[index]
```

debe comprobar límites cuando sea necesario.

Pero el compilador puede eliminar el check cuando pueda demostrar:

```text
0 <= index < length
```

Esto debe ser una optimización estándar, no una API unsafe obligatoria.

---

# 95. Unsafe

Varn puede reservar una frontera explícita:

```text
unsafe
```

para operaciones que requieran romper garantías.

Esto permite:

* FFI;
* memoria nativa;
* SIMD especializado;
* hardware;
* allocators;
* interoperabilidad.

El código normal permanece seguro.

---

# 96. GC y tipos estáticos

El GC no debe inspeccionar dinámicamente todos los valores para descubrir referencias.

El compilador debe generar metadata basada en:

```text
TypeLayout
GCLayout
```

Así:

```text
Array<str>
```

conoce de antemano que sus elementos son referencias.

Mientras:

```text
Array<int>
```

no contiene referencias.

---

# 97. Typed execution

La VM y el JIT no deben depender de:

```text
VmValue + TypeTag
```

para cada operación caliente.

El pipeline debe ser:

```text
source
 ↓
AST
 ↓
typed AST
 ↓
TIR
 ↓
typed IR
 ↓
optimization
 ↓
machine representation
```

El tipo estático debe llegar lo más lejos posible.

---

# 98. VM

La VM puede conservar una representación universal para:

```text
dynamic
reflection
interop
debugging
```

pero las rutas estáticas deben utilizar registros especializados.

Ejemplo:

```text
AddInt
AddFloat
```

debe significar que ambos operandos ya son conocidos como tales.

No debe existir:

```text
AddInt
 ↓
check TypeTag
 ↓
fallback
```

en la ruta normal.

---

# 99. Dynamic boundary

`dynamic` crea una frontera explícita:

```text
typed world
      ↓
dynamic boundary
      ↓
runtime dispatch
```

El coste dinámico debe estar localizado.

Esto evita que un único valor dinámico contamine todo el programa.

---

# 100. TypeTag final

El actual `TypeTag` debe evolucionar.

No debe intentar representar:

```text
primitive types
collections
classes
errors
VM references
stdlib classes
```

en una sola enumeración semántica.

Debe convertirse en algo similar a:

```text
RuntimeKind
```

con categorías compactas:

```text
Null
Bool
Int
Float
BigInt
Decimal
Char
String
Array
Map
Set
Object
Class
Tuple
Function
Enum
Task
Generator
Bytes
...
```

Los detalles genéricos viven fuera de `RuntimeKind`.

---

# 101. `FieldRepr`

`FieldRepr` se reemplaza conceptualmente por:

```text
TypeLayout
```

El antiguo:

```text
size
align
is_gc_ref
```

es insuficiente.

El nuevo sistema debe poder representar:

```text
size
align
stride
ABI
scalar representation
GC layout
field offsets
variant layouts
niches
ownership
```

---

# 102. `VmValuePayload`

`Box<dyn VmValuePayload>` no debe formar parte de las rutas estáticas calientes.

Debe quedar restringido a:

```text
dynamic
native extension
interop
debugging
fallback
```

La ejecución estática debe utilizar representaciones conocidas por el compilador.

---

# 103. DateTime, Duration, UUID, Regex

Estos dejan de ser tipos primitivos del lenguaje.

Pasan a:

```text
std:time
std:uuid
std:regex
```

Esto no significa que sean lentos.

El compilador/runtime puede proporcionar representaciones nativas:

```text
Duration → integer nanoseconds / optimized representation
UUID → 128-bit value
DateTime → optimized timestamp representation
```

pero su semántica pertenece a la plataforma.

---

# 104. Qué se elimina

Se eliminan del lenguaje:

```text
double
number
real

i8
i16
i32
i64
u8
u16
u32
u64
f32
f64
```

También se evita:

```text
implicit arbitrary numeric coercion
```

y se elimina la equivalencia semántica entre:

```text
Map<K,V>
```

y:

```text
{ [key: K]: V }
```

---

# 105. Qué entra

Se incorporan formalmente:

```text
Intersection types
Literal types
Unit
Type<T>
StringView
Span<T>
Iterator<T>
Future<T>
Stream<T>
AsyncIterator<T>
```

Además:

```text
TypeLayout
GCLayout
niche optimization
capability-based operators
```

como fundamentos del compilador.

---

# 106. Qué permanece

Se mantienen:

```text
int
float
bigint
decimal

bool
char
str
null

Array
Tuple
Record
Object
Map
Set

Union
Nullable
Enum
Interface
Class
Generics

dynamic
Range
Function
Task
Generator
```

pero varias de estas construcciones se redefinen internamente con una arquitectura más estricta.

---

# 107. Filosofía definitiva

La regla central de Varn debe ser:

```text
Simple semantic model.
Rich optimizer.
```

No:

```text
Rich semantic model.
Rich runtime model.
Rich representation model.
```

El usuario debería aprender:

```text
int
float
bigint
decimal
```

y no:

```text
i8
u8
i16
u16
i32
u32
...
```

Pero el compilador debe ser perfectamente capaz de producir:

```text
i8
i16
i32
i64
f32
f64
SIMD
```

cuando la representación sea óptima.

---

# 108. Modelo final

La arquitectura completa queda:

```text
                         VARN
                          │
                   ┌──────┴──────┐
                   │             │
              Type System     Runtime
                   │             │
                   │        RuntimeKind
                   │             │
                   ▼             ▼
               Typed IR       Value/Heap
                   │
                   ▼
             Optimization
                   │
        ┌──────────┼───────────┐
        │          │           │
      Scalar      Packed      SIMD
        │          │           │
       i64        i32          vectors
       f64        u8
        │
        ▼
      Native ABI
```

La stdlib se construye encima de esto:

```text
Language Core
      │
      ├── Types
      ├── Operators
      ├── Control Flow
      └── Memory Semantics
             │
             ▼
     Standard Platform
             │
 ┌───────────┼────────────┐
 │           │            │
System      Data       Networking
 │           │            │
OS/FS      JSON/DB      HTTP/TLS
 │           │            │
Concurrency Security   Observability
 │           │            │
 └───────────┼────────────┘
             │
             ▼
      Developer Platform
             │
      ┌──────┼──────┐
      │      │      │
     Test   Bench   FFI
```

El resultado buscado no es una stdlib gigantesca alrededor de un lenguaje pequeño. Es una plataforma donde **el lenguaje, el compilador, el runtime y la stdlib comparten el mismo modelo de tipos y representación**.

La regla arquitectónica más importante para los próximos años es, por tanto:

> **No agregues un tipo al lenguaje para resolver un problema de representación. Agrega una representación al compilador.**

Así `int` puede seguir siendo simplemente `int` durante diez años, mientras el backend pasa de `i64` a optimizaciones de rango, packing, SIMD, registros especializados, escape analysis y otras técnicas sin obligar a rediseñar el lenguaje.
