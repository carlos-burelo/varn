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

`HeapStr` tiene tres formas:

- `Shared(Rc<str>)` — string inmutable.
- `Ext { buf, len }` — buffer extensible; una acumulación `s = s + x` sobre la punta
  del buffer hace append en O(1) amortizado. Se siembra cuando el operando izquierdo
  tiene ≥ 16 bytes (`EXT_SEED_LEN`).
- `Slice { src, off, len }` — vista zero-copy sobre un buffer inmutable.

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

- **Compilación eager, no tiered**: `VmClosure::new` / `with_upvalues` compilan la
  función al construir el closure. No hay contador de calor. El resultado se cachea
  en el `FunctionProto`, así que otro closure del mismo proto reusa el código.
- Si la compilación falla, `jit_entry` queda en `None` y esa función **se
  interpreta** — por eso el intérprete es obligatorio, no opcional.
- Se entra al código compilado en la primera instrucción del frame (`ip == 0`,
  `crates/varn-vm/src/exec/dispatch/mod.rs`). Las llamadas JIT→JIT saltan directo a
  la entrada del callee sin volver al dispatch.
- Fast paths inline: get/set de propiedades y de campos fijos, acceso a arrays,
  aritmética entera tipada.

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

El backend optimizante. En `varn_jit::compile` el router intenta primero
Cranelift; cualquier bail cae al template JIT y de ahí al intérprete — tres
tiers semánticamente idénticos, y el `match` de `clif/lower.rs` es la
autoridad de soporte igual que `compile_proto` lo es del template.

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
  layouts probados); los rechazos van por `call_indirect` a los MISMOS
  helpers del template — ninguno de los admitidos aloca en heap VM, de modo
  que **ningún GC puede correr bajo un frame ruteado** (por eso el subset
  tampoco necesita safepoints ni stack maps todavía). Los loops (contiguos
  post-linearización) cachean el puntero de payload por receiver invariante
  en el preheader (centinela 0), y cada acceso lo testea y salta el walk.
- Límite v1 documentado: sin guard de stack nativo (recursión más profunda
  que el stack del SO aborta en vez del error limpio de la VM).

`VARN_NO_CLIF=1` apaga solo Cranelift (todo va por el template);
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
