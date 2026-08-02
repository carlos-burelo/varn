# Arquitectura de la VM (`varn-vm`)

VM register-based con NaN-boxing, GC generacional e inline caches, más un JIT
x86-64 (`varn-jit`) que comparte con el intérprete los mismos caches y el mismo
heap.

El intérprete **no** es una ruta legacy: es el tier base. Cualquier función que el
JIT declina compilar corre ahí, y en una corrida típica de `tests/main.vn`
alrededor del 6% de las entradas a función se interpretan aun con el JIT activo.

---

## 1. `VmValue` y NaN-boxing

Todo valor cabe en 64 bits reutilizando el espacio de Quiet NaN de IEEE 754.

| Tipo     | Sign  | Exponente | Payload                                  |
|----------|-------|-----------|------------------------------------------|
| `float`  | ±     | IEEE 754  | Valor real (cualquier patrón no-QNAN)    |
| `null`   | 0     | `111...1` | `TAG_NULL`                               |
| `false`  | 0     | `111...1` | `TAG_FALSE`                              |
| `true`   | 0     | `111...1` | `TAG_TRUE`                               |
| `int`    | 0     | `111...1` | `TAG_INT` + payload de **48 bits**       |
| SSO      | 0     | `111...1` | string de **≤ 5 bytes** inline           |
| puntero  | **1** | `111...1` | `TAG_PTR` + índice de heap de 32 bits    |

El bit de signo encendido mantiene los punteros fuera del espacio de Signalling NaN.

Los punteros guardan un **índice de heap**, no una dirección: el bit 31 distingue
nursery (0) de old-gen (1), de modo que el GC puede promover un objeto entre
generaciones sin reescribir todos los `VmValue` que lo referencian.

### Semántica de enteros

Fuente única de las reglas: `crates/varn-core/src/numeric.rs`. Los tres tiers
(const-folding en compile time, intérprete y JIT) deben ser bit-idénticos.

- `int` es de **48 bits** en complemento a dos. Rango `±2^47 - 1`.
- La aritmética entera (`+`, `-`, `*`, `**`) **envuelve a 48 bits**. No hay
  promoción silenciosa a `float` en overflow, así que los fast paths tipados del
  JIT no necesitan guards.
- `int / int` produce **siempre `float`**, incluso si la división es exacta. El
  híbrido histórico (exacto → `int`, inexacto → `float`) hacía que el tipo del
  valor dependiera de los valores en runtime.
- `int % int` produce `int` (resto truncado). Divisor cero: error de runtime.
- `int ** int` produce `int` (con wrap). Exponente negativo: error de runtime.

---

## 2. Heap

`HeapObj` (`crates/varn-vm/src/heap.rs`) aloja lo que no cabe en 64 bits:
`Str`, `Array`, `Object`, `Module`, `FrozenModule`, `VmClosure`, `Class`,
`NativeFn`, `BoundMethod`, `Map`, `Set`, `Task`, `TaskHandle`, `Range`, `Symbol`,
`EnumVariant`, `BigInt`, `Decimal`, `Char`, `Generator`, `AsyncQueue`,
`Spread`, `VmValue`.

No existe un entero boxeado: `int` es i48 inline puro y todo valor fuera de
rango envuelve (wrap mod 2^48, `varn_core::numeric`). Los enteros grandes son
del tipo `bigint`.

### Objetos: una sola allocation

Un objeto es un `Rc<ObjData>` donde `ObjData` es un DST `#[repr(C)]`: la cabecera
(shape, longitud inline, overflow) y los campos comparten **la misma allocation**,
con los campos como cola `[Cell<VmValue>]` dimensionada a la shape con la que se
construyó el objeto (modelo de in-object slots de V8).

- Instancias de clase y object literals conocen su número de campos al construirse
  → **1 malloc**.
- Los campos añadidos *después* no pueden extender la cola y caen en un **overflow
  store** lazy. La allocation **nunca se mueve**: la identidad de un objeto (`===`,
  clave de `Map`/`Set`) *es* la dirección de su `Rc`, así que realocarlo rompería en
  silencio la igualdad consigo mismo, las claves de `Map` y los clones retenidos
  dentro de payloads de enum.
- No hay `RefCell` en el camino de objeto: `VmValue` es `Copy`, así que la cola es
  de `Cell` y la mutación va sobre `&self`.

### Strings

`HeapStr` tiene cuatro formas:

- `Shared(Rc<str>)` — string inmutable.
- `Ext { buf, len }` — buffer extensible; una acumulación `s = s + x` sobre la punta
  del buffer hace append en O(1) amortizado. Se siembra cuando el operando izquierdo
  tiene ≥ 16 bytes (`EXT_SEED_LEN`).
- `Slice { src, off, len }` — vista zero-copy sobre un buffer inmutable.
- `Inline { len, bytes }` — hasta `INLINE_STR_CAP` bytes guardados **dentro** del
  objeto de heap, sin `Rc` detrás. `alloc_str_dynamic` la elige para todo string
  dinámico que pase el límite SSO pero quepa aquí, que es justo donde cae el
  `"prefijo" + <entero chico>` habitual; el `Rc::from` que pagaba antes era un
  malloc más una copia por cada uno.

  `INLINE_STR_CAP = 37` es **medido, no elegido**: es el valor más grande que deja
  `size_of::<HeapObj>()` en 48. Con 38 pasa a 64, y ese número es el *stride* de
  slot que comparte todo tipo de heap — un tercio más de memoria por objeto de
  cualquier clase para favorecer strings sería un mal negocio que ningún benchmark
  de strings mostraría. Lo fija el test `heap_obj_slot_stride_is_unchanged`.

  Un `Inline` **no** se puede rebanar en el sitio: un `Slice` toma prestado un
  buffer `Rc` que sobrevive a cualquier colección, mientras que estos bytes viven
  en el slot y se mueven con él. `alloc_substring` copia, el mismo camino que ya
  tomaba `Ext`.

Los strings de ≤ 5 bytes no llegan al heap: viven inline en el `VmValue` (SSO). Esa
es la razón de que `"User_" + i` **no** active el fast path de acumulación —
`"User_"` son exactamente 5 bytes, o sea SSO, no `Ext`. Ese concat va por el camino
genérico, que construye el resultado en un buffer de pila
(`crates/varn-vm/src/strbuf.rs`) y formatea enteros con un `itoa` propio en lugar de
`core::fmt`.

---

## 3. GC: generacional + mark-and-sweep

**Hay GC.** Este documento afirmó durante meses lo contrario ("sin GC, gestión
determinista por `Rc<RefCell<T>>`"); era falso.

### Nursery (`crates/varn-vm/src/nursery.rs`)

- `NURSERY_CAPACITY = 4096` slots; se considera lleno a 3/4.
- Los objetos nacen aquí. El GC menor **evacúa** los vivos al old-gen (promoción) y
  deja una tabla de forwarding para reparar los índices.
- **Write barrier**: escribir una referencia a nursery dentro de un objeto de old-gen
  lo anota en un remembered set; sin eso el GC menor no vería esa arista.

### Old-gen (`crates/varn-vm/src/gc.rs`)

- Mark-and-sweep tricolor (`mark_gray` / `mark_black`) sobre un
  `Vec<Option<HeapObj>>` con free list de slots.
- Raíces: registros del stack, globals, constantes de los frames vivos y módulos.

---

## 4. VM register-based

Las locales son registros en un array plano: la local del slot `k` está en
`registers[frame.base + k]` — O(1), sin hashmaps.

Cada `CallFrame` guarda `ip` (índice dentro del chunk), `base` y un puntero al
`VmClosure`. Las instrucciones operan sobre slots explícitos (`Add dst, a, b`), no
sobre un stack implícito.

---

## 5. Upvalues

- **Abierto**: mientras el frame padre vive, el upvalue apunta a su registro; las
  lecturas y escrituras se ven en tiempo real.
- **Cerrado**: al terminar el frame padre, el valor se copia al heap y la closure lo
  retiene.

---

## 6. Inline caches

Un IC site guarda hasta **8 entradas** — es polimórfico, no monomórfico:

```rust
CacheEntry { id: u32, slot: u16, is_class: u8, vtable_ver: u8 }
```

- `id` es el **shape id** del objeto (o el id de clase, según `is_class`), no un
  "class_id" a secas.
- `is_class` discrimina el tipo de hit: campo por slot, método, getter, setter,
  `.length` de array/string, transición de shape.
- `vtable_ver` invalida entradas de método cuando la clase muta su vtable.
- Un site que ve demasiadas shapes se marca **megamórfico** y deja de cachear.

Intérprete y JIT **comparten el mismo IC**: el JIT emite el escaneo de las 8 entradas
inline en el código máquina, así que un site calentado por el intérprete ya sirve al
JIT y viceversa.

---

## 7. JIT (`varn-jit`)

- **Compilación tiered, no eager**: construir un closure no compila nada. La
  compilación espera evidencia de que la función lo vale — Cranelift cuesta ~1-2 ms
  por función, y un programa corto construye muchas más funciones de las que
  ejecuta. El resultado se cachea en el `FunctionProto`, así que otro closure del
  mismo proto reusa el código.
- Si la compilación falla, `jit_entry` queda en `None` y esa función **se
  interpreta** — por eso el intérprete es obligatorio, no opcional.
- Se entra al código compilado en la primera instrucción del frame (`ip == 0`,
  `crates/varn-vm/src/exec/dispatch/mod.rs`), **o en la cabecera de un bucle** vía
  OSR (ver abajo). Las llamadas JIT→JIT saltan directo a la entrada del callee sin
  volver al dispatch.
- Fast paths inline: get/set de propiedades y de campos fijos, acceso a arrays,
  aritmética entera tipada.

### Tiering y OSR (on-stack replacement)

Hay dos formas de evidencia, porque una sola no cubre las dos formas del código:

| Evidencia | Umbral | Para qué shape |
|---|---|---|
| Entradas al frame (`jit_entry_count`) | `JIT_TIER_THRESHOLD{,_STRAIGHT}` = 128 | Funciones que se llaman muchas veces |
| Back edges (`backedge_count`) | `JIT_OSR_BACKEDGES` = 1000 | Funciones que se entran **una vez** y giran |

Contar entradas no dice nada de una función que se entra una vez y después itera un
millón de veces: nunca alcanza ningún umbral. Por eso el brazo con back edge estuvo
clavado en 1 (compilar todo bucle en su primera entrada, a ciegas). OSR es lo que
permite subirlo: **la función se compila mientras sigue corriendo**.

Cómo funciona:

1. `OpCode::Loop` cuenta el back edge. Al cruzar el umbral, guarda el ip de la
   cabecera en `ExecCtx.osr_request`, **resetea el contador a 0**, y devuelve
   `ContinueFrame` — el opcode no puede entrar a código compilado por sí mismo, hay
   que salir del bucle de dispatch.

   El reset no es cosmético. El contador vive en el **proto**, que sobrevive al
   frame: con un latch, OSR sería un evento una-vez-por-proceso y el segundo frame
   que entrara a esa función ya estaría pasado del umbral, no pediría nada, e
   interpretaría su bucle entero mientras una entrada compilada perfectamente buena
   seguía en `jit_osr_entry` sin que nadie la alcanzara. Reseteando, cada frame
   posterior vuelve a pedir tras otros 1000 back edges y recibe la entrada
   **cacheada** — `osr_jit_fn` compila una sola vez. También acota el costo de un
   rechazo: un frame que las guardas rechazan paga un round trip del frame loop cada
   1000 back edges, no uno por back edge.
2. El frame loop toma el request, verifica que pertenece a **este** frame comparando
   contra `frames[frame_idx].ip`, y pide `closure.osr_jit_fn(osr_ip)`.
3. `varn_jit::compile(..., Some(osr_ip))` lowerea **el mismo cuerpo** con un prólogo
   distinto: sin parámetros, materializa cada registro desde su home slot en
   `ctx.stack`, y salta al bloque CLIF de `osr_ip` en vez del bloque 0
   (`clif/osr.rs`).
4. La entrada resultante es un `JitFn` normal y corre por el `execute_jit_frame` de
   siempre — el setjmp, la suspensión, el unwind de excepciones y el pop del frame
   son compartidos, no duplicados.

**El requisito de representación (lo que hay que leer antes de tocar esto).** Los
slots del frame del intérprete siempre contienen `VmValue` *boxed*. El bloque en
`osr_ip` espera lo que diga `entries[&osr_ip]` — el estado del `kind_flow` —, donde
algunos registros son `i64` sin boxear y otros `f64` crudos. La conversión del
prólogo debe usar **ese estado, nunca `register_meta`**: las dos autoridades
discrepan en código real (el meta tipa un slot como `Int` mientras el flow lo mergeó
a boxed en la cabecera del bucle, y al revés). Convertir por el meta deja un entero
crudo en un registro que el lowering lee como bits de `VmValue`, lo que reinterpreta
un int pequeño como un float denormal: comparaciones mal y **bucle infinito** cuando
el registro es el contador. Por eso el prólogo reusa `alloc::reload_boxed` tal cual
—hay una sola implementación de la conversión— y `tests/62-jit-osr.vn` existe
exactamente para atrapar el error.

**Guardas.** Un OSR abandona el frame del intérprete a mitad de ejecución, así que
solo es válido cuando el frame no lleva estado que el cuerpo compilado no modele.
Cada guarda vive donde está su evidencia:

- *Handler `try` activo en este frame* → lo rechaza el intérprete; solo `ExecCtx`
  conoce la pila de handlers, y el código compilado no la modela.
- *Generador o async* → lo rechaza `osr_jit_fn`: suspenden contra el buffer del
  frame clif **más externo**, y un frame OSR por construcción no es ese.
- *`CallSelf` en el cuerpo* → lo rechaza el lowering. Es una llamada directa a
  raw@0, que bajo OSR **es** el prólogo de reanudación: ni la firma ni la función
  son las que quiere. Una función recursiva alcanza el umbral de entradas igual.
- *`osr_ip` que no es inicio de bloque*, o cuerpo por encima de `SIZE_GATE_WORDS`
  → los rechaza el lowering / la gate de tamaño.

Una variante OSR por proto: el primer bucle que se calienta se la queda.

**`mirror_home` vs `frame_aware`.** OSR fuerza `frame_aware` porque necesita `base`
para leer los home slots. Eso es la **forma del ABI**, y es una pregunta distinta de
si `ctx.stack[base + r]` tiene que seguir *espejando* el registro `r` mientras el
cuerpo corre. Lo segundo (`mirror_home`) solo hace falta por las razones que
preceden a OSR — y `has_alloc`, cuya lista de opcodes incluye `Call`, `Try`,
`Throw`, `Yield`, `Await` y los de upvalue, cubre casi todas. Cuando es falso, el
frame no puede allocar, llamar, suspender ni capturar, y **nadie puede observar esos
slots** hasta que la función retorna. Mantenerlas fusionadas hacía que cada `Move`
de un bucle OSR escribiera a memoria por una garantía que nadie reclama: medido en
~9% sobre un bucle entero de 20M iteraciones, o sea casi todo lo que OSR debía
recuperar.

**El estado vive en el proto, con su propia epoch.** `jit_osr_entry` / `jit_osr_ip`
/ `jit_osr_code` / `jit_osr_failed`, más `jit_osr_epoch` — deliberadamente *no*
`jit_epoch`: esa celda la re-estampa `clif_link::adopt_if_inherited` cuando un heap
copiado adopta la entrada normal, y ese argumento es por-entrada (compara
`jit_serial` contra el corte de la copia, que no dice nada de una variante OSR
compilada después). Compartir la celda dejaría a un heap copiado entrar a código
horneado contra los objetos de su ancestro. OSR nunca adopta; si la epoch no
coincide, recompila.

### Strings en código compilado

**Tres representaciones, una decisión.** `alloc_str_dynamic` elige en este orden:
≤ 5 bytes ASCII → SSO, inline en el `VmValue`, sin heap; ≤ `INLINE_STR_CAP` (37)
bytes → `HeapStr::Inline`, dentro del objeto de heap, sin `Rc`; el resto →
`HeapStr::Shared` detrás de un `Rc`. Ver §2 para por qué 37.

**El fast path de `StrConcat`** (`clif/strconcat.rs`). Si los dos operandos son SSO
y la suma de sus longitudes cabe en 5 bytes, el concat entero son shifts y ors sobre
valores que ya están en registros: no aloca, así que no hay nada que rootear y el
brazo inline no tiene flush, ni call, ni reload. El brazo del helper conserva su
flush/reload y los dos se juntan en un bloque merge — la misma forma que usa
`alloc::emit_backedge_safepoint` para su chequeo de nursery.

El flush vive **solo** en el brazo del helper. Subirlo antes del branch pagaría el
safepoint en el camino que existe justamente para evitarlo.

El ensamblado espeja `VmValue::try_from_sso`: los bytes de `a` ya están en su lugar,
y el byte `j` de `b` va al índice `a_len + j` del resultado, que está `8 * a_len`
bits más abajo de donde vive en `b` — un solo shift para todo el payload, porque los
bytes más allá de una longitud son cero y los dos payloads simplemente se orean. SSO
rechaza no-ASCII en la construcción, así que un valor que *es* sso es ASCII por
inducción y no hace falta testear bytes.

**Alcance real.** La guarda exige que *ambos* operandos sean SSO, y un entero nunca
lo es. O sea que esto **no** dispara en `"prefijo" + <entero>`, que es la forma del
benchmark de referencia. Medido contando llamadas a `jit_str_concat`: ~400k llamadas
para 400k concats `"a" + (i % 100)` (nunca toma el fast path) contra 0 llamadas para
400k concats `"x" + parts[i % 4]` (siempre lo toma). Llegar al caso del entero
significa emitir `itoa` en CLIF; es otro trabajo.

En la forma que sí toma: 400k concats pasan de 22 ms a 15 ms (~30%), consistente en
7 rondas pareadas con el control limpio.

### La regla del safepoint: qué se flushea y por qué

`live_boxed` responde **una** pregunta: qué registros tiene que rootear el colector.
Filtra tres cosas, cada una tirando registros en los que el colector provablemente
no tiene nada que hacer:

- **Floats** — viven en Variables `F64` que el colector nunca mira.
- **Registros muertos** — lo que `liveness` prueba que ya no se lee después de la
  instrucción actual. Dejar su home slot en paz conserva el `VmValue` (viejo pero
  válido) que garantiza el null-fill del frame, que el colector ya escanea y tolera.
- **Registros provablemente unboxed** (`is_root_kind`) — mismo argumento, un paso
  más: un registro que el kind flow tipó `Int` o `Bool` tiene un entero de máquina,
  no un índice de heap. No hay nada que rootear ni que reescribir.

El tercer filtro se **apaga entero** en una función que contiene un `Try`: una
excepción abandona el código compilado y resume en el **intérprete** en el ip del
handler, que lee todos los registros — enteros incluidos — de vuelta desde sus home
slots (`AllocCtx::narrow_roots`, calculado por `has_try`).

**Lo que `live_boxed` NO responde** es qué registros va a leer un helper. Un puñado
de helpers toma *números de registro* y lee `ctx.stack[base + reg]` por su cuenta:
la ventana de argumentos de una llamada, la de un spread, la de un native op, los
dos límites de un rango, el tag de una variante de enum. Cada uno de esos sitios
materializa su propia ventana, para todos los kinds. Antes varios se colgaban de
que el flush set alcanzara a cubrirlos, lo cual dejó de ser cierto en cuanto el set
se angostó por kind — y ninguno de esos registros es un root, son todos enteros.

`vn debug -p roots` verifica las dos mitades a la vez: contrasta nuestro set contra
los stack maps que emite Cranelift, y descuenta la columna `unboxed` porque
Cranelift marca toda Variable `I64` y no ve nuestros kinds. Sin ese descuento leería
un entero vivo sin flushear como un root perdido.

### Lo que se midió en cero

Convención del repo: una conclusión de rendimiento viene con su evidencia, y una
hipótesis **refutada** vale tanto como una confirmada porque cuesta un día
re-derivarla. Sobre el benchmark de strings (400k `"gc_" + i`), en este host y con el
protocolo de rondas alternadas más control, midieron **cero**:

| Intento | Resultado |
|---|---|
| Saltear el probe del interner de contenido en strings dinámicos | 38 ms vs 39 ms |
| `StrBuf` en pila en vez de `String` de scratch en `BuildStr` | 57 ms vs 55 ms |
| Emitir `StrConcat` tipado en vez de `Add` genérico | 39 ms vs 38 ms |
| Angostar el flush set del safepoint por kind (4 accesos a memoria menos por concat) | 30/30/43/38/39/38/37/38 vs 29/30/40/37/39/37/37/39 |
| `HeapStr::Inline` (un malloc menos por string dinámico corto) | ver abajo |

El caso de `Inline` es el más instructivo porque las dos direcciones se cancelan: en
el benchmark `"gc_" + i` (resultados de 4-9 bytes) gana en 7 de 8 rondas por 0-3 ms,
y en una banda enfocada de 12-17 bytes **pierde** por 0-2 ms. Es ruido. El
asignador de clases chicas de Rust es evidentemente lo bastante rápido como para que
sacarle el malloc no aparezca en el reloj.

Los cinco se conservaron por razones estructurales — DRY, el `<backend_principle>`,
menos código emitido, menos presión sobre el asignador — pero ninguno es un
speedup, y ninguno debería re-intentarse esperando que lo sea.

El único que **sí** midió es el fast path SSO de `StrConcat`, y solo en la forma
string+string descrita arriba.

### Frames lógicos: handshake caller→prologue

Cada activación tiene exactamente **un** CallFrame lógico en `ExecCtx.frames`, y
quién lo empuja se negocia con `ExecCtx.jit_frame_prepushed`: todo camino que
invoca un `jit_fn` habiendo empujado ya el frame (dispatch del intérprete,
`jit_call`, `jit_invoke_virtual`, `jit_construct_fast`, el sitio de `Call` tras
`jit_prepare_call`) pone el flag a 1 justo antes de la llamada; el prólogo lo lee
y lo limpia, y solo empuja cuando entró por un `CallSelf` puro (flag a 0). Todo
prólogo limpia el flag aunque no participe del protocolo — un flag rancio haría
que un prólogo safe posterior saltara un push que le toca.

`CallSelf` en funciones *safe* (puras, sin closures) es así una `call` de hardware
casi desnuda: el prólogo del callee hace depth-guard + chequeo de capacidad +
`frames.len()++` **sin escribir el contenido del slot** (nada en el cuerpo de una
función safe lee su propio frame), y el sitio de llamada decrementa al volver.
Dos reglas de hot-path pagadas con sangre en `bench_fib` (30M llamadas): no
recargar `ARG_CTX` desde `ExecCtx` en el camino común del prólogo (solo el
grow-path lo necesita) y no escribir el CallFrame — juntas costaban 2×.

Tras un longjmp de suspensión (§9), los frames lógicos con `ip == 0` que quedan
por debajo **son el mecanismo de reanudación**: el dispatch los re-ejecuta desde
cero y los `import` ya cacheados los vuelven no-ops. Por eso el balance
un-frame-por-activación es un invariante duro: un frame duplicado significa una
re-ejecución de más.

### Los layouts se prueban, no se hardcodean

El código generado necesita conocer el layout en memoria de los objetos del heap.
Esos offsets se **prueban en el arranque** contra objetos reales
(`Heap::jit_object_layout`, `Heap::jit_array_layout`) y se pasan al codegen vía
`JitHelpers`. Un offset equivocado revienta en un `assert!` al arrancar, no dentro
del código máquina.

No es ceremonia: la versión anterior llevaba los offsets del `Vec` de campos escritos
a mano (32/40/48), y cualquier cambio de representación los convertía en lecturas a
memoria liberada.

### Backend Cranelift (`varn-jit/src/clif/`)

El único backend compilado. El template JIT ya no existe: `varn_jit::compile`
lowerea con Cranelift o devuelve `Err`, y un bail deja la función al
intérprete — dos tiers semánticamente idénticos, y el `match` de
`clif/lower.rs` es la autoridad de soporte.

Los 134 opcodes de `OpCode` están ruteados: la suite completa
(`tests/main.vn`) da 745 rutas y 0 bails bajo `VARN_CLIF_TRACE=1`. Lo que
queda fuera no son opcodes sino funciones enteras, por las puertas de entrada
de `varn_jit::compile` y `clif::lower::try_compile`:

- **`code.len() > 250`** — no es un presupuesto de compilación. Es lo que
  mantiene los top-level de módulo (y otras funciones largas) fuera de clif, y
  con ellos las únicas formas que ponen un frame clif debajo de otro. Un frame
  clif no es reanudable (no tiene `ip` de bytecode propio) y
  `execute_jit_frame` instala un `setjmp` solo para el frame clif MÁS EXTERNO,
  así que un `throw` en una llamada clif anidada desarma la pila nativa de
  todos los frames clif intermedios mientras sus `CallFrame` siguen vivos con
  `ip == 0`, y el frame loop los reentra desde arriba: reejecución infinita.
  Levantar el tope exige frames clif reanudables (buffer de salto por frame
  para excepciones + ip de side-exit para suspensión), no subir el número.
- **`param_kinds.len() != arity - 1`** (`clif: missing param kinds`).
  `arity` cuenta el slot del callee (r0) MÁS los params declarados; tratarlo
  como el número de params deja el backend entero sin rutear.

- **Lowering desde bytecode**, no desde SSA (los runs cacheados `.vnc` solo
  tienen bytecode). Los opcodes tipados (`AddInt`, `LtInt`, …) son las
  pruebas del checker serializadas; `cranelift-frontend` reconstruye SSA con
  una Variable I64 por registro VM.
- **Dos funciones por compilación en un solo `JitBuffer` W^X propio** (nada
  de `cranelift-jit`): la RAW — `fn(exec_ctx, args…) -> i64` con el interior
  100% unboxed (wrap i48 = shl16/sar16 fusionado tras cada aritmética;
  recursión = `call` de hardware directa a su propia entrada) — y el WRAPPER
  con el ABI `JitFn` del template, que consume el flag de prepush, desboxea
  los args declarados `int` y re-taggea el retorno. Las únicas relocations
  admitidas (self-call y wrapper→raw) se parchean a mano.
- **Soundness por prueba, no por especulación**: `FunctionProto` lleva
  `param_kinds` y `return_kind` serializados (tipos declarados, del checker);
  un lattice de kinds flow-sensitive por punto del programa
  (`clif/kinds.rs`) valida cada lectura — el regalloc reusa un registro como
  bool aquí e int allá, así que la validación es por punto, no por registro.
  Sin deopt, sin patching, sin invalidación: nada compilado es una apuesta.
- **Arrays/globals**: walk de heap inline (espejo de `array_fast.rs` con los
  layouts probados); los rechazos van por `call_indirect` a los mismos
  helpers que usa el intérprete. Los que alocan en heap VM (literales de
  array/objeto, `push`, aritmética genérica) sí pueden disparar GC bajo un
  frame ruteado, y se cubren con safepoints que vuelcan cada registro a su
  home slot en `ctx.stack` antes del helper y lo recargan después — sin stack
  maps: las raíces se ven por los home slots. Los loops (contiguos
  post-linearización) cachean el puntero de payload por receiver invariante
  en el preheader (centinela 0), y cada acceso lo testea y salta el walk.
- Límite v1 documentado: sin guard de stack nativo (recursión más profunda
  que el stack del SO aborta en vez del error limpio de la VM).

`VARN_NO_CLIF=1` apaga Cranelift (todo va por el intérprete, igual que
`VARN_NO_JIT=1` ahora que no hay template);
`VARN_CLIF_TRACE=1` loguea cada decisión route/bail con su razón — **ante un
timing plano, trazar el ruteo antes de tocar codegen**: un bail silencioso se
disfraza exactamente de "optimización que no funcionó".

### `VARN_NO_JIT=1`

Apaga el JIT por completo: no compila (0 B de código máquina) y no entra a código
compilado. Es la herramienta para partir un fallo en "¿representación o codegen?".

Se propaga **por construcción** vía `ExecSettings` (`crates/varn-vm/src/settings.rs`)
a todo VM y contexto que se cree: isolates, cuerpos de generador, forks de task y las
VMs del bench harness. Antes eran campos con default `false` y el flag mentía en esos
tres sitios.

---

## 8. Excepciones

La VM rastrea bloques `try` sin desenrollar el stack nativo de Rust: guarda
`(frame_depth, register_depth, catch_ip)` en un `TryHandler`. Al lanzarse una
excepción, rebobina hasta esas profundidades y salta al `catch_ip`.

---

## 9. Async y suspensión

La VM es síncrona por diseño. Las operaciones asíncronas emiten un `VmSuspend`:

- `Await` — sobre un `Task` / `TaskHandle`
- `Yield` — valor de generador, enviado por el `GenChannel`

El frame queda congelado y `varn-runtime` lo reanuda cuando la tarea resuelve. Ver
[RUNTIME_ARCHITECTURE.md](RUNTIME_ARCHITECTURE.md).

---

## 10. Medición

No hay una cifra única honesta para "la VM de Varn": depende del workload.

`vn bench <archivo> -v` imprime lo que sí es medible:

- hits/misses de IC por operación
- distribución de llamadas `vm-fast` / `slow` / `native`
- allocations, promociones y corridas de GC menor/mayor
- hotspots de opcodes
- stats de JIT (funciones compiladas, bytes de código máquina, corridas JIT vs
  interpretadas)

**No copiar números de este documento a un reporte.** Medir. Ver
`<performance_rules>` en `CLAUDE.md`.

### Protocolo, cuando el número importa

Este host invirtió un efecto del 40% durante el trabajo de OSR, y un lote entero
llegó a leerse como una regresión del 50% que era puro drift térmico. Así que:

- comparar **dos binarios alternados en un mismo loop**, nunca en secuencia;
- tomar la **mediana de ≥ 7 rondas alternadas**;
- llevar en la mezcla un **control** que el cambio no pueda afectar (`fib(30)` sirve:
  no aloca). Si el control se mueve entre las dos mitades de un par, el lote se
  descarta;
- purgar la caché de compilación antes de validar, porque está indexada solo por
  hash de fuente y esconde cambios de codegen:
  `Remove-Item -Recurse -Force $env:LOCALAPPDATA\varn\cache`.

### Comparación externa (fase de strings)

400k `j.push("gc_" + i)` más 100k allocations de objetos, en funciones (un loop de
top-level nunca se compila, así que medirlo compara nuestro intérprete contra el JIT
de node). Mismo host, misma sesión, mejor de 12 corridas descartando 2 de warmup:

| | strings | objetos |
|---|---|---|
| node | 8.9 ms | 0.0 ms |
| bun | 19.9 ms | 0.1 ms |
| varn | 33 ms | 0 ms |

La fase de objetos está a la par gracias al escape analysis. La de strings no: lo
que queda es un helper fuera de línea haciendo un trabajo que node emite inline, y
cerrarlo pide allocation inline en CLIF. Nótese que estos números son de una sesión
concreta; lo que vale es la relación, no el valor absoluto.
