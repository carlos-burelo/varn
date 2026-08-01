# PLAN — Cranelift: cerrar cobertura y recuperar rendimiento

Estado a 2026-07-31. Escrito para arrancarse en una sesión limpia sin volver a
derivar nada.

---

## 0.0-bis Sesión 2026-07-31: el código compilado estaba ligado al heap

### El bug que la Tarea 1 describía mal

La Tarea 1 llamaba a esto «bug de paridad de tiers en closures/upvalues, vida de
stack/upvalue». **No era ninguna de las dos cosas.** La traza de vida de
upvalues sobre el repro mostró capturas compartidas correctamente, cierres en su
sitio, y lecturas/escrituras con los bits correctos. Lo que fallaba estaba una
capa más abajo:

```
UV eq  a=0xfffd…001b [idle[object]start]   b=0xfffd…00b4 [idle→start] → false
```

`current + "→" + event` concatenó **un objeto** donde iba la constante `"→"`.

**Causa raíz: el código de Cranelift no es independiente del contexto, pero el
proto que lo guarda sí sobrevive al contexto.** `LoadConst` hornea el `VmValue`
de la constante —un handle a *un* heap— como inmediato
(`clif/lower.rs`, arm `OpCode::LoadConst`), y `CtxLinker` hornea direcciones de
los globals vivos. Un `FunctionProto`, en cambio, pertenece al chunk del módulo
y sigue vivo entre ejecuciones; `jit_entry` viajaba con él. Segunda ejecución =
heap nuevo = el índice horneado apunta a otro objeto.

Por qué parecía un bug de tiering, y por qué era tan frágil:

- Con umbral 1 casi siempre coincidían los handles entre ejecuciones (el
  interning es determinista si el orden de alocación no cambia), así que el
  fallo se escondía. Importar el módulo async 33 delante cambia ese orden.
- `head -25` de `tests/37` falla y `head -24` no: la línea 25 es **posterior**
  al assert que falla. Cambia la asignación de registros, luego el orden de
  alocación, luego qué handle significa qué. Sensibilidad de layout pura.
- `vn run` pasa siempre: una sola ejecución, un solo heap.

### El arreglo: época por heap

`HeapInner` lleva un id (`jit_epoch`) asignado al crearse; `Heap::clone`
—contexto anidado sobre los mismos objetos— lo comparte, y `deep_clone` recibe
uno nuevo (mismos índices, objetos distintos). `FunctionProto::jit_epoch` sella
para qué heap se compiló, `VmClosure::jit_fn` rechaza cualquier otra, y
`Drop for HeapInner` limpia las entradas de su época. El código reemplazado se
**retira** bajo su propia época en vez de liberarse: un contexto anidado puede
recompilar un proto que un frame clif externo está ejecutando.

Fijado por `frame::jit_epoch_tests` (3 tests) y por el gate de `vn bench`, que
ejecuta el programa dos veces y es el único que lo veía.

### Coste de compilación: el 90% se fue en un flag

`enable_verifier` es `true` **por defecto en Cranelift** y `host_isa()` nunca lo
apagaba: pagábamos el verificador de IR en cada compilación del binario
publicado. Es una herramienta de desarrollo del compilador; nuestro lowering es
fijo en tiempo de build. Apagado en release (`VARN_CLIF_VERIFY=1` lo devuelve;
siempre encendido en debug): `compilar` 50.9 → 32.3 ms, **1.58x**.

Además el reuso del `Context` de Cranelift, que la Tarea 3 §2 daba por hecho,
**no estaba en el camino real**: `clif/mod.rs` tenía el thread-local pero
`compile_piece` (`clif/lower.rs`) creaba un `Context::for_function` por pieza, y
son dos piezas por función. Ahora todo pasa por `clif::with_ctx`, único sitio
que compila y único que contabiliza tiempo de backend.

Con la instrumentación nueva (`backend_time_ns`, línea `de eso: cranelift … ·
lowering …`): **91% del coste de compilación es Cranelift, 9% nuestro lowering.**
Cualquier trabajo en el emisor rinde como mucho un 9%.

### Resultado neto, medido pareado

`vn run tests/main.vn`, dos binarios, órdenes alternados, mediana de razones:
**0.41 → 2.4x más rápido**. Benchmarks planos.

**El `execute` de `vn bench` sube igualmente respecto al número viejo, y eso es
correcto.** Un `bench --runs 1` ejecuta el programa **cinco veces** (init,
snapshot, la corrida medida, la de e2e que rehace todo el pipeline…), cada una
con su heap. Antes las cinco reusaban el código de la primera — que es
exactamente el bug. El 65 ms de antes medía un programa cuyas funciones ya
estaban compiladas contra OTRO heap; no era una medida de un arranque en frío.

Dos cosas amortiguan el coste honesto: el tiering consciente de bucles (abajo) y
la adopción por ascendencia — un heap copiado con `deep_clone` hereda las
entradas que su ancestro ya había construido en el momento de la copia
(`HeapInner::jit_ancestry` + `FunctionProto::jit_serial`), porque esa copia es
fiel y esos handles nombran lo mismo. Lo que nunca se comparte es entre
**hermanos**: el código que la corrida 1 compiló horneó handles que alocó
después de su copia, y la corrida 2 no los tiene. Ahí estaba el bug.

### Tarea 2 (barrido de umbral): desbloqueada, barrida, y la respuesta es NO

Con la época arreglada, `vn bench` pasa con umbral 1, 2, 4, 8 y 16 (836/836).
Barrido sobre tiempo real:

| | main.vn | bench_matrix | bench_str_ops |
|---|---|---|---|
| umbral 1 | 552 ms | **90 ms** | **240 ms** |
| umbral 8 | **99 ms** | 218 ms | 386 ms |

El 5.6x de `main.vn` es un artefacto de una suite de correctitud que llama cada
función ~3 veces. `bench_matrix` empeora **2.4x**: contar *entradas de frame* es
ciego a los bucles — una función que se entra una vez e itera un millón de veces
no alcanza ningún umbral.

**Arreglado partiendo el umbral en dos según una propiedad estática del
bytecode**, sin necesitar OSR: si el cuerpo contiene `OpCode::Loop`
(`FunctionProto::has_backedge`, decodificado una vez y memoizado) se compila en
la primera entrada — puede no haber una segunda; si no lo contiene, el conteo de
entradas es exactamente la evidencia correcta y se exige 8.

Barrido del brazo recto sobre `main.vn` (execute, mediana de 3 rondas
alternadas): 1 → 548 ms, 2 → 488, 4 → 204, **8 → 176**, 16 → 168, 32 → 163. A
partir de 8 la curva es plana y el riesgo no. `VARN_JIT_TIER` (fuerza ambos
brazos) y `VARN_JIT_TIER_STRAIGHT` (sólo el recto) quedan para rebarrer.

Resultado: `tests/main.vn` bajo `vn bench --runs 1` **897 → 149 ms** de execute,
cobertura clif 97% (las frames que quedan interpretadas son cuerpos de test
rectos que no valían 2 ms de Cranelift), y los benchmarks planos — `matrix`
1.01, `dto` 1.0, `fib` 1.0, `str_ops` 0.92 pareados contra el árbol pre-sesión.

### El dato que ordena todo lo demás: en esta suite compilar NO paga

Medido sobre `tests/main.vn`, una ejecución:

| | fns compiladas | compilar | execute |
|---|---|---|---|
| `VARN_JIT_TIER=1` (todo) | 517 | 1.133 s | 886 ms |
| default (bucles eager, recto ≥8) | 241 | 349 ms | 281 ms |
| **`VARN_NO_JIT=1`** | 0 | 0 | **42 ms** |

Cualquier dosis de compilación es peor que interpretar, porque cada ejecución
compila ~500 funciones a 1.5-2.2 ms y luego llama a cada una unas tres veces.
Ningún umbral arregla eso: es la razón por la que el `505c004` medía 23 ms
—compilaba con el template JIT, ~19 µs por función, 65-100x más barato— y por la
que el 45 ms del 07-30 sólo existía reusando entre heaps.

El barrido del brazo de bucles deja el conflicto a la vista:

| umbral de bucles | main.vn execute | bench_matrix |
|---|---|---|
| 1 | 244-265 ms | **156-185 ms** |
| 4 | 80-85 ms | 282-316 ms |
| 8 | **63-66 ms** | 264-304 ms |

La suite quiere no compilar; el código real quiere compilar sus bucles en la
primera entrada. Se queda en 1: optimizar el número para la suite es optimizar
para el perfil equivocado.

### La palanca que queda: código independiente del heap

Es la misma que la Tarea 3 §3 (compartir código entre isolates) y ahora tiene
una segunda razón: si el código no hornea handles, se reusa entre corridas y
entre isolates, y el coste de compilación por heap desaparece. Lo que hay que
quitar del inmediato:

1. `LoadConst` → cargar de la tabla de constantes de la closure en tiempo de
   ejecución. Requiere el puntero a la closure también en funciones no
   frame-aware (hoy sólo lo reciben las frame-aware).
2. `ClifTarget::expected_bits` (guarda del call directo) es un handle: no rompe
   corrección —la guarda falla y cae al camino lento— pero se pierde la llamada
   directa tras cambiar de heap.

Nota aparte, del mismo linaje y sin cerrar: `ExecCtx::proto_constants` NO es
raíz de GC, aunque sus `VmValue` son handles vivos. Hoy se salva porque las
constantes también están en `frame.closure().constants` mientras hay frames.

---

## 0.0 Corrección de rumbo (2026-07-30, tarde)

**Este plan medía la métrica equivocada.** Las Tareas 1-4 son todas cobertura
clif y coste de compilación sobre `tests/main.vn`, que es una **suite de
correctitud**: compila 31 funciones para correr cada una ~3 veces, y el 77% de
su tiempo es compilar. Un programa real compila una vez y corre largo, así que
bajar ese coste no se nota fuera de la suite. Los huecos reales frente a Bun
(JSC) estaban en la **llamada**, en la **inferencia de tipos que no llegaba a
los globals**, y en **alloc/GC** — ninguno tocado por las Tareas 1-4.

### Lo que se midió

Coste por operación, dentro de funciones clif, antes de los arreglos:

| operación | ns | veredicto |
|---|---|---|
| iter de loop int (`sum + k*3`) | 1.76 | óptimo |
| iter de loop float | 1.27 | óptimo |
| `{a: i}` que no escapa | 1.48 | scalar replacement funciona |
| lectura `int[]` | +2.0 c/u | bounds + guard de repr |
| `arr.push(i)` | 15.7 | |
| **llamada a otra función** | **43-53** | vs 1.5 inlineado |
| `new P1(i)` | 83.3 | = llamada + alloc |
| `new P5(...)` que escapa | 161-167 | +70.6 de promoción GC |

Aritmética escalar y escape analysis ya estaban a nivel V8. El agujero era la
llamada.

### Lo que aterrizó

1. **Llamada directa clif→clif, ligada tarde** — `43.4 → 2.76 ns` (15.7x).
   El fast path ya existía pero era **código muerto**: `calls_allowed` exigía
   `!has_alloc`, y `OpCode::Call` está EN la lista de `has_alloc`, así que
   nunca era cierto en una función que contuviera una llamada. Además
   `static_target` exigía el callee ya compilado, y los callers compilan antes
   que sus callees por definición. Ahora el sitio de llamada carga la entrada
   desde `FunctionProto::clif_raw` en tiempo de ejecución (0 = todavía no), y
   el ABI acepta parámetros/retornos de cualquier `SlotKind`, no sólo `Int`.
   `tests/main.vn` **correr** 10.44 → 4.89 ms.

2. **BUG DE CORRECTITUD: llamadas con ≥4 parámetros corrompían argumentos.**
   `emit_generic_call` pasaba 4 `VmValue` planos a `clif_call_fallback` pero
   declaraba el `argc` real por separado, y ese conteo incluye el slot r0. Con
   4 params reales se stageaban 4 valores para un `arg_count` de 5 y
   `prepare_call` leía el frame un slot abajo. `f4(1,2,3,4)` daba **123** en
   vez de 1234; `f6(...)` daba **230123** en vez de 123456. `VARN_NO_JIT=1`
   era correcto, así que sólo un caller compilado lo exponía; la suite no lo
   cubría. Los argumentos ahora viajan por la ventana de registros flusheada,
   sin techo de aridad. Fijado por `tests/57-call-arity.vn`.

3. **Inferencia de tipo de elemento para arrays a nivel de módulo.** Existía
   (`binder::array_evolve`) pero excluía el scope Global por diseño. Lo que un
   scan de un solo archivo no puede justificar es un binding que **sale** del
   archivo, y eso es exactamente lo que `bind_export` escapa ahora (forma
   declaración, forma especificador y `export default <expr>`). `bench_matrix`
   sin anotar: **58.9 → 38.4 ms**.

4. **Cache de payload de array en funciones que alocan.** Estaba desactivada
   porque cualquier alocación puede mover el Vec del heap bajo el puntero
   cacheado. Ahora se habilita por región de loop cuyo cuerpo no aloca, y el
   safepoint de back-edge invalida las caches en su rama tomada. 1.17x en el
   micro que tiene esa forma exacta.

5. **mimalloc como allocator global** (`crates/varn-cli`). Varn es
   alloc-bound justo en las formas que importan — construcción de objetos,
   concatenación de strings, crecimiento de arrays — y todas terminan en el
   allocator del sistema; en Windows MSVC ese es el heap del proceso, que
   cobra mucho más por alocación pequeña que un allocator con caches por
   hilo. Es el único cambio que llega a todos esos sitios a la vez.

   | operación | antes | después | |
   |---|---|---|---|
   | `"a" + "b"` | 100.3 ns | 64.2 | 1.56x |
   | `"prefix" + int` | 117.3 | 68.3 | 1.72x |
   | `int.toString()` | 71.6 | 53.7 | 1.33x |
   | `new P5(...)` que muere | 78.6 | 59.3 | 1.33x |
   | `new P5(...)` que escapa | 166.6 | 119.1 | 1.40x |

6. **Un solo lookup por objeto promovido en el minor GC** — limpieza
   estructural, **sin** cambio de rendimiento medible (0.98x / 1.07x). Los
   70 ns por objeto promovido están en el movimiento del `HeapObj` y en el
   push al Vec de old gen, no en los lookups. Anotado para no volver a
   intentarlo por ahí.

### Resultado acumulado

Pareado contra el árbol pre-sesión, método robusto a térmica (los dos
binarios adyacentes en cada ronda, mediana de las razones):

| bench | |
|---|---|
| bench_json | **2.04x** |
| bench_gc_alloc | **1.69x** |
| bench_matrix | **1.52x** |
| bench_dto | **1.44x** |
| bench_str_ops | **1.35x** |
| bench_array_ops | 1.08x |
| fib · class_fields · math · multiple | plano |
| llamada cruzada (micro) | **15.7x** (43.4 → 2.76 ns) |
| `new X(...)` (micro) | **2.05x** (83.3 → 40.7 ns) |

`tests/main.vn` 805/805 con `run` y con `bench`, en clif, `VARN_NO_JIT=1` y
`VARN_NO_CLIF=1`; `cargo test --workspace --release` 48 suites verdes.

### Lo que sigue abierto, por ROI medido

`bench_dto` sigue ~20x por detrás de Bun (49.5 ms contra 2.43) y `bench_matrix`
4x (36 contra 9.4). Reparto medido de dto con los costes de hoy: ~5.8 ms en
constructores, ~6.8 en concatenación, ~7.2 en promoción, ~2.1 en `push`, ~3 en
lecturas de propiedad. El resto es el bucle del módulo.

> **Las optimizaciones puntuales en el camino de alocación se agotaron.**
> Después de mimalloc, los cinco intentos siguientes dieron ruido (ver más
> abajo y en la Tarea de GC). Todo número grande que queda — `push` 15.5 ns,
> `new` 40.7, concat 64, promoción ~60 — es *tocar un objeto del heap*, y todos
> comparten la misma causa: un objeto es un `Rc<ObjData>` con campos
> NaN-boxeados, alojado en un slot de `Vec<Option<HeapObj>>` de 48 bytes que
> luego hay que copiar al promover. El siguiente paso real es cambiar eso
> (objetos inline en una región bump, campos crudos), que colapsa alocación y
> promoción a la vez. Es un rewrite del heap, el GC y todos los consumidores de
> `HeapObj` — no un ajuste más.

Medido para acotar el objetivo: la aritmética escalar y el escape analysis ya
están a nivel V8 (1.5-1.8 ns/iter int, 1.3 float, 1.5 para un objeto que no
escapa), y la llamada ya está en 2.8 ns. No queda nada que ganar ahí.

1. **Constructores: 40.7 ns** (eran 83.3; mimalloc se llevó la mitad). El
   callee de `new X(...)` es un `Class`, no un `VmClosure`, así que nunca toca
   el linker: va por `clif_call_fallback` → `call_vm_window` →
   `construct_staged_fast`, que empuja un `CallFrame` de verdad (143 333 frame
   pushes en `bench_dto`) y paga cuatro clones de `Rc` por instancia. El
   arreglo real es **inlinear** el constructor en el sitio de llamada cuando su
   cuerpo son sólo `SetFixedField` desde parámetros: alocar con la shape
   conocida y emitir los stores, sin llamada. Objetivo ~25 ns. Los apaños
   parciales (quitar clones, cachear el ctor resuelto sin `dyn Any`) suman ~10
   ns de los 40 y no valen el riesgo por separado.
2. **Promoción del GC: ~50-70 ns por objeto promovido.** Medido aislando
   `freshEscape` contra `freshShortLived` a igual tasa de alocación. Tres
   caminos ya descartados con medición:
   - **Subir la nursery NO sirve**: de 16384 a 131072 bajó los minor GC de 20
     a 3 y empeoró `bench_dto` un 18.6% y `bench_gc_alloc` un 11.8`%`
     (locality). El coste es por objeto evacuado, no por colección.
   - **Reducir los lookups del scan tampoco**: colapsar cuatro `get_raw` por
     objeto en uno dio 0.98x / 1.07x, ruido.
   - **Quitar el segundo push de Vec por alocación tampoco**: `try_alloc`
     empujaba a `objects` y a `forwarding`, y `forwarding` no se lee hasta
     que corre un GC. Moverlo a un `resize` en `collect` dio 1.04x / 0.98x /
     0.96x / 1.00x. Revertido: churn en el camino caliente del GC por ruido.

   Lo que queda es el movimiento del `HeapObj` (48 bytes) y el push al Vec de
   old gen. Eso pide el trabajo de representación desboxeada, no un ajuste.
3. **La Tarea 4 (subir el tope de 250 words) es AHORA el lever más grande.**
   Corrijo lo que decía esta sección antes: medí dos veces que envolver
   `bench_dto` en una función no ganaba nada y concluí que era alloc-bound.
   Esa medición era correcta **en su momento y ya no lo es**: entonces la
   llamada de 53 ns por constructor dominaba igual desde un caller
   interpretado que desde uno compilado, y tapaba el despacho del intérprete.
   Con el entorno 3x más barato (llamada directa + mimalloc) queda expuesto:

   | `bench_dto` | |
   |---|---|
   | top-level, `<module>` interpretado (gate a 252 words) | 50.95 ms |
   | envuelto en función, 100% clif | **33.42 ms** |

   Con el tope en 8192 y las dos correcciones de abajo: suite **805/805** en
   los tres modos, cobertura clif **1282/1282 frames**, `CLIF GATE` **0**, y
   `bench_dto` pareado **1.53x** (67.2 → 42.5 ms). El criterio de aceptación
   de la Tarea 4, cumplido.

### Tarea 4: CERRADA (`0e1be7d`)

`SIZE_GATE_WORDS` = **8192**. Los `<module>` top-level y las funciones largas
rutean por clif por primera vez.

| | |
|---|---|
| `tests/main.vn` | **836/836** con `run` **y** con `bench`, en clif, `VARN_NO_JIT=1` y `VARN_NO_CLIF=1` |
| cobertura clif | **1287 de 1287 frames (100%)** |
| `CLIF GATE` | **0** |
| `CLIF BAIL` | **0** |
| `cargo test --workspace --release` | 48 suites verdes |
| `bench_dto` pareado | **1.53x** (52.0 → 33.5 ms) |
| `bench_matrix` / `bench_json` | 1.06x / 1.00x |

Aterrizado en `8583e3b`:

1. **Buffer de salto por frame.** Era el diseño de la Tarea 4 §1.
   `execute_jit_frame` instalaba `setjmp` sólo para el frame clif más externo,
   así que un `throw` varias capas adentro saltaba por encima de los
   intermedios y el bucle de frames los reentraba desde `ip == 0` para
   siempre. Ahora cada frame instala el suyo y restaura el del padre al salir;
   el desenrollado es de a un salto, y `clif_call_fallback` re-lanza contra el
   buffer del padre ya restaurado. **`11-errors.vn` pasa con el tope en 8192.**
   La suspensión sigue necesitando el buffer MÁS EXTERNO (Tarea 4 §2): vive en
   `jit_suspend_buf`, fijado por el frame externo — comportamiento idéntico al
   de antes, sin anidar.
2. **`InvokeRuntimeStatic` volcaba mal sus operandos.** `jit_range` recibe
   NÚMEROS DE REGISTRO y lee de `ctx.stack[base + reg]`, pero el lowering
   nunca los ponía ahí: `0..5` construía `RangeData { start: 0, end: 0 }`.
   Vivo en funciones normales, no sólo en top-level. Fijado por
   `tests/58-clif-range.vn`. Barrido hecho sobre todos los demás helpers que
   leen home slots — era el único hueco.

Aterrizado en `0e1be7d`, los tres bugs que faltaban:

3. **Todo retorno `int` NEGATIVO daba null.** No sólo con el tope alto: vivo
   en una función normal del binario publicado.

   ```varn
   function sub(a: int, b: int): int { return a - b }
   sub(1, 4)   // clif: null      intérprete: -3
   ```

   Un raw que devuelve `int` produce SIEMPRE un payload i48 desboxeado — las
   tres ramas del caso `SlotKind::Int` de `emit_return_value` lo hacen. El
   wrapper re-etiquetaba sólo si los bits altos estaban limpios, asumiendo que
   un tag NaN-box puesto significaba "ya boxeado". Ese test no distingue un
   valor boxeado de un payload **negativo**: `-3` es `0xFFFF_FFFF_FFFF_FFFD`.
   Re-etiquetado ahora incondicional. La suite no lo veía porque sus funciones
   que devuelven `int` daban siempre no-negativos.
4. **`Negate` sobre operando boxed.** Elegía su camino inline por el meta del
   registro DESTINO solamente, así que un operando boxed caía en `use_int` —que
   acepta boxed— e `ineg` corría sobre los bits de payload de un VmValue
   heap-tagged: `-(7.5d)` daba null. Ese caso va ahora al helper (negar un
   decimal ALOCA, que es justo por lo que `Negate` está en `has_alloc`).
5. **`Negate` inline escribía un payload crudo** en un registro que el flujo de
   kinds tipa `Boxed`. Ahora envuelve como el `make_int` del intérprete y boxea.

   > **El camino float no se toca, a propósito.** `use_f64` baila ante lo que
   > no puede probar, y ese bail es load-bearing: es lo que manda
   > `(-a) + (-f)` (operando int hacia destino float-typed) al intérprete. Un
   > primer intento se tragó ese caso en un arm de helper y convirtió -4.5 en
   > -1.5.

**Esperar más bugs de la misma familia.** Nada por encima de 250 words se había
compilado nunca; cada función que ahora rutea es superficie que la suite jamás
pudo ejercitar. Los cuatro que salieron en esta tanda
(`InvokeRuntimeStatic`, retorno negativo, y los dos de `Negate`) estaban todos
vivos en funciones normales, no sólo en top-level.

### Método de medición (corrección)

Tomar el min de N por binario está **sesgado**: la máquina calienta de forma
monótona, así que el binario que ocupe el primer slot del barrido se lleva el
min. Un mismo binario midió `bench_fib` a 36.4 ms en la ronda 1 y a 74.0 ms en
la ronda 3. Medir los dos binarios **adyacentes** en cada ronda, tomar la razón
por ronda y reportar la mediana; alternar el orden entre rondas.

---

## 0. Dónde estamos

### Migración de opcodes: terminada

Los 134 variantes de `OpCode` tienen arm de lowering en `crates/varn-jit/src/clif/`.
El template JIT **ya no existe**: `varn_jit::compile` lowerea con Cranelift o
devuelve `Err`, y el bail deja la función al intérprete. No hay tier intermedio.

Cobertura real sobre `tests/main.vn` (`VARN_CLIF_TRACE=1`):

| | |
|---|---|
| `CLIF ROUTE` | 450 |
| `CLIF BAIL` (rechazo del lowering) | **0** |
| `CLIF GATE` (`code.len() > 250`) | **47** |

Ningún opcode rechaza. Lo que queda fuera son **funciones enteras** que la
puerta de tamaño descarta antes de llamar a clif — todos los `<module>`
top-level entre ellas. La traza `CLIF GATE` existe precisamente porque ese
rechazo no aparecía ni en `CLIF BAIL` ni en `compile_fail`, y sin ella el
"0 bails" sobreestima la cobertura.

### Rendimiento

`tests/main.vn` no es execution-bound, es **compile-bound**. Con
`VARN_NO_JIT=1` el árbol actual y el baseline `505c004` miden lo mismo
(15.2 vs 15.9 ms): todo el delta con JIT activo es tiempo de compilación.

Cranelift compila ~**65x** más lento que el template borrado — sobre las mismas
144 funciones de `tests/47-isolates-multithread.vn`: **2.48 ms** (template) vs
**91.5 ms** (clif), ~19 µs vs ~1.24 ms por función.

| `tests/main.vn` execute min | |
|---|---|
| baseline `505c004` (07-25) | 23.9 ms |
| antes del tiering | 87.3 ms |
| **hoy** | **38.3 ms** (suite completa: 42.1 ms min / 45.6 ms p50) |

> Actualizado tras la sección 0.0: el reparto es `compilar 34.3 ms (77%) +
> correr 10.4 ms (23%)`, y la parte de **correr** bajó a **4.89 ms** con la
> llamada directa. El coste que persiguen las Tareas 2-3 es el 77% de una
> suite de tests, y lo empequeñece la `precompilación de módulos`: **141.7 ms**
> de arranque en frío, fuera del p50.

### Estado del árbol

**Sin commitear**: ~27 archivos, +1177/−547. Incluye la migración de 11 opcodes
a clif de la sesión anterior, cuatro reparaciones de correctitud, el tiering
perezoso, y la limpieza de scaffolding de debug en `tests/28|42|53`.

`HEAD` (`d67e92b`) **no puede correr la suite** — panica en el test 7
(`ctx_jit_values.rs`, index 512 en len 1). El último commit sano para
comparaciones es **`505c004`** (07-25), que todavía tenía el template JIT.

Validación actual: 775/775 con clif, con `VARN_NO_JIT=1` y con
`VARN_NO_CLIF=1`; `cargo test --workspace --release` 48 suites sin fallos.

**Primer paso de cualquier sesión nueva: commitear esto.** Es mucho trabajo
verde sin proteger, y `HEAD` está roto.

---

## Tarea 1 — Bug de paridad de tiers (CERRADA 2026-07-31)

> **CERRADA.** La segunda mitad no era un bug de closures/upvalues ni de
> tiering: era código compilado contra un heap muerto. Ver §0.0-bis. El repro
> (`33` + `37`, `vn bench`, umbral > 1) pasa, y la suite pasa con umbral 1, 2,
> 4, 8 y 16 en los tres modos. Lo que sigue se conserva porque documenta la
> primera mitad (el sink `int` de la stdlib, `a09e24a`) y porque la
> descripción equivocada de la segunda es justo lo que costó la sesión.

> **Estado 2026-07-30: mitad resuelta.** La causa del fallo de `Duration hours`
> era que **la stdlib nunca se type-checkeaba**: `stdlib_loader::compile_source`
> corría el checker y tiraba sus diagnósticos, así que `std/time.vn` podía
> declarar `int_div(...): int` devolviendo un float entero. Ahora el bundle se
> valida al construirse (`compile_source_checked`), existe `float.toInt(): int`
> (única conversión real: `as int` no baja a nada), y los 7 errores de tipos que
> el gate destapó están arreglados — junto con tres bugs del checker que
> afectaban también a código de usuario. Commit `a09e24a`.
>
> **Queda un SEGUNDO bug de paridad, distinto, en closures/upvalues**, que sigue
> bloqueando cualquier umbral > 1:
>
> ```text
> tests/scratch.vn:
>     import "./33-globals-async-coherence.vn"
>     import "./37-complex-closures.vn"
> vn bench tests/scratch.vn --runs 1   →  ASSERT FAIL: sm after start
> ```
>
> `vn run` pasa. `vn bench` falla porque ejecuta el programa dos veces (warmup +
> medidas): sólo entonces `makeStateMachine` llega a su segunda entrada de frame
> y se compila, y el `current` capturado vuelve a leerse con su valor previo al
> `transition`. Reproduce con umbral 2 y con 4; necesita el módulo async 33
> delante del 37; con 37 solo no reproduce. Es vida útil de stack/upvalue, no
> aritmética. Empezar por comparar `emit_make_closure`/`emit_close_upvalue`
> (`clif/alloc.rs`) contra `capture_upvalue`/`close_upvalues_above`
> (`exec/ctx_frames.rs`) y por qué la suspensión del módulo 33 cambia el layout.
>
> **`vn bench` es gate obligatorio** (CLAUDE.md), no sólo `vn run`: este fallo no
> aparece en `run`.

Lo que sigue es el análisis original, ya resuelto, que se conserva por contexto.

### Repro exacto

En `crates/varn-vm/src/frame.rs:306`:

```rust
const JIT_TIER_THRESHOLD: u32 = 1;   // subir a 2
```

Con `2`:

```
cargo run --release --bin vn -- run ./tests/main.vn
  → ASSERT FAIL: Duration hours
    at <module> (tests/31-stdlib-migration-test.vn)
```

`VARN_NO_JIT=1` pasa. Con umbral `1` pasa. O sea: falla sólo cuando **unas
llamadas se interpretan y otras corren compiladas**. La compilación ansiosa
lo ocultaba porque todo corría JIT desde la primera llamada.

### Hipótesis principal (verificar antes de tocar nada)

`tests/31-stdlib-migration-test.vn:10` es `assert("Duration hours", d.hours === 1)`.
La línea 9, `d.totalMilliseconds`, **pasa**. Mirando `std/time.vn:187-195`:

```varn
this.totalMilliseconds = ms;                    // pasa
this.hours = int_div(total_s, 3600);            // falla
```

El campo que falla es exactamente el que viene de `int_div`. `int_div` devuelve
un **float entero** en un sink declarado `int`; el contrato es que la VM coerce
en el sink, no en la definición — está documentado en
`crates/varn-jit/src/clif/kinds.rs` (arm `OpCode::Call`, ~línea 109):

> A call result is ALWAYS boxed bits, even into an `int`-typed slot […] stdlib
> code relies on the VM coercing a whole float (`int_div`) at the int sink.

Sospecha: el store del campo (`SetFixedField` / `SetProperty`) coerce distinto
en el intérprete que en clif, así que una instancia `Duration` construida por el
constructor interpretado y otra construida por el constructor compilado guardan
representaciones distintas para `hours`, y `=== 1` falla en una de las dos.

Ver [[varn-int-semantics-i48]]: las reglas viven SÓLO en `varn-core/numeric.rs`
y deben ser tier-idénticas.

### Cómo atacarlo

1. Reducir a un `.vn` mínimo: una clase con `this.x = int_div(a, b)`, construirla
   dos veces, comparar `x` entre la instancia 1 (interpretada) y la 2
   (compilada). Con umbral 2 eso es exactamente lo que ocurre.
2. Imprimir los bits crudos (`heap.str_repr` no basta — hace falta el `VmValue.0`)
   del campo en ambas instancias.
3. Comparar el camino de store: intérprete en `crates/varn-vm/src/exec/props.rs`
   (`set_fixed_field`) contra el emitido por `crates/varn-jit/src/clif/fields.rs`.
4. Arreglar en el punto único de verdad; no parchear un tier.

### Criterio de aceptación

- Umbral 2: `tests/main.vn` 775/775 con clif, con `VARN_NO_JIT=1` y con
  `VARN_NO_CLIF=1`.
- Un test nuevo que fije la paridad: misma clase construida antes y después del
  umbral, campos idénticos.

---

## Tarea 2 — Subir el umbral de tiering y medir

Sólo después de la Tarea 1. **Sigue bloqueada**: el umbral está en `1` porque
2 y 4 rompen `vn bench` (ver el recuadro de la Tarea 1). Medir cualquier umbral
antes de arreglar ese bug es medir un árbol incorrecto.

`crates/varn-vm/src/frame.rs:306`. Hoy vale `1`, que es el tiering más débil
posible: salta únicamente los protos que se **construyen y nunca se entran**
(en el test de isolates, 144 funciones compiladas para ejecutar 38 frames JIT).
Todo lo que se ejecuta sigue compilándose antes de su primera llamada.

El ahorro real es dejar que las primeras N llamadas interpreten. Barrer
`2, 4, 8, 16` y quedarse con el mejor, midiendo:

- `tests/main.vn` (execute min)
- `tests/47-isolates-multithread.vn` — el más sensible, 60.8 → 27.9 ms con el
  tiering actual, baseline 10.9 ms
- los benchmarks de `benchmarks/` (fib, matrix, …) — un umbral alto castiga
  funciones calientes de arranque corto; vigilar que no regresen

Contexto de la implementación actual:

- `FunctionProto.jit_entry_count` (`crates/varn-types/src/chunk.rs:551`) cuenta
  entradas de frame mientras el proto siga sin compilar.
- `VmClosure::hot_jit_fn` (`crates/varn-vm/src/frame.rs:320`) cuenta y compila.
- Punto de tiering: `crates/varn-vm/src/exec/dispatch/mod.rs`, entrada de frame
  en `run_until_inner_raw`.
- La entrada compilada vive **sólo** en `FunctionProto.jit_entry`. `VmClosure` ya
  no tiene campos `jit_entry`/`jit_code` (muchas closures comparten proto y el
  tiering ocurre después de construirlas). Leer con `closure.jit_fn()`.

---

## Tarea 3 — Coste de compilación de Cranelift

> **§1 y §2 hechos (2026-07-31), §3 es ahora la palanca principal.** Ver
> §0.0-bis: verifier apagado (1.58x), `Context` reusado de verdad, y el reparto
> medido es 91% Cranelift / 9% lowering.

1. **`opt_level`**: `"speed"` sigue siendo el default; `VARN_CLIF_OPT` lo
   hace barrible. Medido: `none` da un 15% más de compilación sobre el verifier
   ya apagado (32.3 → 27.9 ms) a costa de calidad de código. Eso pertenece a un
   primer tier con un segundo escalón `speed`, no al único tier que hay.
2. **Reutilizar `Context`**: hecho, en `clif::with_ctx`. El thread-local ya
   existía pero el camino real no lo usaba.
3. **Compartir código entre isolates y entre corridas**: bloqueado por lo mismo
   que causaba el miscompile — el código hornea handles del heap. Quitar ese
   horneado (ver §0.0-bis, «la palanca que queda») es lo que lo habilita.

Medir siempre con el método de la sección "Cómo medir".

---

## Tarea 4 — Tope de 250 words: frames clif reanudables

> **CERRADA en `0e1be7d`.** El tope es 8192, cobertura clif 100%, `CLIF GATE`
> 0, suite 836/836 en los tres modos, `bench_dto` 1.53x. Lo que sigue es el
> análisis original, conservado porque el diseño que describe es exactamente el
> que se implementó (§1 = buffer por frame, §2 = `jit_suspend_buf`), y porque
> la advertencia sobre la suspensión sigue vigente: sigue desenrollando al
> frame MÁS EXTERNO, no anida.

`crates/varn-jit/src/lib.rs:357`. **No es un presupuesto de compilación** — es
lo único que mantiene los `<module>` top-level fuera de clif, y con ellos las
únicas formas que ponen un frame clif debajo de otro.

Por qué no se puede subir el número y ya:

- Un frame clif no es reanudable: no tiene `ip` de bytecode propio, así que la
  VM sólo sabe reentrarlo desde `ip == 0`.
- `execute_jit_frame` (`crates/varn-vm/src/exec/dispatch/mod.rs:54`) instala
  `setjmp` **sólo para el frame clif más externo** (`is_outer`, línea 60).
- Un `throw` en una llamada clif anidada hace longjmp hasta ese buffer externo y
  destruye la pila nativa de todos los frames clif intermedios, cuyos
  `CallFrame` siguen vivos con `ip == 0` → el frame loop los reentra por JIT
  desde arriba → reejecución infinita.

Comprobado: con el tope en 8192,
`assert("try catch returns", safeDivide(10, 0) === -1)` de
`tests/11-errors.vn` cuelga y muere en
`memory allocation of 51539607552 bytes failed`. Prefijo mínimo que falla:
`head -11 tests/11-errors.vn`.

La suspensión (`Await`, `Yield`) tiene el mismo hueco sin el bucle.

### Diseño requerido

1. **Buffer de salto por frame.** `execute_jit_frame` guarda el
   `ctx.jit_jmp_buf` anterior, instala el suyo, y lo restaura al salir. En la
   rama de error (`code == 1`, línea 132), si el handler NO pertenece a este
   frame (`handler.frame_depth - 1 < frame_idx`), re-lanzar con
   `my_longjmp(saved, 1)` para que el padre desenrolle su propio frame nativo.
   Cuando `saved` es null estamos en el más externo: devolver `Err` como hoy.
2. **La suspensión necesita el buffer MÁS EXTERNO**, no el innermost: un frame
   clif intermedio no puede parkearse a mitad de función. Hace falta un
   `ctx.jit_suspend_buf` separado que `jit_await` / `jit_yield` usen, o un ip de
   side-exit por punto de suspensión.
3. Precedente ya resuelto para un opcode concreto: `jit_load_module`
   (`crates/varn-vm/src/exec/ctx_jit_runtime.rs`) rebobina el ip a la propia
   instrucción `LoadModule` y parkea **su** frame — capturado ANTES de la carga,
   porque el módulo suspendido deja su frame encima. Ahí funciona porque el
   destino de reanudación es conocido. Ver también el `if let Some(resume_ip)`
   tolerante en el handler `code == 2` (`dispatch/mod.rs:167`).

### Criterio de aceptación

- Tope subido a ≥8192, `tests/main.vn` 775/775 en los tres modos.
- `CLIF GATE` a 0 sobre la suite.
- Las 47 funciones gated hoy incluyen `runComprehensiveTest` (2004 words),
  `testIsolates` (720), `runAsync` (652), `parse` (462), `dirname` (434),
  `normalize` (405).

---

## Cómo medir (obligatorio)

La máquina termaliza duro: en esta sesión el **mismo binario** pasó de 23.9 a
35 ms. Un A/B suelto miente.

- Dos binarios, órdenes alternados, **min de 6 rondas**, nada más corriendo.
  Un build en paralelo distorsiona 6x (medí 628 ms donde eran 92).
- Baseline de comparación: worktree en `505c004`
  (`git worktree add <tmp> 505c004 --detach`), `cargo build --release --bin vn`.
  Ese árbol necesita `--release`: con `--profile quick` no linkea
  (`.varn_ops$A` undefined).
- La suite de `505c004` corre **713** asserts y la de hoy **775**; para comparar
  trabajo igual, excluir `tests/42` de ambos `main.vn`.
- Usar `execute` **min**, no p50 ni e2e.
- `bench -v` da lo que de verdad importa aquí: `freshly compiled`,
  `total compile time`, `JIT runs`, `calls slow/prepare`, `frame pushes`.
  Fue esa tabla — no el timing — la que localizó la regresión.

---

## Trampas conocidas (cuestan horas si se redescubren)

- **`FunctionProto.arity = 1 + nparams`**: incluye el slot r0 del callee, que en
  un método es el receptor. `clif::lower` debe usar
  `proto.arity.saturating_sub(1)` (tres sitios). Tratarlo como el número de
  params hace que `param_kinds.len() != nparams` sea siempre cierto → **bail en
  todas las funciones, backend clif muerto sin un solo error visible**.
- El caller stagea r0 como placeholder `null`; un `BoundMethod` **rellena** ese
  slot, no inserta delante. `expected = arity + has_this` corre los params un
  registro (`Box.map(f)` → "value is not callable: null").
- El fast path Vm de `BoundMethod` en `try_prepare_call_fast` rompe `super`
  (`tests/48-opt-unsupported-phase1.vn`, "super write via method"). Sólo el
  target `Native` va por fast path.
- `.vnc` cachea por hash de fuente: purgar antes de validar cambios del
  compilador.
- No usar `--profile quick` para nada que toque `varn-builtins`: no linkea.

---

## Orden sugerido

Tareas 1, 2 y 4 cerradas; Tarea 3 §1-§2 hechas. Lo que queda, por ROI:

1. **Código independiente del heap** (§0.0-bis): `LoadConst` desde la tabla de
   la closure en vez de inmediato, y el puntero de closure disponible también en
   funciones no frame-aware. Habilita reuso entre corridas e isolates, que es
   todo el coste de compilación por heap.
2. **Contar back-edges + OSR**: sin eso, ningún umbral de tiering sirve (Tarea 2
   lo midió: `bench_matrix` 2.4x peor con umbral 2).
3. **Representación desboxeada** (objetos inline en región bump, campos crudos):
   lo único que queda para `bench_dto`/`bench_matrix` según §0.0.
