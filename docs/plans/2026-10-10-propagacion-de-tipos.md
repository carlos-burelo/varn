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
   Cero «fase B» (la puerta ya no existe): async/generadores compilan
   y aparcan por ayudante.
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
- **Namespaces, rediseñados de raíz**: ver el informe que sigue.

#### Informe: rediseño de namespaces (2026-10-11)

Pedido por el dueño («corrige mi implementación de namespaces que es
defectuosa por diseño… de raíz», y después «no quiero namespaces
parchados, sino corregidos de raíz»). Un namespace ya no existe en
ejecución: es un ámbito estático. Validado en release: 3097/3097 en los
cuatro cuadrantes, corpus 196/196, cero abandonos del JIT, `vn check`
limpio sobre `std` y tests, tablas de contrato al día. Tres commits en
`main`, sin push.

**Qué estaba mal por diseño**

- *Estado en copias.* Un namespace era un objeto literal con copias de
  sus exportados: `N.n` daba 0 tras dos `N.bump()`, y `N.n = 50`
  escribía la copia.
- *Acceso lento.* Cada `N.miembro` era una carga de global más un
  `GetProperty` por nombre, también en bucles calientes.
- *Anidados invisibles entre módulos.* `Geo.Deep.twice` no compilaba
  desde otro archivo, y el anidado filtraba lo no exportado.
- *No delimitaba tipos.* Dos namespaces no podían declarar la misma
  clase (`VN4002`); interfaces, alias y anidados homónimos se pisaban en
  silencio; `A.Item` no existía como anotación de tipo.
- *Checker ciego dentro del namespace.* Resolvía los nombres de tipo
  solo en el ámbito global, así que `implements` de una clase dentro de
  un namespace no se comprobaba.

**Qué hay ahora**

- `b470ffb6` — los nombres de un namespace son suyos (`N::nombre`), no
  del módulo.
- `f29c479b` — ámbito estático.
  - Cada miembro es un global `N::miembro`; `N.m` se resuelve al
    compilar a un slot o a una llamada directa
    (`crates/varn-emit/src/body/namespace_access.rs`,
    `crates/varn-emit/src/namespace_scope.rs`).
  - El `ExportMap` del resolver, que ya fijaba los slots de módulo,
    lleva una clave por miembro
    (`crates/varn-resolver/src/namespace_exports.rs`). Reexports,
    renombres (`export { Geo as Shapes } from`), `export *` e
    `import * as L` llegan al mismo namespace y al mismo estado.
  - El binder apunta qué namespaces importa cada módulo
    (`BindResult.namespace_imports`); el emisor cambia el import del
    namespace por uno por miembro.
- `339fbc6c` — tipos del namespace.
  - El nombre registrado es cualificado (`A::Item`): dos namespaces
    pueden declarar la misma clase, enum, interfaz, alias o anidado. El
    binder apunta el nombre de cada declaración
    (`BindResult.declared_type_names`) y checker y emisor lo leen.
  - `A.Item` y `Geo.Box` valen como tipo, también importados
    (`crates/varn-binder/src/type_resolution/qualified.rs`), y
    `class Crate extends Geo.Box` funciona.
  - El checker resuelve un nombre de tipo en el ámbito donde está
    escrito (`BindView::at`).
- `26b8625c` — documentación (`docs/lang/types.md` §10.7 a §10.9).

**Decisiones de lenguaje**

- `VN4020`: un namespace no es un valor. `const g = N` o pasarlo como
  argumento es error; un miembro sí es valor (`const f = N.area`).
- `VN4021`: lo exportado es constante. `export let` es error en módulo
  y en namespace, igual que `export { x }` de un `let`. La sonda mostró
  que esto fallaba también a nivel de módulo: `import { counter }`
  nunca veía los cambios. El estado compartido se exporta como
  funciones.
- Asignar `N.m` desde fuera es error (`VN3005`).

**Coste medido de las reglas**: una sola línea en todo `std` y tests
usaba un namespace como valor (`typeof Prompt` en tests/110, quitada) y
ninguna exportaba un `let`.

**Tests**: 202 (entre módulos: reexport, renombre, `import * as`) y 203
(tipos homónimos, anotaciones con punto, herencia de una clase de
namespace importado), con los helpers `tests/helpers/namespace-*.vn`, y
diez casos nuevos en `tests/errors/`.

**Sin hacer**

- No hay medida antes/después de rendimiento: el binario base no
  ejecutaba la sonda.
- `expr_call.rs` (437 líneas) y `bind.rs` (436) siguen por encima de
  400; ya lo estaban y no se partieron.
- Lo que queda abierto en namespaces está en 2.9.

### Novena tanda (test 204)

- **La forma de un argumento sale de un solo sitio: `call_params`.**
  `guard_args`, `fill_omitted` y el ensanchado `int` -> `float`/`decimal`/
  `bigint` leían dos fuentes: la firma del llamado (`SigId` de la tabla
  de métodos, del constructor o del callee) y los parámetros que validó
  el checker (`Desugarings.call_params`, ya instanciados). En una clase
  genérica la primera tiene `T` borrado y la segunda el tipo real, así
  que la guarda y el relleno veían un tipo distinto del que validó el
  checker. Ahora `shape_of` resuelve primero `call_params` (vía
  `checked_shape`) y deja la firma como respaldo cuando no hay llamada
  validada; `settle_args`, `settle_super_args`, `guard_args` y
  `fill_omitted` reciben el `call_id`. Un método prefiere la firma
  validada a la tabla (`checked` antes que `table_sig`).
- **Se borró el caso `int` -> `float?` de `as_declared`.** Era anterior
  al ensanchado por `call_params`: hoy el argumento ya llega como
  `float` y solo se encaja en el `float?`.
- **`call_params` conserva el `rest`.** Antes lo cortaba (`take_while !
  is_rest`) y `checked_signature` mentía `has_rest: false`; ahora
  `CheckedParam` lleva `is_rest`, el ensanchado desenvuelve el interior
  del `Array` para cada argumento extra y la firma reconstruida lleva su
  `has_rest`.

Validación (perfil `quick`): cuadrante 1 `3114/3114`, corpus 200/200 con
caché nueva, cero abandonos reales del JIT, A/B pareado de forma de
llamada (3 bloques x 10, alternos): mediana 32 ms los dos binarios.
Test 204 (genérico `T`/`T?` con `int`, `rest` con `float`, omitido).

### Décima tanda (test 205)

- **`new` de clase importada con padre y de clase nativa: directo.**
  `collect_foreign_classes` saltaba ambas (`get_class_parent` y
  `is_builtin_or_intrinsic`): su `new` iba por llamada genérica y su
  tipo se opacaba a `Dynamic`. Ahora camina la cadena (`foreign_chain`,
  con visita para cortar ciclos): los campos se aplanan de raíz a hoja
  (el primero gana, igual que `ancestor_fields`), los métodos se unen,
  el constructor es el más cercano a la hoja. El emisor no cambia: la
  `ForeignClass` aplanada entra al mismo `New` directo (`AllocInstance`
  + `constructor` por nombre) que las raíces.
- Medido (sonda 1M `new` de subclase importada, `quick`, 3 bloques x 10
  alternos): 432 ns -> 324 ns por `new` (-25%). `throw new
  TypeError`/`catch`/`instanceof` intactos.
- Test 205 + helper `tests/helpers/foreign-child.vn`.

Validación (perfil `quick`): cuadrante 1 `3124/3124`, corpus 200/200 con
caché nueva, cero abandonos reales del JIT.

### Undécima tanda (tests 206 y errores)

- **El constructor efectivo se ve desde fuera y se valida.**
  Una clase sin constructor escrito ni primario igual tiene constructor:
  el emisor lo sintetiza (reenvía al padre o cero parámetros) pero el
  checker no lo veía: `new Bare(1, 2)`, `new Box2<int>(7)`,
  `super(1, 2)` y hasta `new Tally(1, 2, 3)` importado compilaban y
  soltaban los argumentos. Ahora hay un solo origen
  (`effective_ctor_ty`: el constructor más cercano en la cadena, o uno
  vacío sintetizado, con sustitución de genéricos del instanciado) que
  usan los tres sitios: `new` (`ops.rs`), `super(...)`
  (`super_call.rs`) y `ForeignClass`. Pasar de más o de menos es
  `VN3001`, local e importado.
- Medido (`new` importado sin ctor, 500k, `quick`, 3 bloques x 10
  alternos): 107 ms -> 49 ms (-54%; el genérico citado en 2.1).
- Sin rotura: `vn check` sobre `std` y `tests` no añade ningún error
  nuevo frente a la base.
- Tests 206 (construir sin ctor: campos, `super()` y genérico vacío)
  y dos casos en `tests/errors/`.

Validación (perfil `quick`): cuadrante 1 `3129/3129`, corpus 202/202
con caché nueva, cero abandonos reales del JIT.

### Duodécima tanda (test 207)

- **Un genérico pelado en anotación toma sus defaults.**

- **Un closure ya no deshace el estrechamiento al definirse.**
  Cualquier asignación dentro del cuerpo olvidaba el estrechamiento
  exterior (`const f = () => { x = null }` bastaba, aunque `f` no se
  llamara). Ahora `in_function_body` aísla el estado: lo de dentro no
  sale. A cambio, llamar un closure conocido olvida lo que ese closure
  asigna (efectos sintácticos por símbolo, también para `function`
  anidadas, IIFEs y closures en argumentos). Llamar lo que no asigna no
  toca nada.
- Sin A/B con ganancia esperada: es precisión del checker; control de
  forma de llamada para confirmar no regresión. Test 208.

Validación (perfil `quick`): cuadrante 1 `3137/3137`, corpus 202/202
con caché nueva, cero abandonos reales del JIT.

- **Un genérico pelado en anotación toma sus defaults.**
  `const c: Cell = new Cell(5)` dejaba `Cell<T>` crudo y `c.v` era
  `T`. Ahora hay un solo origen (`TypeContext::generic_defaults`, con
  delegación en los contextos envoltura): `resolve_named_type` rellena
  con los defaults cuando existen para todos. Sin defaults para lo que
  falta no cambia nada (sigue crudo y los errores de después lo cazan).
- **Literal de objeto contra interfaz genérica sustituye.**
  Apareció por el camino: ni `Box<str>` explícito pasaba
  (`'item' is 'str', expected 'I'`). `check_object_with_context`
  usaba los miembros crudos; ahora sustituye con `generic_mapping`
  cuando el origen es local. Foráneo queda crudo como antes.
- Sin A/B: solo checker, sin cambio de código generado para código
  válido. Test 207.

Validación (perfil `quick`): cuadrante 1 `3133/3133`, corpus 202/202
con caché nueva, cero abandonos reales del JIT, `vn check` sin errores
nuevos en `std` ni `tests`.

### Decimotercera tanda (test 208)

- **Un closure ya no deshace el estrechamiento al definirse.**
  Cualquier asignación dentro del cuerpo olvidaba el estrechamiento
  exterior (`const f = () => { x = null }` bastaba, aunque `f` no se
  llamara). Ahora `in_function_body` aísla el estado: lo de dentro no
  sale. A cambio, llamar un closure conocido olvida lo que ese closure
  asigna (efectos sintácticos por símbolo, también para `function`
  anidadas, IIFEs y closures en argumentos). Llamar lo que no asigna no
  toca nada.
- A/B de control contra el padre: forma de llamada, medianas iguales
  (sin regresión). Test 208.

Validación (perfil `quick`): cuadrante 1 `3137/3137`, corpus 202/202
con caché nueva, cero abandonos reales del JIT.

### Decimocuarta tanda (test 209)

- **Llenar un miembro lo estrecha (`this.v ??= 3`).**
  Faltaba el último hueco de 2.1. Claves por ruta (`MemberKey`, la
  coincidencia exacta manda y la recursión hace el resto; `?.` o
  computado no participan), creación y olvido como en variables:
  asignar resetea, llamar e instanciar olvidan todo (como TS), los
  closures aíslan por `in_function_body`. El emisor honra el tipo
  estrechado en `field_access` y el verificador lo acepta
  explícitamente; el test de coherencia existente sigue verde (`Str`
  contra `Int` se rechaza igual).
- 140 y 165 se adaptan a la paridad con variables: `?.` sobre algo
  llenado es `VN3028`, y tras una llamada hay que releer.
- Test 209. Sin A/B con ganancia: la base no compila el código
  nuevo; control de no regresión con forma de llamada (3 bloques x 10:
  medianas 27 y 28 ms).

Validación (perfil `quick`): cuadrante 1 `3143/3143`, corpus 202/202
con caché nueva, cero abandonos reales del JIT.

### Decimoquinta tanda (tests 210, 2.2 completo)

- **`compare::eq` ya no mezcla variante con int.** Las dos
  direcciones (`variante == int`, `int == variante`) devuelven
  `false`, como `1 == "a"` dinámico; el estático lo rechaza
  (`VN3010`). El `_ => return false` que queda es la decisión total
  explícita, no un caso escondido.
- **Estáticos de clase por slot (`Counter.total`).**
  `ClassInfo.statics` con tipos, productor `Resolution::StaticField`
  (la referencia a la clase es `Var` + `GlobalSlot` de su propio
  global; el alias lo impide el checker con `VN4106`) y bajada a
  `GetFixedField` / `SetFixedField` por slot con coherencia que lo
  comprueba. Solo campos planos: métodos y accesores siguen por
  nombre y el receptor dinámico ve el mismo almacén. Medido en
  `quick` (5M iters, bloques alternos): 614 ms → 183 ms (3,3x;
  instancia 39 ms). Test 210.
- `Eq` tipado descartado con medida: el genérico del JIT ya inlinea
  la identidad de bits y solo cae al helper en distinto (~5 ns); un
  opcode nuevo no paga el cruce.

Validación: cuadrante 1 `3163/3163`, corpus sin errores nuevos,
cero abandonos reales del JIT, matriz release 4/4 `3163/3163`.

### Decimosexta tanda (test 211, despacho estático directo)

- **Llamar un método estático ya no pasa por `MethodCall`.**
  `ClassInfo.statics` lleva también los métodos (`sig` incluida) en
  orden de emisión: campos y luego métodos, como el runtime. Si el
  receptor es la clase, `method_call` resuelve el closure por
  `StaticField` y emite `Call` con la firma para dar forma a los
  argumentos (sin `verify` cuando hay firma). `this`/`super` en un
  estático lo impide el checker (`VN4115`), así que el reencamine es
  exacto; el receptor dinámico y lo foráneo siguen por nombre.
  Medido en `quick` (5M llamadas, binarios alternos): 466 ms →
  88 ms (~89 ns → ~13 ns por llamada, como la de instancia).
  Test 211 (orden de slots, defaults, dinámicos, método como valor,
  `override static`, bucle caliente).

Validación (perfil `quick`): cuadrante 1 `3178/3178`, corpus sin
errores nuevos, cero abandonos reales del JIT, matriz release 4/4
`3178/3178`.

### Decimoséptima tanda (2.7 primera fase: async/generadores compilan)

- **Fuera las dos puertas de fase B** (`lower.rs:142`,
  `drive.rs:44`): la bajada `suspend` ya sabía aparcar
  (`spill` + ayudante + `longjmp` exterior).
- **Destino de `await`/`yield` desde home**: el ayudante nunca
  retorna; el verificador CLIF exige definir el valor igual.
- **Sin entrada nativa para async/generadores**: la llamada directa
  ejecutaba el cuerpo en vez de crear la tarea (test 154:
  `batch[200]` era el `acc` de `churn`). La entrada enmarcada
  sigue valiendo para ejecutar tareas.
- Sin test nuevo: el 154 existente lo cubre (fallaba, ahora pasa).
  Batería B0: checksums JIT/NO_JIT idénticos.
- Veredicto B1 (reanudar en nativo): sin cambio en esta tanda.
  Pide tabla `resume_ip`→bloque en el SSA portable, params en los
  bloques de continuación y entrada con valores desde home: cirugía
  con riesgo propio, varios días. El modelo actual (primer segmento
  en nativo, resto en intérprete) es seguro por construcción.

Validación (perfil `quick`): cuadrante 1 `3178/3178`, abandonos
«fase B» 0, matriz release 4/4 `3178/3178`.

### Decimoctava tanda (test 212, sustitución de ancestro genérico)

- **Un solo sitio: al aplanar en el binder.** Los miembros
  heredados ya viajan sustituidos (`super_type_args` por fin se
  lee); con eso `this.v` es `int`, `new IntBox(1)` valida y el
  `override` compara directo. `parent_type` construye el genérico
  para el cruce (local y foráneo con re-intern).
- **`override`** caza el `str` donde va `int` (`VN4116` directo e
  indirecto); **`implements`** contra clase genérica instancia
  sus miembros (se acabó el falso `VN4117`).
- Foráneo y `extends` pelado: camino viejo (solo se activa donde
  antes se saltaba; sin regresión posible).

Validación (perfil `quick`): cuadrante 1 `3183/3183`, corpus sin
errores nuevos (+3 ficheros), matriz release 4/4 `3183/3183`.

### Decimonovena tanda (test 213, `never` termina la rama)

- **`completes` consulta la tabla de tipos**: la sentencia-expresión
  cuya llamada tipa `never` no completa (antes todo `Expr`
  completaba). Un solo punto (vale para `if`/`else`, `switch`,
  `try`, estáticos, miembros y closures); lo demás (`break`,
  bucles, `Decl`) intacto. La cola no-`never` sigue dando
  `VN4018` (corpus +1).

Validación (perfil `quick`): cuadrante 1 pendiente, corpus sin
errores nuevos (+1 fichero), matriz release pendiente.

## 2. Pendiente, por orden

Cada item: qué falta, dónde, cómo se mide.

### 2.1 Forma de llamada: lo que queda

Hecho todo: `new` importado con y sin ctor (novena a undécima),
genérico pelado en anotación (duodécima) y estrechamiento por
asignación en variables, closures y miembros (tandas 6, 8, 13 y 14).
Queda sin inlinear el constructor ajeno y, por diseño conservador, la
rama que asigna invalida la otra y la llamada olvida miembros.

- `new` de una clase importada: hecho cuando la clase declara
  constructor y no está decorada. Hecho en la undécima tanda para clases
  sin constructor escrito: el constructor efectivo (reenvío al padre o
  vacío) se valida (`VN3001`) y viaja en `ForeignClass`, así que el
  `new` va directo. Sigue sin inlinearse: haría falta el cuerpo del
  constructor ajeno.
- Slots de método para clases importadas: medido, no hace falta. Con
  tipos exactos la llamada por nombre con caché iguala al slot (6,8 ns
  las dos).
- Hecho (novena tanda, test 204): la forma sale de `call_params`
  (`shape_of` + `checked_shape`); la firma del llamado queda como
  respaldo y se borró el caso `int` -> `float?` de `as_declared`.
- Hecho (duodécima tanda, test 207): un genérico pelado en anotación
  toma sus defaults (`TypeContext::generic_defaults`).
- Hecho (decimotercera tanda, test 208): definir un closure no toca
  el estrechamiento exterior; llamarlo olvida solo lo que asigna.
- Hecho (decimocuarta tanda, test 209): llenar un miembro lo estrecha;
  asignar lo resetea y llamar olvida. La rama que asigna invalida la
  otra por diseño conservador.

### 2.2 Tipos que `HirType` no distingue

`HirType` tiene Int, Float, Bool, Str, Ref, Dynamic, Array, Map, Set,
Tuple, Class, Nullable. Char, Bytes, Enum, Decimal, BigInt, Fn y los
tipos de objeto caen en `Ref` (`from_tir/ty.rs`) y pierden su identidad
antes de elegir opcode.

- Hecho en la tercera tanda: la variante es un slot global por módulo
  (`EnumInfo::variant_global`). Hecho en la decimoquinta tanda (test
  210): los estáticos de clase son slots en el objeto clase
  (`ClassInfo.statics`, `Resolution::StaticField` → `GetFixedField` /
  `SetFixedField` por slot). Solo campos planos: métodos y accesores
  siguen por nombre, y las variantes de enum ya iban por global. Son
  mutables, así que no valía un global aparte (divergiría de la
  lectura dinámica). Medido en `quick` (5M iters, bloques alternos):
  campo estático 123 ns → 37 ns por iter (3 lecturas + 1 escritura),
  3,3x; la llamada a método estático sigue lenta (100 ns extra) y
  queda fuera de este item. Regla: la referencia a la clase es
  `Var` + `GlobalSlot` de su propio global (un alias lo rechaza el
  checker con `VN4106`); el receptor dinámico sigue por nombre y ve
  el mismo almacén.
- `Eq` tipado, medido y descartado: el `Eq` genérico del JIT ya
  inlinea la identidad de bits (`boxed_compare`: mismos bits no
  float, o ambos SSO, sin helper) y solo cae al helper cuando los
  operandos difieren (~5 ns). Un opcode nuevo (`EqChar`/`EqEnum`)
  ahorraría solo la llamada en el caso distinto, con contenido
  (`char`) o estructura (`variant_eq`) iguales de caros. No paga el
  cruce (OpCode, SsaBinOp, VM, JIT, AOT, verificador). La caída de
  Char/Enum a `Ref` en `HirType` queda como imprecisión correcta
  (el fallback genérico decide bien).
- Hecho: `compare::eq` ya no mezcla variante con int (las dos
  direcciones devuelven `false`, como `1 == "a"` dinámico). El
  checker estático lo rechaza (`VN3010`); el `_ => return false`
  restante es la decisión total explícita (tipos distintos no son
  iguales), no un caso escondido: los `match` que sí deben romperse
  ante variantes nuevas (`text_bytes`, pares iguales) listan cada
  variante sin `_`.
- Medida: benchmarks con `Eq` 503 / `EqInt` 242 y `GetFixedField`
  841 / `GetProperty` 1878; el A/B decide (test 210: estático
  614 ms → 183 ms frente a instancia 39 ms).

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

Hecho para clases importadas raíz (`fd70b173`) y en la décima tanda
(test 205) para importadas con padre y nativas: `ForeignClass` aplana
la cadena (`foreign_chain`) y el `new` aloca directo (432 ns -> 324 ns).
Medido en `quick` (2M `new`, binarios alternos): foráneo 62 ms,
local 23 ms (~19 ns por `new` de hueco: el local inlinea el ctor,
el foráneo lo llama).
Veredicto: sin inlinear. El cuerpo ajeno no viaja en la interfaz
(`ForeignClass` lleva solo el tipo) y traerlo es compilación
cruzada (re-mapear globales y constantes del otro módulo). La
dirección dueña es el registro global (AUDIT F3/F4: borrar
`ForeignClass`); cuando los defs foráneos sean locales, el inline
sale solo. No se extiende `ForeignClass`.

### 2.6 Coste de entrar en un `try` compilado

Medido en `quick` (2M llamadas, binarios alternos): con `try` 165 ms,
sin `try` 24 ms, ~70 ns por entrada. La función compila (`clif`) pero
con activación con marco (`framed: alloc+resume`): `Try` en el chunk
fuerza `resumes_in_interpreter()` y con ello `Activation::Framed`
para toda la función, más el `push_try`/`pop_try` por entrada.
Veredicto: sin cambio. Quitarlo pide unwinding en marcos nativos
(personality + landing pads), que es el mismo proyecto que el
rediseño async (2.7): rehacerlo aquí duplicaría ese trabajo. No es
propagación de tipos; queda anotado con esta medida.

### 2.7 Funciones async sin JIT

Hecho en la decimoséptima tanda (primera fase: compilan; reanudar
sigue en intérprete). La bajada `suspend` ya existía entera
(`await_`/`yield_` derraman a home y aparcan por `longjmp` exterior);
solo la frenaban dos puertas (`lower.rs`, `drive.rs`). Al quitarlas
hicieron falta dos arreglos: el destino del `await`/`yield` se define
desde home (el ayudante nunca retorna) y la entrada nativa no se
publica para async/generadores (una llamada directa ejecutaba el
cuerpo en vez de crear la tarea: lo cazó el test 154 con `churn`
devolviendo su `acc` dentro del lote). Es seguro por construcción:
el intérprete solo entra a JIT con `ip == 0`; reanudar va siempre a
intérprete; el OSR de suspendibles sigue vetado.
Medido en `quick` (A/B pareado, 3×10 y 2×10, binarios alternos):
async sin suspensión (`churn` 400k) 43→23 ms (1,9x, corre entero
en nativo); `pingpong` 100k 62→52 ms (16%); cadena `await`
dominada por maquinaria: sin ganancia (245 vs 230 ms). Batería B0:
checksums JIT/NO_JIT idénticos; abandonos «fase B» en suite: 0
(eran 860). Queda para B1 de verdad: re-entrar a nativo en
`resume_ip` (veredicto abajo).

### 2.8 Llamada a método dinámico

Medido en `quick` (5M llamadas, 3 bloques, binarios alternos): estático
12 ns, dinámico 95 ns, dinámico sin verificar 60 ns (quitando el verify
en scratch, sin validez). La doble búsqueda existe (el verify resuelve
y el despacho vuelve a resolver), pero el despacho acierta el IC en
caliente: reencaminar a resolver-una-vez + llamada directa dio ~2x
PEOR (llamada sin IC + alloc del ligado). Veredicto: sin cambio. El
camino real es fusión verify/despacho en el JIT o memo del verify con
almacenamiento visible al GC; queda abierto con este análisis.

### 2.9 Encontrado y sin arreglar

- Namespaces: `import * as L` sigue siendo un objeto de módulo para lo
  que no es namespace (`L.f`), y se puede pasar como valor: falta darle
  el mismo trato estático. Los nombres de variante de un enum siguen
  siendo del módulo (dos enums de namespaces distintos con una variante
  homónima comparten `sum_variant_parent`, veredicto abajo). `struct`
  y tipo suma dentro de un namespace no se exportan como valor. El
  nombre de una clase de namespace se muestra como `A::Item`, no
  `A.Item`.
  Veredicto variante homónima (sondas, sin cambio): con `Red` en
  `A.Color` y `B.Mode` el `match` resuelve bien en ambos órdenes y la
  no-exhaustividad señala al que falta (`Green`, `Red`). La rama viva
  de exhaustividad indexa por tipo (`sum_type_variants[nombre]`), no
  por variante; el mapa compartido solo alimenta la rama secundaria
  (`get_enum_members_local` sin entrada en el primero), inalcanzable
  con lo que el binder produce. Endurecer la clave rompería lo que
  hoy funciona (el lector no siempre conoce al padre: es para lo que
  está el mapa).
- `BindResult.namespace_imports` y `declared_type_names` no se
  serializan: valen porque el módulo que se emite siempre se acaba de
  ligar; si algún día se emite desde un bind cacheado, hay que
  guardarlos.
- Literal de objeto contra interfaz/clase genérica foránea: los
  miembros llegan crudos (la sustitución solo cubre origen local).

- Hecho (tanda 18, test 212): sustitución del ancestro genérico
  en UN solo sitio: al aplanar en el binder (los miembros
  heredados ya viajan sustituidos; `super_type_args` nadie los
  leía). Con eso `this.v` es `int`, `new IntBox(1)` valida y el
  `override` compara directo; `parent_type` construye el genérico
  para el cruce foráneo. `implements` contra clase genérica
  instancia sus miembros igual (antes falso positivo `VN4117` en
  código correcto). Cadenas foráneas y `extends` pelado siguen el
  camino viejo (sin regresión posible: solo se activa donde antes
  se saltaba).
- Un `int` variable no se acepta donde va un `float` (solo el literal),
  pero sí donde va un `decimal` o un `bigint`. Falta decidir si eso es
  una regla o un accidente.
- El `return` de un generador se compara contra el tipo de lo que
  produce: `Generator<T>` tiene un solo parámetro y el valor final no
  tiene dónde declararse (tests/76-generator-values.vn lo esquiva).
- Con un operando `dynamic`, `variante == 1` compara el tag del enum
  (`compare::eq`); el checker lo rechaza cuando los dos tipos se
  conocen. `compare::eq` cierra además con `_ => return false`.
- Hecho (tanda 19, test 213): la llamada que tipa `never` termina
  la rama (`completes` consulta `expr_table`; vale en `if`/`else`,
  `switch`, `try`, estáticos, miembros y closures). Sigue dando
  `VN4018` la cola no-`never` (corpus +1). Nota de niveles: esas
  funciones hacen bail (`value-less return of a scalar`, van a
  intérprete); son colas frías de error, sin relevancia.
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
