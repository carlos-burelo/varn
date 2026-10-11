# Propagación de tipos desde los cimientos (living, 2026-10-10)

Objetivo: que lo que el checker sabe llegue intacto hasta el código
máquina, sin que ninguna capa lo pierda, lo invente o lo contradiga.
Cadena: checker `Type` → `BackendTy` (varn-tir, `varn-emit/src/ty/lower.rs`)
→ `HirType` (`varn-compiler/src/hir/mod.rs`, `from_tir/ty.rs`)
→ `SlotKind` = lo que ve el JIT.

Regla de trabajo (Ley 14): los cimientos primero. Si al avanzar aparece
una mentira de tipo, un hueco entre capas o un bug de lenguaje, se
arregla ahí, en su capa, antes de seguir. Se admiten cambios que rompen
código existente. Si este documento contradice al código, manda el
código (Ley 12).

## 0. Cómo se valida cada item

1. Matriz release de 4 cuadrantes de `tests/main.vn` (dev / `@embedded`
   × JIT / `VARN_NO_JIT=1`) + corpus `tests/errors/*.vn`. Caché nueva
   (`VARN_CACHE_DIR`) en cada corrida. No se corre `cargo test`.
2. Barrido de abandonos: `vn debug -p bails` sobre `tests/*.vn` y
   `tests/benchmarks/vn/*.vn` (ojo: `vn/`, no `tests/benchmarks/*.vn`).
   Debe quedar solo «generator/async not JIT-able in fase B».
3. Todo test nuevo se mira con `vn debug -p tiers`: una función que
   abandona pasa la suite igual, interpretada.
4. Rendimiento: A/B pareado contra el binario anterior (dos binarios,
   bloques alternos, mínimo de 3×10, descartar la primera corrida).
   `xtask compare` solo en frío: justo después de compilar dio 3, 6 y 2
   victorias en tres pasadas seguidas.
5. Un commit atómico por arreglo, en `main`. Sin stash ni ramas.

## 1. Estado

### Primera tanda (commits `0c3a2add`..`f58fc56e`, tests 185 y 186)

- Cero abandonos reales del JIT en corpus y benchmarks.
- `try/catch` compila (antes ninguna función con `try` lo hacía).
- Bucles versionados de cota constante compilan.
- Parámetro con valor por defecto recupera su tipo.
- Llamada directa a función libre entrega los argumentos con la forma
  de la firma (`int` → `int?`, argumento omitido → `null`).
- `||` y `&&` devuelven un operando; sus tipos se afinan.
- Literal de array con un elemento `dynamic` es `dynamic[]`.
- `e as Error` funciona sin imports.

Frontera de `dynamic` (sesión anterior): completa. Ver memoria
`varn-dynamic-boundary`.

### Segunda tanda (commits `b9744f27`..`6fd7a992`, tests 187 y 188)

Decisiones de lenguaje de la sección 3, y lo que destaparon:

- Índice fuera de rango lanza; leer por clave da `V?`; solo `null` y
  `false` son falsos; opcional y `null` son lo mismo.
- 2.1 hecho: toda llamada con firma conocida (método, constructor,
  valor función, `super`) entrega los argumentos con la forma
  declarada, desde un único punto del emisor (`guard_args` +
  `fill_omitted`). Se borró `lower_args_as_declared` del compilador.
- 2.5 hecho para clases importadas raíz: `ForeignClass` lleva el tipo
  del constructor.
- Leer un estático de clase o una variante de enum: 84–90 → 36–53 ns.

Bugs previos que aparecieron por el camino, cada uno arreglado en su
capa:

- El checker no validaba los argumentos de `new` (ni tipos ni aridad):
  `new K("x")` con `constructor(n: int)` compilaba y rompía en runtime.
- `m[k] ?? d` sobre `Map<str, int>` fallaba en runtime con «expected
  'int', got 'null'»: el emisor re-derivaba el tipo del elemento en
  tres sitios y decía `int`.
- Un estrechamiento restaba al tipo declarado, no al ya estrechado:
  tras `if (v == null) return`, el `else` de `Array.isArray(v)`
  recuperaba el `null`. Seis estrechadores leían el tipo cada uno por
  su cuenta.
- `function f(s?: str)` veía `s` como `str` en el cuerpo.
- Un miembro `tag?: T` no aceptaba `null` aunque se leía como `T?`.
- Un `int` literal hacia `float?` llegaba como int dentro del `float?`
  (`typeof` decía `int`).
- La rama `else` de `if (n)` con `n: int?` estrechaba a `null` aunque
  `n` fuera 0. Con la regla nueva es correcto por construcción.
- Cada lectura de propiedad por camino lento alocaba un `Arc<str>` para
  un nombre corto (SSO).

Sonda de 2.1 (ns por llamada, perfil `quick`, mínimo de 7 × 5M, mismo
estado térmico antes → después):

| Llamada | antes | después |
|---|---|---|
| método, `int` → `int?` | 64 | 20 |
| método, (`str`, `int` → `int?`) | 123 | 21 |
| método con argumento omitido | 107 | 12 |
| constructor con argumento omitido | 123 | 59 |
| valor función, `int` → `int?` | 108 | 28 |
| método con tipos exactos (referencia) | 10,6 | 10,6 |

La máquina termaliza: las sondas se repiten alternando binarios; una
sola pasada dio hasta 3× de diferencia en la misma medida.

### Tercera tanda (commits `52fbbef6`..`75074dcc`, tests 189 a 192)

- La variante de un enum es un slot global resuelto en compilación,
  local o importado: `Color.Red` 28 ns -> 1,4 ns; `LogLevel.Warn`
  importado 121 ns -> 1,2 ns (dos lecturas).
- `new` deduce los argumentos de tipo y valida siempre el constructor.
- Un constructor con valores por defecto se inlinea: 20,6 ns -> 0,4 ns
  (igual que sin defecto). El mensaje de `46ce2cab` dice «~59 ns»: esa
  cifra era de otra sonda; la medida con esta es 20,6.
- Un método de clase importada recibe los argumentos con su forma:
  `int` -> `int?` 44,6 ns -> 15,6 ns (el mismo método local: 14,8).
- El estrechamiento sigue a la asignación.

Bugs previos que aparecieron, cada uno arreglado en su capa:

- `new Box(1)` era `Box` sin instanciar: `b.value` tenía tipo `T` y el
  constructor no se validaba (`new Pair(1, "x", "tres")` compilaba).
- El valor por defecto de un parámetro de tipo (`class Cell<T = int>`)
  se parseaba y no se guardaba en ningún sitio.
- La inferencia de genéricos emparejaba uniones por posición, `null`
  incluido: un `int` dado a `label: L?` no fijaba `L`.
- `new Box<float>(2)`, `Map<str, float>.set("a", 1)` y
  `new Box<decimal>(1)` guardaban un int: la conversión se hacía contra
  la firma del llamado, que en una clase genérica tiene `T` borrado.
- El checker no visitaba el valor por defecto de un parámetro:
  `function a(x: float = "s")` compilaba, un nombre inexistente también.
  En `f(x: float = 1)` el compilador deducía del relleno que `x` era
  int, y `f(2.5)` lanzaba «expected 'int', got 'float'».
- `[...conjunto]`, `[..."ab"]`, `[...generador]` y `[...quizaNulo]`
  compilaban y lanzaban al ejecutar; `[...tupla]` también lanzaba.
- Asignar no tocaba el estrechamiento: tras `if (x == null) return`,
  `x = null; x.length` compilaba. Igual dentro de un `if` y en un bucle.
  Y al revés: ni `x = 5` quitaba el `null` de un `int?`.

### Cuarta tanda (commits `136c0b38`..`207b9a24`, tests 193 y 194)

Empezó por lo pendiente y siguió con un barrido de sondas: «¿se hace
cumplir el tipo declarado?» en cada sitio donde se declara uno.

- `new` de una clase importada aloca directo y llama al constructor por
  método: 92 ns -> 12,6 ns (un `new` local sin inlinear: 14-15).
- `toEqual` de `std:test` compara por contenido.

Reglas que el checker no hacía cumplir, cada una con su error:

- Una función con tipo de retorno debe retornar por toda ruta
  (`VN4018`). Antes `function count(): int { }` devolvía `null` tipado
  `int`, y `count() + 1` hacía paniquear a la VM.
- `override` debe poder ocupar el lugar de lo que redefine (`VN4116`).
- `implements` se comprueba en la declaración (`VN4117`).
- Un miembro abstracto heredado debe implementarse (`VN4107`).
- Estático desde una instancia, o de instancia desde la clase
  (`VN4106`). `Counter.count` con `count` de instancia desbordaba la
  pila.
- La restricción `T extends X` vale para el tipo inferido (`VN3202`).
- Un `yield` se comprueba contra el tipo que el generador declara.
- Una asignación acepta un literal entero donde va `float`, como el
  inicializador; antes `f = 2` era error y `let f: float = 2` no.
- `new Map()` sin argumentos de tipo es `VN3201` como cualquier
  genérico.
- El spread de una tupla aporta sus tipos al literal de array.

Medidas todas con `vn check` sobre `std` y `tests` antes de fijarlas:
en total rompían un sitio (`new Map()` en el test 66).

### Quinta tanda (commits `94558cfa`..`0cbadb30`, tests 195 y 196)

Siguió el barrido de sondas por `match`, namespaces, desestructuración,
operadores y herencia. Tres fallos eran de ejecución, no del checker:

- **`"a" < "b"` daba `false`** y `"b" <= "a"` daba `true`: el orden
  entre strings no tenía caso en `lt_heap` y caía a comparar dos NaN.
- **`super.m()` en un método heredado dos niveles más abajo se llamaba
  a sí mismo**: `super` se resolvía desde la clase de la instancia. Ahora
  sale de la clase dueña de la función que corre (`home_class`).
- **Una función dentro de un namespace no veía las variables de al
  lado** (`null`): el emisor les reservaba un global y nunca lo llenaba.
- `super()` no rellenaba los argumentos omitidos.

Reglas que el checker no hacía cumplir:

- Un patrón de `match` de un tipo imposible, o con más nombres que
  valores lleva la variante (`VN4015`; lo segundo rompía el emisor).
- Solo lo exportado es miembro de un namespace (`VN3004`), y sus
  nombres son del módulo: `A.f` y `B.f` chocaban en silencio (`VN4002`).
- `-`, `+`, `~` unarios y `++`/`--` exigen un número: `-"a"` valía `0`
  y `s++` sobre un `str` daba `"a1"`.
- Desestructurar un campo que no existe (`VN3004`) o una tupla con más
  nombres que elementos (`VN3016`); cada nombre toma el tipo de su
  posición.
- El constructor de una subclase llama a `super(...)` (`VN4108`) y sus
  argumentos se validan. `super.prop` y `super` dentro de una flecha
  son error (`VN4109`, `VN4114`): fallaban al ejecutar.

### Sexta tanda (commits `43a5accb`..`d0988e8b`, test 197)

- **`break` dentro de un caso de `switch` salía del bucle de fuera.**
  Un caso nunca cae al siguiente (el `switch` es una cadena de `if`);
  `break` como último enunciado del caso se descarta, en otro punto es
  `VN4005`. Caso vacío y `default` que no es el último: `VN4019`.
- **`break etiqueta` y `continue etiqueta` ignoraban la etiqueta** y
  actuaban sobre el bucle más interno. Son error hasta implementarlas.
- Un argumento nombrado que no nombra ningún parámetro, o repetido,
  dejaba el parámetro en `null`. Es `VN3001`.
- Descartar un tipo con `typeof` deja los demás en la rama contraria y
  tras un `return`.

### Séptima tanda (commits `49b7a3c7`..`c3051ec2`, test 198)

- **Cada vuelta de un `for (let i ...)` tiene su variable** cuando una
  closure la captura. Antes tres closures del bucle veían la misma `i`.
  Sin captura el bucle conserva su forma (A/B de cuatro benchmarks con
  bucles: 0,99 a 1,05).
- **`x!` comprueba de verdad.** Era solo un cambio de tipo: con `null`,
  `s!.length` daba `0`. El opcode `AssertNotNull` existía sin productor
  y su helper del JIT tenía la ABI mal (segfault al usarlo por primera
  vez).
- Miembros repetidos en una clase (`VN4110`) y accesores a medias
  (`VN3005`). La regla encontró dos casos en `std`: campos declarados
  con `let` en `std/ffi/library.vn` y un campo y un método `values` en
  `std/net/query.vn`.
- Una variante de enum se convierte a texto con su nombre, no
  `[object]`.

### Octava tanda (commits `cd15bd0d`..`339fbc6c`, tests 199 a 203)

- **Contenedores y campos mutables invariantes** (`cd15bd0d`): la
  covarianza dejaba guardar un `Cat` en un `Dog[]`. Los literales se
  adaptan al tipo pedido (`literal_fit.rs`).
- **`T` opaco en su cuerpo** (`4a49beb0`), `??` sobre `T?` genérico
  (`c2276b00`), `let`/`function` rechazados como miembro de clase
  (`9524ce16`).
- **`VN3028`** (`1b4f4e8a`): operador de nulos sobre un valor que nunca
  es `null`. Limpió 32 sitios de `std` y tests.
- **`super` en flechas, getters y campos** (`13644dad`), **etiquetas de
  bucle** implementadas (`56b7d3ee`).
- **Namespaces, rediseñados de raíz** a petición del dueño («defectuosa
  por diseño»), en tres commits:
  - `b470ffb6`: los nombres de un namespace son suyos (`N::nombre`).
  - `f29c479b`: el namespace deja de ser un objeto de runtime. Antes
    guardaba copias (`N.n` daba 0 tras dos `N.bump()`), cada acceso era
    `GetProperty` por nombre y los anidados importados no existían para
    el checker. Ahora `N.m` es un slot o una llamada directa; el
    `ExportMap` lleva una clave por miembro, así que reexports,
    renombres y `import * as` reenvían solos. Reglas: `VN4020`
    (namespace no es un valor), `VN4021` (lo exportado es constante),
    asignar `N.m` desde fuera es `VN3005`.
  - `339fbc6c`: identidad de tipos cualificada (`A::Item`). Dos
    namespaces ya pueden declarar la misma clase, enum, interfaz, alias
    o anidado, y `A.Item` vale como anotación. El checker resolvía los
    nombres de tipo solo en el ámbito global (`BindView::at`).
  - Medido con `chk`: una línea usaba un namespace como valor y ninguna
    exportaba un `let`.

## 2. Pendiente, por orden

Cada item: qué falta, dónde, cómo se mide.

### 2.1 Forma de llamada: lo que queda

- `new` de una clase importada: hecho cuando la clase declara
  constructor y no está decorada. Una clase importada sin constructor
  escrito (solo inicializadores de campo) sigue por la llamada genérica
  (92 ns): desde fuera no se ve si se sintetizó un constructor. Y sigue
  sin inlinearse: haría falta el cuerpo del constructor ajeno.
- Slots de método para clases importadas: medido, no hace falta. Con
  tipos exactos la llamada por nombre con caché iguala al slot (6,8 ns
  las dos).
- La forma de un argumento sale de dos sitios: la firma del llamado
  (`guard_args`) y los parámetros que validó el checker (`call_params`,
  usados para ensanchar y para clases sin tabla de métodos). El segundo
  es la fuente correcta; falta mover `guard_args` a él y borrar el caso
  `int` -> `float?` que quedó dentro de `as_declared`.
- Un tipo genérico escrito sin argumentos en una anotación (`x: Cell`)
  no toma los valores por defecto de sus parámetros; solo `new` lo hace.
- El estrechamiento por asignación no cubre miembros (`this.v ??= 3`),
  ni asignaciones hechas desde un closure, y una asignación en una rama
  de un `if` invalida también la otra.

### 2.2 Tipos que `HirType` no distingue

`HirType` tiene Int, Float, Bool, Str, Ref, Dynamic, Array, Map, Set,
Tuple, Class, Nullable. Char, Bytes, Enum, Decimal, BigInt, Fn y los
tipos de objeto caen en `Ref` (`from_tir/ty.rs`) y pierden su identidad
antes de elegir opcode.

- Hecho en la tercera tanda: la variante es un slot global por módulo
  (`EnumInfo::variant_global`). Quedan por nombre los estáticos de clase
  (`Contador.total`): son mutables, así que un global aparte divergiría
  de la lectura dinámica. Necesitan slots en el objeto clase
  (`ClassObj.statics` es un `HashMap`) y es lo que produciría
  `Resolution::StaticField`, que sigue sin productor.
- `Eq` tipado, medido antes de hacerlo: la identidad de bits solo vale
  para bool, instancias de clase y enums sin payload. `char` es un
  objeto de heap que se compara por contenido y una variante con payload
  se compara por estructura (`variant_eq`). El `Eq` genérico solo cae al
  helper cuando los operandos difieren: unos 5 ns en ese caso (int 3,8,
  bool 5,4, referencia 8,4, enum 10,6, char 9,0 ns con la llamada).
  Pide un opcode nuevo o Enum/Char en `HirType`; no se hizo.
- `compare::eq` cierra con `_ => return false` y deja que
  `variante == int` compare el tag: revisar contra la Ley 7 y contra la
  regla de no mezclar tipos en `==`.
- Medida: recuento `EqInt`/`Eq` y `GetFixedField`/`GetProperty` en
  benchmarks, y A/B pareado. Un mejor recuento no es una mejora de
  tiempo: se confirma con el A/B.

### 2.3 `DynReason` que deberían ser tipos

`BackendTy` lleva funciones, tareas, generadores y objetos como
`Dynamic(DynReason::{Callable, Awaitable, Yields, Shape})`. Son tipos
conocidos disfrazados de dinámicos: todo consumidor tiene que mirar la
razón para no tratarlos como `dynamic` de verdad.

- Graduarlos a variantes propias de `BackendTy` y revisar cada `match`
  (Ley 7, sin `_ =>`).
- Va antes de 2.4: los tipos de objeto con layout fijo lo necesitan.

### 2.4 Tipos de objeto con layout fijo

Los tipos de objeto son exactos (memoria `varn-object-types-are-exact`)
pero se leen por `GetProperty` con caché (1,6 ns frente a 0,6). Es la
Fase E de `2026-10-09-representacion-un-bloque-por-valor.md`; ese plan
manda en el diseño. Aquí solo se anota que es el mayor bloque de
genéricos que queda (340 `GetProperty` en benchmarks).

### 2.5 Firmas de constructores de clases externas

Hecho para clases importadas raíz (`fd70b173`). Falta: clases
importadas con padre (hoy `collect_foreign_classes` las salta) y clases
nativas (`is_builtin_or_intrinsic`). Desde la tercera tanda la forma de
los argumentos ya no depende de esto (`call_params`); lo que sigue
dependiendo es poder alocar e inlinear.

### 2.6 Coste de entrar en un `try` compilado

65 ns por entrada (2M llamadas: 130 ms compilado, 190 ms interpretado).
Viene del helper `try_push` y de exigir activación con marco
(`proto.resumes_in_interpreter()` ⇒ `Activation::Framed`). No es
propagación de tipos; se anota porque `try` es ubicuo en `std`.

### 2.7 Funciones async sin JIT

Por diseño hoy («fase B»). Es la mayor masa de código interpretado:
todo `it(...)` de la suite y buena parte de `std`. Depende del rediseño
async aprobado (memoria `varn-async-redesign-approved`).

### 2.8 Llamada a método dinámico

Hace doble búsqueda (unos 80 ns frente a 45). Pendiente de la sesión
anterior.

### 2.9 Encontrado y sin arreglar

- Namespaces: `import * as L` sigue siendo un objeto de módulo para lo
  que no es namespace (`L.f`), y se puede pasar como valor: falta darle
  el mismo trato estático. Los nombres de variante de un enum siguen
  siendo del módulo (dos enums de namespaces distintos con una variante
  homónima comparten `sum_variant_parent`). `struct` y tipo suma dentro
  de un namespace no se exportan como valor. El nombre de una clase de
  namespace se muestra como `A::Item`, no `A.Item`.
- `BindResult.namespace_imports` y `declared_type_names` no se
  serializan: valen porque el módulo que se emite siempre se acaba de
  ligar; si algún día se emite desde un bind cacheado, hay que
  guardarlos.

- `override` e `implements` no se comprueban cuando la clase o un
  ancestro es genérico: comparar firmas pide sustituir los parámetros
  de tipo del ancestro.
- Un `int` variable no se acepta donde va un `float` (solo el literal),
  pero sí donde va un `decimal` o un `bigint`. Falta decidir si eso es
  una regla o un accidente.
- El `return` de un generador se compara contra el tipo de lo que
  produce: `Generator<T>` tiene un solo parámetro y el valor final no
  tiene dónde declararse (tests/76-generator-values.vn lo esquiva).
- Con un operando `dynamic`, `variante == 1` compara el tag del enum
  (`compare::eq`); el checker lo rechaza cuando los dos tipos se
  conocen. `compare::eq` cierra además con `_ => return false`.
- El análisis de «toda ruta retorna» es sintáctico: una llamada a una
  función `never` al final de una rama no cuenta como salida.
- Una interfaz acepta un objeto literal con campos de más; un tipo de
  objeto (`{ n: int }`) no. Es deliberado, pero no está escrito.
- Etiquetas de bucle: implementarlas pide profundidad en
  `TirStmt::Break`/`Continue`, y que el emisor sepa a qué bucle del TIR
  corresponde cada bucle fuente (el `for` con `continue` y el `for-of`
  meten bucles propios) y qué `finally` cruza el salto
  (`finally_splice`).
- Dentro de una clase, `let nombre: T;` se parsea como un campo llamado
  `let` seguido del campo real: el parser acepta palabras reservadas
  como nombre de miembro. Así estaba escrito `std/ffi/library.vn`.
- `std` no se comprueba al compilar el binario: un error de tipos en
  `std` solo aparece al pasar `vn check` por sus archivos.
- `a ?? b` con `a` no nullable compila sin aviso (el `??` no hace nada);
  una `Task` que nadie espera tampoco avisa.
- Una variante con valores se imprime como `Shape.Circle(..)`, sin los
  valores.
- Los nombres de un namespace no se cualifican al emitir: por eso dos
  namespaces no pueden declarar el mismo nombre (`VN4002`). El arreglo
  de fondo es que el emisor resuelva `x` dentro de `A` como `A.x`
  (globales y `fn_index` por nombre cualificado).
- `super.prop` y `super` dentro de una flecha están prohibidos, no
  soportados: el getter del padre recursa y la flecha no tiene `this`.
- No se comprueba que `this` no se use antes de `super(...)`, ni los
  argumentos de `super(...)` cuando el padre es genérico.
- `char < char` no lo acepta el checker, aunque el runtime ya ordena.
- `[1] == [1]` es `false` (identidad) y compila sin aviso.
- Un campo declarado sin inicializador ni asignación en el constructor
  se lee como `T?`; no hay error de «campo sin inicializar».
- El valor por defecto de un patrón (`{ a = "x" }` sobre un campo
  `int`) no se comprueba contra el tipo del campo.
- Barrido hecho: funciones, clases, herencia, genéricos, generadores,
  `match`, namespaces, desestructuración, operadores. Sin barrer:
  tipos condicionales y mapeados, `struct`, tipos suma, extensiones a
  fondo, decoradores, y el paso de valores entre tareas.

## 3. Decisiones de lenguaje (tomadas 2026-10-10, delegadas por el dueño)

Criterio: una regla que quepa en una frase, que el tipo no mienta, y
ningún pecado heredado (tablas de falsos, `undefined`, huecos en
arrays). Cada una declara qué se gana, cómo se verifica y qué se borra
en su commit.

- **Índice fuera de rango lanza `RangeError`** (`b9744f27`). `xs[i]`,
  `s[i]`, `bytes[i]` y `rango[i]` tienen tipo `T` y ahora es verdad,
  también al escribir (se borra el relleno con `null`). El acceso que
  puede fallar es `at(i): T?`; crecer es `push`. Se descartó `T?`:
  obliga a `!` en cada bucle y mete nullables en el camino caliente.
- **Leer por clave da `V?`** (`e5ce9bc9`). En `Map<K, V>` y
  `{ [key: K]: V }` la clave ausente es un caso normal, no un error.
  La escritura pide `V`. Asignación compuesta aritmética sobre un
  destino nullable es error.
- **Solo `null` y `false` son falsos** (`f636c2ad`). `0`, `0.0`, `NaN`
  y `""` son verdaderos. Condicionar sobre un tipo que nunca es falso
  (int, str, array, instancia…) es error `VN3027`. Con `x: T?`,
  `if (x)` es `x != null` exacto en las dos ramas y compila a `IsNull`.
- **`&&` y `||` se afinan con la misma regla** para cualquier tipo
  (`never_false`), no solo instancias de clase.
- **Opcional y `null` son lo mismo** (`bac1cf33`, `2c1249fc`). Un
  miembro `tag?: T` es `T?`; un parámetro `p?: T` es `T?` en el cuerpo;
  quien llama puede pasar `T?` a cualquier parámetro opcional o con
  valor por defecto (el `null` toma el defecto).

Tercera tanda:

- **`new` deduce los argumentos de tipo; lo que no se deduce ni tiene
  valor por defecto es error `VN3201`** (`e3c679cc`). Se descartó
  rellenar con `dynamic`: es el `any` implícito de otros lenguajes.
- **El valor por defecto de un parámetro se comprueba contra su tipo**
  (`cbf7dad8`).
- **Un spread toma un array o una tupla, en un literal igual que en una
  llamada** (`dc287ea2`). Se descartó aceptar cualquier iterable: hoy
  no hay protocolo de iteración que el literal pueda recorrer sin
  ejecutar código (generadores), y aceptarlo a medias es peor.
- **Asignar deshace el estrechamiento; llenar con un valor no nulo lo
  crea** (`b2d9e3f7`).

Las cuatro se midieron con `vn check` sobre todo `std` y `tests` antes
de fijarlas: rompían, en total, un sitio.

Séptima tanda:

- **La variable de un `for` es de la vuelta, no del bucle**
  (`49b7a3c7`), como en `for-of`. Lo que la vuelta escribe en ella lo
  ve su closure y pasa a la vuelta siguiente.
- **`!` es una afirmación que se comprueba**, no un permiso para
  saltarse el tipo (`decff686`).

Sexta tanda:

- **Un caso de `switch` nunca cae al siguiente**, y `break` solo lo
  cierra (`cad4b450`). Agrupar valores es de `match` (`a | b`). Se
  descartó la caída de C: es la fuente clásica de casos que se ejecutan
  sin querer.
- **Un argumento nombrado inexistente es error**, no se ignora.

Quinta tanda:

- **Los strings se ordenan por sus puntos de código** (`af267731`).
- **`super(...)` es obligatorio y explícito en el constructor de una
  subclase** (`0cbadb30`). Se descartó insertarlo solo cuando el padre
  no pide argumentos: una llamada que no se ve es peor que una línea.
  Rompió dos sitios (tests/12-classes.vn).
- **Un namespace solo expone lo que exporta.**
- **Los operadores numéricos unarios no convierten**: ni `-"a"` ni
  `-true`.

Cuarta tanda:

- **Toda ruta retorna, también en una función `T?`** (`0f1b3913`). Se
  descartó el `null` implícito al caer del final: es el `undefined` de
  otros lenguajes con otro nombre.
- **Estático y de instancia no se cruzan** (`207b9a24`).
- **`override` e `implements` son contratos que se comprueban al
  declararlos**, no al usarlos (`544370a0`).
- **La anotación de un generador nombra lo que produce**, escrita corta
  (`: int`) o larga (`: Generator<int>`) (`201a8e62`).
- **Un literal entero vale donde va `float`, `decimal` o `bigint`, en
  cualquier posición** (inicializador, argumento, valor por defecto,
  asignación, `yield`), y llega convertido (`fef41b27`).

Medido antes de decidir: en todo `std` + suite solo 2 tests dependían
del `null` fuera de rango, 9 sitios de la lectura por clave no nula y
17 condiciones de un tipo que nunca es falso (12 en el test 186).

## 4. Trampas conocidas

- `VARN_NO_JIT=` vacío ya no desactiva el JIT; el cuadrante JIT de la
  matriz usa `X=1`.
- El cuadrante `@embedded` se cuelga en el test 111 si
  `target/debug/vn.exe` está viejo: recompilar debug y matar los `vn`
  colgados.
- El verificador de TIR exige aridad exacta en `DirectFn`
  (`varn-tir/src/verify/coherence/calls.rs`). No relajarlo: rellenar en
  el emisor.
- Un worktree nuevo no compila `varn-builtins` por los finales de línea
  de los contratos (`contracts.json` compara hash de bytes): copiar
  `crates/varn-builtins/src/modules` desde el checkout principal.
- Los scripts de edición deben conservar CRLF.
- Tras cambiar el compilador, purgar `~/.varn/cache`.
- Los tests 111 y 112 lanzan `target/debug/vn.exe` por ruta fija. Para
  correr los cuadrantes `@embedded` con otro perfil hay que copiar ese
  binario ahí, o el bundle se rechaza («compilado por otra version») y
  el proceso se queda colgado.
- `tests/main.vn` aborta en el primer error no capturado y solo enseña
  ese. Para ver todo lo que rompe un cambio de lenguaje: `vn check`
  sobre cada `.vn` de `std/` y `tests/`, y `vn run` de cada test suelto.
- El corpus `tests/errors/` no tiene corredor: cada archivo declara en
  su primera línea el código que `vn check` debe emitir.
- Un cuadrante `@embedded` falló una vez en el test 113 por un archivo
  temporal que dejó un `vn` matado; al repetir pasó.
- Un test que no compila hace que `tests/main.vn` salga con código 1 y
  sin línea `PASSED`. Mirar la salida antes de commitear: en la tercera
  tanda se commiteó un test roto por encadenar la corrida y el commit.
- El checker solo acepta un `int` donde va un `float` si es un literal.
  Un test que pase una variable `int` a un parámetro `float` no compila.
- `tests/errors/` y `/tmp/chk.sh` miden el impacto de una regla nueva,
  pero `std` se compila dentro del build: si `std` rompe, es el build el
  que falla, no la medición.
