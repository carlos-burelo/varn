# Modelo asíncrono de Varn: rediseño de raíz

Fecha: 2026-08-16
Estado: diseño aprobado, pendiente de plan de implementación
Baseline: `ed0af33` (árbol limpio)

---

## 1. Problema

Varn declara "runtime asíncrono" pero no lo tiene. Lo que hay es un trampolín
síncrono con nombres de async, más cuatro mecanismos de suspensión muertos que
le dan apariencia de arquitectura.

### 1.1 Mediciones sobre `ed0af33`

Todas reproducidas contra `target/release/vn.exe`.

**No hay concurrencia.**

| Prueba | Esperado si fuera async | Medido |
|---|---|---|
| `parallel([sleep(100)×3])` | ~100 ms | **303 ms** (serial) |
| `spawn(slow(120ms))`, tiempo hasta la línea siguiente | ~0 ms | **121 ms** (bloquea) |
| `await ch.rx.receive()` sin sender | error o pending | **cuelga para siempre** (exit 124) |

Las trazas de `parallel` son concluyentes: `[4] start t=0 / [4] end t=101 /
[5] start t=101`. Las tareas ni siquiera arrancan a la vez.

**Y el coste por `await` escala con el tamaño del programa.** Función `async`
que nunca suspende, mismo N=50000:

| Globals extra en el programa | `plain` | `async` |
|---|---|---|
| ~0 | 1 ms | **71 ms** |
| 600 | 1 ms | **218 ms** |

Con N=200000 y sin globals extra el ratio es **58–70x**. La causa es
`fork_for_task`, que hace `GlobalStore::clone()` en cada llamada: copia
profunda de `Vec<VmValue>`, del `FxHashMap<Rc<str>, usize>` y de
`Vec<Rc<str>>`, con un bump de refcount por cada nombre global.

Consecuencia: **cuanto más grande el programa, más caro cada `await`.** Eso no
lo arregla ningún scheduler. Es la representación.

### 1.2 Inventario de mecanismos de suspensión

Cinco mecanismos paralelos. Cuatro sin un solo productor.

| Mecanismo | Estado |
|---|---|
| `VmSuspend::Await` + `run_lazy_task_sync` | Vivo. El único. Trampolín síncrono. |
| `VmSuspend::Task` | **0 productores.** 3 consumidores que lo ignoran: `harness.rs:142`, `execute.rs:152`, `generator.rs:120`. |
| `GenChannel` | **0 llamadas a `GenChannel::new`.** `ExecCtx.gen_channel` sólo se asigna `None`. Protocolo entero (`output`, `cancel_signal`, `wake_signal`, 3 `AsyncTask`) que nunca corrió. |
| `AsyncQueue` | **Nunca se construye.** Cableado en `Value`, `TypeTag`, `HeapObj`, `Hash`, `PartialEq`, `Display`, `gc.rs:277`, `intern.rs:115/199`, `access.rs:95`, `structs.rs:95`. Su `make_iter_result` usa `value_to_nv`: el bug ya documentado y corregido en `varn-vm/src/generator.rs:11-19`. Nació roto. |
| `deferred_tasks` | Nunca se lee ni se escribe. |

### 1.3 Defectos de corrección

- **UB: `Rc` cruzando hilos.** `task.rs:28` declara `unsafe impl Send for
  AsyncTask`. Su `Inner` guarda `Value`, que es un árbol de `Rc` no atómicos.
  `net.rs:126`, dentro de `std::thread::spawn`, construye
  `Value::Str(Rc::from(...))` en el hilo worker; el hilo de la VM lo clona y lo
  dropea. Refcount no atómico tocado desde dos hilos. Los cuatro
  `thread::spawn` de `net.rs` tienen el mismo defecto. Agravante:
  `AsyncTask::settle` ejecuta los callbacks en el hilo que resuelve.
- **Deadlock sin diagnóstico.** Un `await` sobre un handle que sólo este hilo
  podría resolver cuelga el proceso. Sin error, sin timeout.
- **`TASK_POOL` es `thread_local` pero `Drop` corre en cualquier hilo.** Un
  `Inner` asignado en el hilo A vuelve al pool del hilo B.
- **Código muerto:** `set_timer`/`clear_timer`, 7 implementaciones, 0 llamadas.
  No existe `setTimeout`.

### 1.4 La causa raíz

`ExecCtx` conflaciona dos cosas distintas, y de ahí sale todo lo demás:

- **Estado de isolate** (uno por hilo): `heap`, `globals`, `modules`,
  `precompiled`, `loader`, `linker`, `settings`, `resources`,
  `proto_constants`, `static_closures`, contadores. ~14 campos.
- **Estado de computación suspendida**: `stack`, `frames`, `try_handlers`,
  `open_upvalues`, `pending_constructors`, `pending_setters`, `vm_suspend`,
  buffers del JIT. ~16 campos.

Consecuencias medibles hoy: `fork_for_task` existe sólo para simular la
separación y, como no puede, copia `globals` y `modules` y los pisa al volver
(`ctx.rs:289`, `ctx_tasks.rs:222`). `NanGenDriver` guarda un `Box<ExecCtx>`
entero por generador (`generator.rs:28`): el mismo problema resuelto por
duplicado. Los cuatro recorridos de trazado de GC del driver existen porque
posee estado que debería ser del isolate.

---

## 2. Decisiones de diseño

La sintaxis de JS/TS se conserva intacta: `async`, `await`, `Task<T>`,
`function*`, `for await`. Lo que se rediseña es la máquina debajo.

| Decisión | Elegido | Descartado y por qué |
|---|---|---|
| Representación de la suspensión | **Máquina de estados stackless**, generada por un pase sobre SSA | *Corrutina en heap*: no llega a coste cero y mantiene vivo el longjmp del JIT. *Híbrido*: dos representaciones permanentes = sistema dual prohibido por `<evolution_strategy>`. |
| Ciclo de vida de la tarea | **Concurrencia estructurada sin escape** | *Con `detach`*: el hatch se vuelve el camino por defecto (lección de `GlobalScope` en Kotlin). *Libre estilo JS*: hereda promesas flotantes, unhandled rejection y fugas; necesita linter externo para ser seguro. |
| Paralelismo | **Reparto probado por checker** sobre pool de isolates | *Un scheduler por isolate*: renuncia a los cores disponibles. *Heap compartido multinúcleo*: obliga a `Rc`→`Arc`, GC concurrente y revisar la época del JIT. Es un rediseño de VM y merece ser el proyecto siguiente, no éste. |

Las tres decisiones se pagan mutuamente. Se detalla en §3.2.

---

## 3. Arquitectura

### 3.1 El pase de transformación

Ubicación: `varn-compiler`, como pase sobre SSA.

Orden en el pipeline, ya verificado:

```
ssa::build -> passes::optimize_with -> PASE NUEVO -> assign_registers -> emisión
```

El pase va **antes** de `assign_registers` porque la asignación debe correr
sobre la función ya partida.

Entrada: una función SSA marcada `async`, `function*` o `async function*`.
Salida: una función normal (`poll`) más un layout de estado.

1. **Localizar puntos de suspensión** — `InstKind::Await` e `InstKind::Yield`,
   que ya existen en el IR.
2. **Liveness a través de cada suspensión.** El conjunto de valores SSA vivos
   *cruzando* un punto de suspensión es exactamente lo que debe guardarse.
3. **Partir el CFG** en cada suspensión. Cada región resultante es un estado.
4. **Emitir `poll`**, con entrada por cadena de comparación sobre `state.st`
   (§3.8: Varn no tiene salto indexado, y con 1-2 estados por función no hace
   falta).

**El objeto de estado mide el máximo live-set que cruza una suspensión, no el
tamaño del frame.** Lo que no cruza un `await` se queda en registros y nunca
toca memoria.

#### Fuente de la liveness

Verificado en el código: el análisis correcto **ya existe y está al nivel
correcto**, pero no donde se esperaba.

- `varn-compiler/src/ssa/emit/regs.rs:102-116` — dataflow por bloque sobre
  **valores SSA**, `live_in`/`live_out` a punto fijo sobre el CFG. Es lo que
  necesita el pase.
- `varn-regalloc/src/liveness.rs` — `LiveRange { vreg, start, end }` sobre
  **vregs de bytecode**, intervalos lineales. Nivel equivocado, no sirve.

Hoy vive como variable local dentro de `assign_registers`. Hay que **extraerlo
a un análisis SSA compartido** con dos consumidores: asignación de registros y
el pase nuevo. Es el refactor correcto con independencia de este proyecto.

#### Las tres formas del lenguaje, un solo pase

`Poll` de tres variantes sirve a las tres:

| Forma | Variantes que emite |
|---|---|
| `async function` | `Pending`, `Ready` |
| `function*` | `Yielded`, `Ready` |
| `async function*` | `Yielded`, `Pending`, `Ready` |

Difieren sólo en qué variantes puede producir el cuerpo. Escribirlas por
separado costaría casi lo mismo y abriría un periodo dual innecesario.

### 3.2 Representación de `Task<T>` en memoria

Hoy: `Box<Inner>` con `Mutex<TaskState>` + `Mutex<Vec<Box<dyn FnOnce>>>` +
`AtomicU32`, sacado de un pool `thread_local`.

Nuevo: un objeto de heap con el struct de estado inline, un discriminante y
**una sola ranura de waker**.

Las tres decisiones de §2 se refuerzan entre sí:

- El `Vec<Box<dyn FnOnce>>` desaparece porque, con concurrencia estructurada,
  **una tarea tiene exactamente un padre por construcción**. Nunca hay dos
  esperando.
- Los dos `Mutex` y el `AtomicU32` desaparecen porque el scheduler es por
  isolate y lo que cruza va por `SendValue`.
- El objeto de estado es un objeto de heap normal, con shape normal, que el GC
  **ya sabe trazar**. Mueren los cuatro recorridos custom de `NanGenDriver`.

§3.8 concreta esto: el objeto es un `ObjData` con shape sintética, y mientras
la corrutina no suspende ni siquiera existe — el estado vive en los registros
del marco de `poll`.

### 3.3 Camino de coste cero

En el call site, el compilador conoce el tamaño del estado cuando conoce el
callee:

```
estado en la pila del llamante
poll(&mut estado)
  Ready(v)  -> se usa v. NUNCA se alocó una tarea.
  Pending   -> se mueve el estado al heap y se registra en el scheduler.
```

`await f()` donde `f` no llega a suspender cuesta lo mismo que `f()`. Ahí es
donde el 58–70x medido baja a ~1.0x, y donde se supera a V8, que aloca Promise
+ microtask en todos los casos.

Con callee dinámico no se conoce el tamaño y se aloca siempre: **misma
representación, distinto sitio de alocación**. No es un sistema dual.

### 3.4 El scheduler

Ubicación: `varn-runtime`, el crate que hoy sólo tiene canales y con esto pasa
a justificar su nombre.

```
ready:  VecDeque<TaskId>
tasks:  Slab<TaskObj>
timers: BinaryHeap<(Instant, TaskId)>
io:     Poller                 // fd -> TaskId
scopes: ScopeTree
```

```
loop {
    while let Some(t) = ready.pop_front() {
        match poll(t) {
            Ready(v)   => settle(t, v),   // despierta al padre
            Pending    => {}              // ya se registró donde toca
            Yielded(v) => deliver(t, v),
        }
    }
    if nothing_pending() { break }
    io.poll(next_timer_deadline())   // ÚNICA llamada bloqueante del proceso
    expire_timers()
}
```

Propiedad que define el modelo: **`poll` nunca bloquea.** El bloqueo ocurre en
exactamente un sitio del proceso.

### 3.5 Concurrencia estructurada

Un `TaskGroup` es un nodo de scope. Invariantes:

- Toda tarea tiene scope padre. Garantizado en compilación: `spawn` fuera de un
  scope es error de compilación, no fallo en runtime.
- Salir del scope no completa mientras queden hijas vivas.
- Un error en una hija cancela a sus hermanas y sube al padre.

No requiere sintaxis nueva: `using` y `TaskGroup` ya existen en `std/task.vn` y
son forma TS. Un servidor no es excepción: sostiene su scope raíz abierto
mientras viva el proceso.

**La cancelación sale gratis con máquinas de estados.** Cada entrada a `poll`
es un punto de cancelación natural — un `if` en el prólogo. No hay que inyectar
comprobaciones ni definir "cancellation points" como hace Go, ni instrumentar
como haría falta con corrutinas stackful.

### 3.6 `Sendable(T)` y reparto a isolates

El checker debe probar tres cosas para que una tarea pueda migrar:

1. **Capturas enviables** — escalares, strings, arrays/objetos de enviables,
   enums de enviables. **No**: instancias de clase con métodos, closures sobre
   estado mutable, generadores, recursos abiertos.
2. **Tipo de retorno enviable.**
3. **No toca recursos locales del isolate** — ficheros y sockets registrados en
   *este* poller.

`Sendable(T)` es una propiedad derivada del grafo de tipos, no un sistema de
tipos nuevo. Se calcula en compilación y baja como un bit en el proto:
`<backend_principle>` literal, porque hoy el checker ya tiene esa información y
se descarta entera.

**No poder probarlo no es un error: la tarea corre local.** Degradación suave.
El peor caso del reparto es comportarse como un scheduler único, lo que lo hace
seguro de aterrizar incrementalmente.

El pool: N isolates, cada uno con su heap y su scheduler. Lo que cruza va como
`SendValue`, materializado en el heap destino — maquinaria existente y probada.
El retorno despierta la tarea padre por la misma puerta.

Y esa puerta es **la única**. Quitar el `unsafe impl Send` de `AsyncTask` deja
de ser un arreglo puntual de UB y pasa a ser el invariante que define la
arquitectura: el compilador impide que nadie abra una segunda.

### 3.7 I/O

Un `Poller` por isolate (`mio` o `polling`). Sockets no bloqueantes, registro
`fd → TaskId`.

**Tokio queda descartado**: trae su propio scheduler y su propio modelo de
tarea, y aquí la corrutina es la del VM. Sólo hace falta la capa de readiness.
Tokio ya está en el workspace pero **sólo lo usa `varn-lsp`**, así que no hay
deuda que preservar.

`fs` no tiene readiness portable: pool de hilos pequeño por isolate, cruzando
`SendValue` — que el invariante de §3.6 vuelve obligatorio a nivel de tipos, no
de disciplina.

La superficie del lenguaje no cambia. Sólo la implementación.

### 3.8 Mecánica del pase

Ronda de diseño posterior al spec original, decidida con las cifras de P2a
delante (127 puntos de suspensión, 13 en `try`, 7 en bucle, conjuntos vivos de
0 a 8 con 34 vacíos). Cierra las cinco decisiones que §3.1–§3.3 dejaban
abiertas.

#### El objeto de estado es un `ObjData`

No una variante nueva de `HeapObj`. `ObjData` **ya** es un DST con cola inline
de `Cell<VmValue>`, alocado por `ObjData::alloc(shape, n)`, `#[repr(C)]` con el
orden de campos fijado, y **el JIT ya sondea su layout** (`JitObjectLayout`,
con camino rápido inline). Es exactamente la estructura que hace falta, ya
construida y ya optimizada: reusarla da ese camino rápido el primer día.

El pase conoce el slot de cada valor en compilación, así que emite acceso por
**índice fijo** y se salta el lookup por nombre. La `shape` queda como
metadato de depuración, no como camino caliente.

Descartado añadir `HeapObj::Coro`: ahorraría una palabra por estado a cambio de
una variante más en un enum de 24, sus brazos de GC, intern, extract,
`TypeTag`, `Display` y `Hash`, y un `JitCoroLayout` que replicaría el sondeo
que `JitObjectLayout` ya hace. Descartado también un `Slab` fuera del heap: el
GC dejaría de ver los valores guardados y haría falta trazado explícito, que es
justo la deuda de `NanGenDriver` que este proyecto borra.

#### `poll` es una función de bytecode normal

Su prólogo compara el discriminante con `JumpIfTrue` encadenados. **No existe
opcode de salto indexado en Varn** (sólo `Jump`, `JumpIfFalse`, `JumpIfTrue`,
`Loop`), y las cifras dicen que no hace falta: con 127 puntos repartidos en
~100 ficheros, una función típica tiene 1-2 estados. Una cadena de 1-2
comparaciones bien predichas suele batir a un salto indirecto mal predicho.

Añadir un `JumpTable` se justifica **si aparece medida** una función con muchos
estados, no antes: tocaría el enum de opcodes, el decodificador —que este repo
mantiene como fuente única de formas de instrucción—, el dispatch del
intérprete, el lowering de Cranelift y el regalloc.

Que `poll` sea una función corriente es lo que hace que el intérprete, el
tiering y Cranelift la traten sin caso especial. Es la propiedad que permite
borrar `jit_await`, `jit_yield`, `jit_suspend_buf` y el `longjmp` **sin
construir nada que los sustituya**.

#### El estado vive en el marco de registros de `poll` hasta que suspende

Consecuencia de combinar las dos decisiones anteriores con el camino de coste
cero, y la pieza que ninguna producía por separado.

Como `poll` es una función normal, sus registros ya viven en `ExecCtx::stack`,
que el GC ya escanea como raíces. El estado no necesita slots aparte: **es el
rango de registros del propio marco de `poll`**. Mientras la corrutina no
suspende no hay objeto, no hay alocación y no hay trazado nuevo — hay una
llamada normal.

Al devolver `Pending` se copia a un `ObjData` exactamente el subconjunto que
`Liveness::live_after` reporta. Al reanudar se copia de vuelta a los registros
del marco nuevo. Con `live` entre 0 y 8, esas copias son de 0 a 8 palabras, y
sólo se pagan en la suspensión real.

#### El coste cero no depende de conocer el callee

`FunctionProto` publica el tamaño de su estado, igual que ya publica
`register_count`. El llamante lo lee **en runtime**, así que el camino de coste
cero vale también para despacho dinámico: métodos de interfaz, callbacks,
`dynamic`.

Esto va más allá de lo que §3.3 proponía. El tamaño *es* conocible en runtime;
limitarlo al callee estático dejaría fuera todo el despacho dinámico del
lenguaje sin necesidad técnica.

#### Un protocolo, dos implementaciones

```rust
enum Poll<T> { Ready(T), Pending, Yielded(T) }
```

Dos productores:

1. **Estado de corrutina** — generado por el pase, un `ObjData`.
2. **Futuro hoja** — para el host. `AsyncTask` sobrevive pero adelgaza: una
   sola ranura de waker, sin `Mutex`, sin `AtomicU32`, sin `TASK_POOL`
   `thread_local`, `!Send`.

No es un sistema dual: es un protocolo con dos formas de producirlo, como Rust
tiene `async fn` y futuros hoja implementados a mano. Responde a un hecho
medido: **19 sitios** (red, canales, isolates, timers) se resuelven desde fuera
y ninguna máquina de estados puede representarlos.

`LazyTask` y `Value::Task` **mueren**: llamar a una `async` construye su estado
directamente, sin capturar closure y args para diferirlos.

Descartado hacer que también los natives devuelvan objetos de estado: los 19
sitios tendrían que fabricar estados sintéticos con un discriminante que no
corresponde a ningún corte de CFG, y el scheduler necesitaría un `poll` nativo
especial de todas formas — el segundo concepto reaparecería disfrazado.

#### Las tareas siguen siendo perezosas

Llamar a una `async` no la arranca. **No es un cambio**: Varn ya se comporta
así hoy, verificado por sonda.

Y deja de ser un detalle para ser **requisito**: repartir una tarea a otro
isolate (§3.6) sólo es posible si no ha empezado a correr localmente. Una tarea
sin arrancar es closure + args, trivialmente enviable si sus capturas lo son;
una que ya ejecutó su primer tramo tiene estado local. Semántica eager estilo
JS y reparto multinúcleo son incompatibles.

#### Efecto lateral: el tiering encaja mejor

Hoy una función `async` es un marco largo que se entra una vez, así que el
umbral por conteo de entradas apenas la ve. Con máquinas de estados, `poll` se
entra **una vez por suspensión**: un bucle que hace `await` genera muchas
entradas y cruza el umbral de forma natural. El modelo de tiering existente
encaja mejor con el diseño nuevo que con el actual.

### 3.9 Verificación de la mecánica contra el bytecode

§3.8 fijó las decisiones razonando sobre las estructuras de Rust. Esta sección
las contrasta contra el bytecode y el IR reales, antes de escribir el pase.
**Conclusión: no hace falta ningún opcode nuevo.**

#### Acceso a los slots del estado

`GetFixedField { object, slot: u16 }` y
`SetFixedField { object, value, slot: u16 }` existen como `InstKind` y como
opcode, y toman el slot como **constante**. Es acceso indexado sin lookup de
shape: exactamente lo que §3.8 pedía cuando dijo que el pase emite acceso por
índice fijo y la `shape` queda como metadato.

`BuildObjectWithShape` construye el objeto. La decisión 1 —reusar `ObjData` en
vez de añadir `HeapObj::Coro`— queda validada contra el bytecode, no sólo
contra la estructura.

#### Partir un bloque

`Block { params: Vec<Value>, insts, term, preds }`: SSA con **argumentos de
bloque**, no con phis. Partir en la instrucción `i` es:

1. crear un bloque nuevo con los `insts[i+1..]` del original;
2. poner al original `Terminator::Jump { target: nuevo, args }`;
3. arreglar `preds` del nuevo y de los sucesores del original.

Los pases ya mutan `func.blocks` directamente —`passes/cfg.rs` hace
`func.blocks = new_blocks`—, así que no hace falta API nueva.

#### Crear valores SSA desde un pase

`ValueDef { ty: HirType }`, y `values.push(...)` sólo aparece hoy en
`build/mod.rs:78`. Un pase que cree valores necesita su propio helper; es
trivial, pero conviene saber que no existe.

#### Lo que sigue sin resolverse

- El literal que hoy devuelve `state_machine::run` es un **tamaño en
  palabras** y coincide numéricamente con `STATE_YIELDED = 1`, que es un
  **discriminante**. Namespaces distintos, cero contacto en código hoy. Debe
  ganar nombre propio antes de que el emisor escriba `state[0]`.
- La firma de `run` devolviendo `u16` no basta al partir el CFG: hará falta
  devolver también el layout (valor SSA → slot).
- El gate necesita invertir el orden para el top-level (analizar primero,
  decidir después), lo que choca con el pre-filtro O(n) que evitaría correr
  liveness completa en el 48% de funciones `async` triviales. Los dos se
  resuelven juntos o ninguno.
- `await using` no está modelado como suspensión:
  `InstKind::Dispose { is_await: true }` baja a un `CallMethod disposeAsync`
  seco. Cuando el modelo nuevo lo haga esperar de verdad, `suspend::analyze`
  tendrá que contarlo o el estado se pierde.

---

## 4. Flujo de datos: un `await` de principio a fin

```
async function f(): int { const r = await g(); return r + 1 }
```

1. **Compilación.** El pase parte `f` en dos estados: antes y después del
   `await`. `r` cruza la suspensión, así que es campo del estado. El resultado
   de `g()` no cruza nada más, así que nada más se guarda.
2. **Llamada.** El llamante lee `proto.state_size` de `f` —en runtime, así que
   da igual si `f` se conoce estáticamente— y llama a `f$poll`. El estado vive
   como los registros del marco de `f$poll`, que el GC ya escanea (§3.8).
3. **Estado 0.** Llama a `g$poll`.
   - Si `g` devuelve `Ready(v)`: `f` sigue de largo al estado 1 sin suspender.
     **No se alocó ninguna tarea, ni para `f` ni para `g`.**
   - Si devuelve `Pending`: `f` guarda `st = 1`, copia a un `ObjData` el
     subconjunto que `live_after` reporta —aquí sólo `r`— y el scheduler lo
     registra como hijo del scope actual, esperando a `g`.
4. **Suspensión.** `f$poll` devuelve `Pending`. El scheduler pasa a la
   siguiente tarea de `ready`. Si `ready` se vacía, entra en
   `io.poll(deadline)`.
5. **Despertar.** Cuando `g` se completa, `settle` escribe el resultado y
   empuja a `f` a `ready`.
6. **Reanudación.** `f$poll` entra por la cadena de comparación en `st = 1`
   (§3.8: no hay salto indexado, y con 1-2 estados no hace falta), copia `r`
   del `ObjData` de vuelta a los registros, calcula `r + 1` y devuelve `Ready`.

---

## 5. Errores y cancelación

### 5.1 La cancelación es un throw no atrapable

Viaja como un throw especial que **`catch` no puede interceptar**. Sólo corren
`finally` y `dispose`.

Es la única forma de que `using` y `try/finally` sigan significando algo bajo
cancelación. Un `catch (e)` que se tragara una cancelación rompería la garantía
del scope. Es el diseño de `BaseException` en Python y de `Cancelled` en Trio.

### 5.2 "Unhandled rejection" deja de existir

Bajo concurrencia estructurada toda tarea tiene padre y todo error tiene
destinatario. No se gestiona el problema de JS: se elimina la categoría.

### 5.3 `try`/`catch` cruzando una suspensión

Hoy `try_handlers` vive en `ExecCtx`. Debe pasar a ser parte del estado, y el
desenrollado debe funcionar entrando por `poll`. **Es la parte del pase que más
fácilmente se hace mal** y necesita cobertura de pruebas dedicada.

---

## 6. Qué se borra

Un resultado esperado del proyecto es un saldo de líneas **fuertemente
negativo**. Si no lo es, el diseño se aplicó mal.

**Mecanismos muertos:** `VmSuspend::Task`, `GenChannel`, `AsyncQueue` (con sus
brazos de GC, `HeapObj`, `TypeTag`, intern/extract), `deferred_tasks`,
`gen_channel`, `set_timer`/`clear_timer`.

**Sustituidos por el pase:** `fork_for_task`, `run_lazy_task_sync`,
`NanGenDriver` y sus cuatro recorridos de GC, `VmSuspend`, `jit_await`,
`jit_yield`, `jit_suspend_at`, `jit_suspend_buf`, el `longjmp` de suspensión.

**Sustituidos por el scheduler:** `wait_task_handle_value`, el `mpsc`, el
`thread::sleep` de `suspend_timer`, la ejecución ansiosa de `spawn_internal`,
los cuatro `thread::spawn` de `net.rs`, el `unsafe impl Send for AsyncTask`, el
`TASK_POOL` `thread_local`.

---

## 7. Secuencia de aterrizaje

Regla de orden: **ningún paso introduce código que otro paso borra, y cada paso
termina en una supresión.** Donde no se cumple, se dice explícitamente.

| # | Paso | Contenido |
|---|---|---|
| 0 | La resta | Borrar los cinco mecanismos muertos de §1.2 y `set_timer`/`clear_timer`. Cero cambio de comportamiento por construcción. |
| 1 | Extraer liveness SSA | Sacar el dataflow de `assign_registers` a un análisis compartido. Refactor puro. |
| 2 | El pase, las tres formas de golpe | `async function`, `function*` y `async function*` en un aterrizaje. Muere `fork_for_task`. |
| 3 | Scheduler + I/O readiness | `Pending` gana destino. Muere todo lo bloqueante. |
| 4 | Concurrencia estructurada | Árbol de scopes, `spawn` sin scope = error de compilación, cancelación no atrapable. |
| 5 | `Sendable(T)` + reparto | Lo último: es lo único que degrada suavemente si sale mal. |
| 6 | Cierre | `setTimeout`/`clearTimeout` reales; reescribir `docs/RUNTIME_ARCHITECTURE.md`. |

### 7.1 El único tramo no-final del plan

En el paso 2 un `Pending` todavía no tiene a dónde ir, así que
`wait_task_handle_value` sigue vivo **tal cual está**. No se escribe bloqueo
nuevo: se reutiliza el existente y lo borra el paso 3. Es supresión diferida de
código existente, no deuda nueva. Dura exactamente un paso.

### 7.2 Por qué el paso 2 entrega solo la ganancia mayor

El 58–70x se desploma en el paso 2, sin scheduler, porque la causa era la
representación y no la falta de concurrencia. Se puede medir aislado, lo que
convierte el paso más caro del proyecto en el más fácil de justificar.

---

## 8. Pruebas y criterios de fallo

Sin señal verde, no se pasa al paso siguiente.

| Paso | Señal de éxito | Criterio de fallo |
|---|---|---|
| 0 | Matriz de 4 verde; saldo de líneas fuertemente negativo | **Cualquier** cambio de comportamiento ⇒ algo no estaba muerto |
| 1 | **Bytecode idéntico byte a byte** antes/después | Cualquier diff de bytecode |
| 2 | ratio `async`/`plain` de 58–70x a **<2x**; desaparece el escalado con globals (hoy 71→218 ms con 600 globals) | Matriz de 4 en rojo, o ratio que no baja de 2x |
| 3 | `parallel([sleep(100)×3])` de **303 ms a ~100 ms**; el canal sin sender **termina** en vez de colgar | Cualquier regresión en los benchmarks existentes |
| 4 | Fugar una tarea es imposible; `finally` corre bajo cancelación; un error cancela hermanas | Existe alguna forma de fugar una tarea |
| 5 | `parallel` de N tareas CPU-bound enviables escala con cores | Migra algo no enviable, o no hay speedup |
| 6 | `setTimeout`/`clearTimeout` con cobertura propia; `RUNTIME_ARCHITECTURE.md` describe lo que existe y no lo que se planea | El documento vuelve a describir arquitectura no instanciada — el error exacto que el propio doc registra en su §2 |

### 8.1 Disciplina de medición

No negociable. Viene de errores ya pagados en este repo.

- **Nada de `cargo test`.** La señal es `tests/main.vn`, matriz de 4:
  `run`/`bench` × `VARN_NO_JIT` × std de árbol/`@embedded`. Purgar
  `vn cache clean` al cambiar de procedencia.
- **Benchmark pareado por bloques.** Dos binarios, alternar **bloques** (no
  corrida a corrida), tirar la primera corrida de cada bloque, min de 6.
  Alternar corrida a corrida mide el rebuild de caché de bytecode, no el
  código.
- **Benchmark de control** en cada tanda: la máquina termaliza duro y un A/B
  suelto miente.

### 8.2 Pruebas nuevas que hoy no existen

Las tres sondas de la auditoría pasan a `tests/` como regresión:

- `async_audit.vn` — solapamiento de `parallel`, no-eagerness de `spawn`.
- `net_audit.vn` — `accept`/`connect` concurrentes, ida y vuelta TCP.
- `deadlock_audit.vn` — canal sin sender; **debe terminar**.

Más, a partir del paso 4: fuga de tarea (debe ser error de compilación),
cancelación de hermanas ante error, `finally` bajo cancelación, `dispose`
asíncrono bajo cancelación.

---

## 9. Riesgos abiertos

Los que el plan de implementación debe cerrar con decisión explícita:

1. **`finally` que a su vez hace `await` bajo cancelación.** Si la tarea ya
   está cancelada, ¿ese `await` suspende o falla? Hace falta una regla — Trio
   lo resuelve con scopes "shielded". Sin regla, `dispose` asíncrono es
   inseguro.
2. **`try`/`catch` cruzando suspensión** (§5.3). El punto más delicado del
   pase. `try_handlers` vive hoy en `ExecCtx` y tiene que pasar a ser parte del
   estado, con el desenrollado funcionando al entrar por `poll`. **Medido en
   P2a: 13 de 127 puntos (10,2%)** — minoría, pero la cifra es del corpus
   actual, que no contiene ningún `try` con control de flujo antes del `await`.
   No leerla como «el `try` es raro, ya lo haremos en fase 2».
3. **Bucles con `await` dentro.** El corte del CFG debe tratar back-edges: un
   estado es reentrable. Es estándar, pero es donde se rompen las
   implementaciones ingenuas. **Medido en P2a: 7 de 127 puntos (5,5%)**.
3b. **El `assert_eq!` de `compute_try_depth_in`** pasa a correr en la ruta de
   compilación cuando P2b consuma `analyze`, donde un pánico sería sobre código
   de usuario. Su premisa es cierta desde `fac22b4`, pero merece revisión al
   conectarlo.
4. **Cancelación cruzando isolates.** Requiere mensaje al scheduler destino y
   un punto de recogida.
5. **Granularidad del reparto.** Repartir tareas triviales cuesta más que
   ejecutarlas. Hace falta umbral, y hay que **medirlo**, no suponerlo.
6. **`Sendable(dynamic)`.** No demostrable; siempre local. Aceptado.

---

## 10. Cambios incompatibles aceptados

- **`spawn()` deja de ejecutar hasta el final antes de retornar.** Lo que en
  `tests/main.vn` dependa de ese orden se romperá, y **debe** romperse: es la
  señal de que el trabajo funcionó.
- **`spawn` fuera de un scope pasa a ser error de compilación.**
- **`catch` deja de poder interceptar una cancelación.**

Los tres son breaking changes controlados de los que `<evolution_strategy>`
permite.
