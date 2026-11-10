# ADR-0027 — Auditoría y rediseño del recolector de basura

Estado: PROPUESTO (2026-11-09). Sustituye las secciones de heap de ADR-0020, que describe una tabla de handles con un GC que mueve objetos y ya no existe en el código. Las secciones 1 a 9 son la auditoría y la corrección de cimientos; las 10 a 14 fijan el objetivo de grado industrial, la línea base frente a V8, CoreCLR y Go, y los diferenciadores propios de Varn.

Base de la auditoría: commit `7dc95881`, binario `target\release\vn.exe` compilado por `verify.ps1 -Fast`, Windows 11 x86-64, una sola máquina.

## 1. Alcance y método

Por la Ley 12, ninguna afirmación de este documento se apoya en otro `.md`. Cada hallazgo cita el código que se leyó (`archivo:línea`) y la medición que se ejecutó. Lo que no se pudo comprobar está marcado como tal.

Se leyó el GC completo: `heap/{gc,minor,major,young,children,structs,core,jit,strings,intern,obj}.rs`, `heap/cells/{mod,blocks,typed,native}.rs`, `varn-types/src/cell.rs`, `vm_value/{array,vm_array,vm_array_access}.rs`, `exec/ctx_gc.rs`, `closure.rs`, `dispatch/modules.rs`, el contrato con el JIT (`varn-jit/src/clif/alloc/safepoints.rs`, `from_ssa/alloc_instance.rs`) y `scripts/verify.ps1`.

Se midió con programas deterministas (apéndice A), con los contadores de `vn bench -v` y con `VARN_GC_TRACE`. Las pausas por fase del GC mayor se midieron con instrumentación temporal que no se commiteó.

Una lección de método que invalidó una primera tanda: el análisis de escape del JIT elimina `const t = new Node(i); sink += t.id`. Los primeros experimentos de churn no asignaban nada. Todos los temporales de este documento escapan a un anillo (`ring[i & 1023] = new Node(i)`).

Límites: una máquina, un compilador, cargas sintéticas más el corpus `tests/benchmarks/vn`. No se midió con heaps de varios GB ni con tareas concurrentes.

## 2. El GC tal como está

### 2.1 Estructura

```
HeapInner
├─ cells: CellSpace
│    ├─ classes[24]: SizeClass          16 B … 2048 B
│    │     ├─ blocks: Vec<bloque 256 KiB>   (alloc_zeroed, alineación 16)
│    │     └─ lane { free, bump, end }      lista libre LIFO + bump
│    ├─ large: Vec<(HeapRef, bytes)>        celdas > 2048 B, una a una
│    ├─ old_growth, survivors               contadores en celdas
│    └─ native_roots                        raíces mientras corre un nativo
├─ young: { born: Vec<HeapRef>, remembered, threshold = 786432 }
├─ scan_roots: Vec<HeapRef>                 closures, métodos ligados, clases, módulos, generadores
├─ identity_index, 4 internadores           compactados solo en el mayor
└─ gc_threshold = max(2 × supervivientes, 786432)   (en celdas)
```

Cada celda tiene una cabecera de 8 bytes (`state`, `kind`, `class`, `_aux`) y un cuerpo. Las referencias son direcciones crudas (`HeapRef`). **El GC no mueve objetos.**

### 2.2 Ciclo

| Paso | Código | Qué hace |
|---|---|---|
| Asignar | `heap/core.rs:14-24` | celda `Young` y `born.push(r)` (el JIT lo hace en línea) |
| Disparo menor | `young.rs:33-35` | `born.len() >= 786432`; el JIT lo comprueba con un inmediato (`safepoints.rs:132-135`) |
| Menor | `minor.rs:8-52` | marca desde raíces + `remembered` + `scan_roots`; `sweep_young` promueve las marcadas y libera el resto |
| Disparo mayor | `gc.rs:37-40` | `old_growth >= gc_threshold` |
| Mayor | `major.rs`, `cells/mod.rs:235-252`, `gc.rs:52-67` | marca con el bit `0x80`, barre todas las celdas, compacta internadores, reconstruye `scan_roots` |
| Raíces | `ctx_gc.rs:46-91` | un `Vec` nuevo con pila, JIT, globales, módulos, constantes y metadatos, en **cada** GC |

### 2.3 Lo que está bien y se conserva

- **No mueve.** Los nativos guardan `VmValue` crudos en variables de Rust (`native_roots`, `cells/native.rs`), y el JIT tiene registros y huecos con referencias. Mover exigiría handles en todos los nativos.
- **Celdas por clase de tamaño con bump y lista libre**, con una cabecera de 8 bytes. La asignación en línea del JIT depende de este formato.
- **Arrays tipados** (`I64`, `F64`) que el GC no recorre, y el prefijo limpio de `Boxed` (`vm_array.rs:92-100`, `vm_array_access.rs:20,101,125`), con `tests/155`.
- **Barrera generacional con conjunto recordado** y `match` exhaustivo al recorrer hijos (`children.rs`, Ley 7).
- **Internadores que nacen `Old`** (`strings.rs:45-62`, `intern.rs`), así que una menor nunca deja entradas colgando.
- **La suite pasa con el GC bajo estrés**: `VARN_GC_STRESS=2048` y `VARN_GC_STRESS=256` → `PASSED: 5542, FAILED: 0` (ver H7).

## 3. Hallazgos

Cada fila enlaza código y medición. Los números de las mediciones salen del apéndice A.

| # | Gravedad | Hallazgo |
|---|---|---|
| H1 | Crítico | El disparo no ve la memoria que los objetos tienen fuera de la celda |
| H2 | Alto | El coste de una GC menor crece con el tamaño del heap viejo |
| H3 | Alto | Los objetos se promueven tras sobrevivir una sola vez |
| H4 | Alto | El mayor es stop-the-world, proporcional al heap vivo y con pasadas redundantes |
| H5 | Alto | El proceso nunca devuelve memoria al sistema |
| H6 | Medio | El ritmo del mayor se mide en celdas y con un piso fijo |
| H7 | Medio | Las barreras son una convención manual; el estrés del GC no está en ninguna puerta |
| H8 | Medio | El JIT está acoplado a las entrañas del heap, y los offsets de `born` no se validan |
| H9 | Info | Un literal `{…}` pesa mucho más que una instancia de clase |
| H10 | Info | Refutado: resolver la clase por objeto no domina el marcado |
| H11 | Info | ADR-0020 está obsoleto |

### H1 — Memoria externa invisible (crítico)

Un array, un mapa, un set y una string larga son una celda más un buffer de Rust. El GC solo cuenta celdas: `take_cell` suma 1 a `old_growth` (`cells/mod.rs:118-121`), `promote` también (`:165-168`) y el nursery se llena por `born.len()` (`young.rs:33-35`). `ArrayRepr` es un `Vec` (`array.rs:3-11`) y `HeapStr` guarda `Arc<str>` o `Rc<String>` (`str.rs:14-35`).

**E1.** 300 iteraciones de `Array.filled(500000, i)` (4 MB cada una, todo basura): **pico de 1192 MB**. Eran 300 celdas; el GC no tenía motivo para correr. Con 30 000 iteraciones el proceso se quedaría sin memoria.

### H2 — Una menor depende del heap viejo

Una GC menor debe costar según lo que sobrevive, no según lo que hay en el heap viejo. Hay tres causas.

**(a) `scan_roots`.** `minor.rs:28-32` reescanea en cada menor todos los closures, métodos ligados, clases, módulos y generadores que existan (`structs.rs:119-129`). Es así porque no tienen barrera: `VmUpvalue::write` hace `g.value = val` sin acceso al heap (`closure.rs:46-55`) y `op_store_module_slot` hace `m.set_slot(…)` sin barrera (`dispatch/modules.rs:107-109`).

**E4.** Mismos 1024 objetos promovidos y la misma basura joven en los tres casos:

| closures viejas | pausa menor |
|---|---|
| 0 | 1,8–2,0 ms |
| 100 000 | 3,0–4,6 ms |
| 400 000 | 7,3–7,8 ms |

Son ~14 ns por closure vieja en cada menor.

**(b) Mapas y sets recordados.** Un array recordado solo reescanea su sufijo sucio. Un `Map`/`Set` recordado se recorre entero (`children.rs:91-97`, sin distinguir `Reach::Minor`).

**E7.** Un `Map` viejo que recibe un valor joven cada ~130 000 iteraciones:

| entradas | pausa menor |
|---|---|
| 0 | 1,6–1,9 ms |
| 500 000 | 2,8–3,9 ms |
| 1 000 000 | 3,6–4,0 ms |

**(c) Raíces.** `gc_roots` recorre globales, módulos, clausuras estáticas, constantes de prototipos, metadatos y `hashable_keys` en cada menor (`ctx_gc.rs:70-89`). No se midió su coste por separado.

### H3 — Promoción inmediata

`sweep_young` promueve toda celda marcada (`minor.rs:67-72`). No hay envejecimiento.

**E5.** Una ventana deslizante de 500 000 nodos vivos (cada nodo vive 500 000 asignaciones, menos que el nursery de 786 432):

- **500 000 de 786 432 promovidos en cada menor (64 %).**
- 4,0 M de objetos promovidos en total.
- 4 GC mayores solo para limpiar basura que ya había muerto.
- GC total: 69 ms (menores) + 65 ms (mayores) = **134 ms**.

En `http_routing` el primer nursery promueve el 42 %.

### H4 — El mayor

- Es stop-the-world y recorre el heap vivo (`major.rs`).
- `sweep_major` itera todas las celdas con `refs()` (que incluye las libres) y arma un `Vec` con todas las muertas (`cells/mod.rs:235-252`).
- `collect` añade `compact_interners` y `rebuild_scan_roots` (`gc.rs:42-50`, `:7-17`), que vuelven a recorrer las celdas.

**E3.** Pausa mayor máxima con N objetos vivos:

| vivos | pausa mayor máx. |
|---|---|
| 1 M | 14,9 ms |
| 2 M | 21,7 ms |
| 4 M | 61,6 ms |

Son ~15 ns por objeto vivo, lineal y sin cota. Con instrumentación temporal en `http_routing`, 330 k supervivientes: marcado 4,6 ms (13,8 ns/obj) y barrido 3,8 ms (11,6 ns/obj). El barrido es casi la mitad de la pausa.

### H5 — La memoria no se devuelve

Los bloques solo se liberan en `Drop` (`cells/mod.rs:266-273`). El barrido devuelve celdas a listas libres LIFO por clase, sin orden por dirección (`blocks.rs:66-69`).

**E2.** Tras construir 3 M nodos (189 MB), soltarlos y hacer churn, el conjunto de trabajo queda en **232 MB** y no baja en los 2,5 s siguientes.

Lo que sí funciona (**E2c**): las celdas muertas del heap viejo se recogen y se reutilizan. Reconstruir otros 3 M nodos solo sube el pico de 232 a 245 MB.

### H6 — Ritmo del mayor

El umbral es `max(2 × supervivientes, piso)` en celdas (`gc.rs:65`). El piso era 65 536, menos que lo que promueve un nursery; el primer mayor marcaba 330 k vivos y liberaba 47. Se corrigió en `7dc95881` (piso = tamaño del nursery).

Queda abierto: con supervivencia del 100 % la primera menor promueve exactamente 786 432 y alcanza el piso. En **E2b** una construcción pura de 3 M objetos hace 2 mayores. No se midió cuánto liberan, pero el criterio sigue siendo un conteo de celdas con piso fijo, no una relación de crecimiento sobre bytes vivos.

### H7 — Seguridad de las barreras

- `write_barrier` se llama a mano desde unos 40 sitios (VM, helpers del JIT, `builtins`). Cada mutador nuevo debe acordarse de ella y de bajar `clean_prefix`. Una barrera olvidada es un objeto joven liberado mientras alguien lo referencia.
- No existe un verificador de invariantes del heap.
- `VARN_GC_STRESS` existe (`settings.rs:24-28`) pero **ningún script ni CI lo usa**: `verify.ps1` define cinco cuadrantes y ninguno lo activa; un `grep` de `GC_STRESS` solo encuentra su definición y su ayuda.
- Positivo medido: la suite completa pasa con el GC bajo estrés, con N=2048 (5542/0, 66 s) y con N=256 (5542/0, 62 s). El estrés está activo de verdad: el mismo programa pasa de 5 a 15 629 colecciones menores. Las barreras y las raíces actuales aguantan esa presión; lo que falta es que alguien lo compruebe en cada cambio.

### H8 — Acoplamiento con el JIT

El JIT asigna instancias y objetos en línea escribiendo en el `Vec` `born` y en los carriles de cada clase (`alloc_instance.rs:80-205`, `build_object.rs`), con offsets fijos (`cell.rs:41-52`). El umbral del nursery es un inmediato en el código generado (`safepoints.rs:132-135`) y entra en la clave de la caché del JIT (`code_cache.rs:45`).

Hay una sonda de layout en arranque (`heap/jit.rs:22-41`), pero comprueba los `Vec` de elementos de los arrays (puntero y longitud) y los discriminantes de `ArrayRepr`. Los offsets del `Vec` `born` son constantes `0/8/16` (`cell.rs:50-52`) y `born_len_byte_offset` supone el orden capacidad, puntero, longitud (`young.rs:42-44`). Rust no garantiza ese orden, y en el código que leí no hay una comprobación equivalente para `born`. Es un riesgo de corrupción silenciosa ante un cambio de versión del compilador.

Consecuencias de diseño: cualquier cambio de ritmo o de nursery toca este contrato, un umbral inmediato impide ajustar el ritmo en ejecución, y la etapa S5 (nursery por bloques sin `born`) elimina además esta dependencia.

### H9 — Un literal pesa más que una instancia

**E6.** 3 M objetos retenidos con un campo `int`:

| Representación | Pico | Pausa mayor máx. |
|---|---|---|
| Instancia de clase | 394 MiB | 60–74 ms |
| Literal `{ id: i }` | 616 MiB | 79–165 ms |

Cae fuera del alcance del GC (lo decide la representación del compilador, ADR-0026), pero amplifica H3 y H4.

### H10 — Refutado: la clase por objeto

Se sospechó que `class_child` (búsqueda en `identity_index` por objeto) dominaba el marcado. E6 lo contradice: las instancias de clase marcan más rápido que los literales. No hay acción.

### H11 — ADR-0020

ADR-0020 describe un `Option<HeapObj>` de 48 B por handle y un GC que "mueve objetos". El código actual tiene celdas con cabecera y direcciones crudas, sin movimiento. Este ADR lo sustituye en lo que toca al heap.

## 4. Principios del rediseño

1. **El GC mide bytes, no celdas.** Lo que pesa es la memoria, incluida la externa a la celda.
2. **Una menor cuesta lo que sobrevive.** Ninguna menor recorre el heap viejo.
3. **Se conserva "no mueve".** Es consecuencia de cómo se escriben los nativos y el JIT.
4. **Los cimientos primero (Ley 14).** Antes de cambiar el ritmo hay que poder comprobar que el heap sigue sano: verificador y cuadrante de estrés.
5. **Cada etapa borra lo que sustituye (Ley 8).** Sin rutas paralelas.
6. **Cada etapa trae una puerta numérica (Ley 10).** Si no se alcanza, la etapa no entra.

No se añade superficie al lenguaje (Leyes 17 y 20): todo es interno al runtime.

## 5. Comparación con otros lenguajes (Ley 21)

| Runtime | Modelo | Lección para Varn |
|---|---|---|
| C# / .NET | Tres generaciones, barrera de tarjetas, LOH aparte, GC de fondo para gen2, compactación opcional. `GC.AddMemoryPressure` para memoria no administrada | Un búfer de envejecimiento (gen1) evita promover lo efímero (H3). La memoria externa se declara al GC (H1) |
| Dart | Joven por copia con espacio de supervivientes, viejo con marcado concurrente y compactación a demanda; el tamaño externo cuenta para disparar | Mismo patrón: envejecer antes de promover y contar lo externo |
| Kotlin | JVM: G1/ZGC con regiones y marcado concurrente. Kotlin/Native: GC de marcado y barrido sin mover | **Un GC que no mueve es viable en producción** con interop nativo (apoya principio 3) |
| Go | Marcado-barrido sin mover, concurrente, con ritmo calculado sobre todo el heap y un *scavenger* que devuelve memoria | Ritmo relativo al heap vivo en bytes (H6) y devolución activa de memoria (H5) |
| Python | Cuenta de referencias + recolector cíclico generacional | La memoria externa se libera al instante, pero cada asignación paga y el JIT sufriría. **Descartado** |
| Swift | ARC, sin GC de trazado | Determinismo, a cambio de coste en cada escritura y fugas por ciclos. **Descartado** |
| Rust | Sin GC; propiedad | El runtime de Varn está en Rust: los búferes externos son `Vec` propios de las celdas, y su capacidad se puede contabilizar al crecer |

## 6. Diseño propuesto

### 6.1 Ritmo en bytes y contabilidad externa

- Cada contenedor con memoria externa (`VmArray`, `ValueMap`, `ValueSet`, `HeapStr`, `VmBuffer`, desborde de `ObjData`) anota el cambio de capacidad en un `ExternalLedger` al crecer y al soltarse. Es un contador por heap, y hoy hay un heap por hilo. Si al implementar aparece más de un heap por hilo, el ledger pasa a ser un campo del heap que los contenedores reciben por parámetro; el número de sitios es el mismo.
- El nursery se mide en **bytes**: bytes de las celdas jóvenes más el crecimiento externo desde la última menor. El valor inicial se calibra para que, en objetos pequeños, el comportamiento sea el de hoy.
- El mayor se dispara cuando los bytes del heap viejo superan `max(piso_en_bytes, bytes_vivos_tras_el_último_mayor × 2)`. El piso en bytes es al menos dos nurseries, para que una construcción con supervivencia del 100 % no dispare un mayor inútil (H6).
- **El JIT lee un límite variable** en lugar de un inmediato. Cuando el ledger rebasa el presupuesto, `request_gc()` baja el límite a 0 y el siguiente sondeo cae al camino lento. Coste: una carga adicional por sondeo.

No se mide la memoria en el asignador global: `mimalloc` está instalado solo en el binario `varn-cli` (`main.rs:2-3`), no en la biblioteca.

### 6.2 Envejecimiento

Dos edades jóvenes: `Young → Survivor → Old`.

- El estado de la celda usa un valor nuevo (`Survivor`) y las marcas pasan a bits propios (`0x40` menor, `0x80` mayor, ya existente), de modo que marcar no pierda la edad. Se elimina `SlotState::Marked`.
- Los supervivientes viven en una lista aparte (`survivors`) que **no** cuenta para el umbral del nursery (si contara, el nursery se llenaría antes de tiempo).
- Se promueve a `Old` al sobrevivir la segunda menor.
- La barrera pasa a preguntar `es_joven_o_superviviente(hijo)` (hoy `is_young`, `gc.rs:25-27`).

### 6.3 Raíces volátiles, estables y registro de aristas

- **Una menor parte de las raíces volátiles** (pila, huecos del JIT, raíces de nativos, tareas) **más un registro de aristas viejo → joven** (*sequential store buffer*).
- Los almacenes que hoy no tienen barrera pasan por ella: upvalues (`closure.rs:46-55`), huecos de módulo (`modules.rs:107-109`), estáticos de clase, estado de generadores, globales y constantes.
- Los mapas y sets anotan en el registro `(contenedor, hijo)` en `collection_write_barrier` (el gancho ya existe: `host/mod.rs:33`, `native.rs:333`) en lugar de quedar recordados enteros. El registro conserva las aristas cuyo hijo sigue siendo joven tras la menor.
- Un mayor sigue usando todas las raíces.
- Se borran `scan_roots`, `needs_minor_scan` y `rebuild_scan_roots`.

### 6.4 Heap viejo por bloques

- Los bloques pasan a estar alineados a su tamaño (256 KiB), de modo que `dirección & !(BLOCK-1)` da la cabecera del bloque, que guarda: clase, número de vivos y un **mapa de bits de marcas**.
- El marcado escribe en el mapa de bits, no en la cabecera de cada celda.
- El **barrido es por bloque y perezoso**: se barre un bloque al asignar desde él o con un presupuesto proporcional. La lista libre de cada bloque se reconstruye ordenada por dirección.
- Las celdas con propiedad externa llevan un indicador `needs_drop` en la cabecera; el barrido solo ejecuta destructores en esas.
- Tras un mayor, los bloques sin vivos se devuelven al asignador, conservando un colchón (máximo entre un nursery y el 10 % del heap). Que `mimalloc` los entregue al sistema operativo es asunto suyo; la puerta mide el conjunto de trabajo.
- Se borran el `Vec` de muertas, los recorridos de `refs()` en barrido y reconstrucción, y `give_back` LIFO.

### 6.5 Pausa del mayor: decisión, no suposición

Tras el barrido perezoso, el mayor queda en el marcado (~14 ns por vivo). Reducir más exige marcado incremental con una barrera en **todas** las escrituras de referencias, que se paga siempre. Un hilo de marcado concurrente queda descartado: `Rc` y `RefCell` no son `Send`.

No se implementa hasta que se fije un objetivo de pausa (decisión D1). Con 15 ns por vivo, un objetivo de 10 ms se supera a partir de ~700 000 objetos vivos.

### 6.6 Seguridad

- **Verificador** (`VARN_GC_VERIFY=1`, y siempre en el cuadrante de estrés): tras cada menor y mayor recorre el heap y comprueba que (i) ninguna referencia apunta a una celda libre, (ii) todo viejo con hijo joven está recordado o en el registro, (iii) no quedan marcas, (iv) las listas libres solo contienen celdas libres y los contadores cuadran.
- **Nuevo cuadrante de `verify.ps1`** con `VARN_GC_STRESS` y verificador, y el mismo en CI.
- **Barrera por tipos.** Los mutadores de contenedores devuelven un `WriteEffect` `#[must_use]` que el llamador entrega al heap. Olvidar la barrera es un error de compilación (Leyes 7 y 14).

### 6.7 Observabilidad

`VARN_GC_TRACE` solo traza menores (`gc_trace` se llama en `minor.rs`); los mayores hubo que instrumentarlos a mano para esta auditoría. Se traza cada colección con fases, bytes y edades, y `bench -v` suma histograma de pausas y bytes por generación.

## 7. Plan por etapas

Cada etapa es un commit atómico (Ley 9) que borra lo que sustituye (Ley 8) y solo entra si cumple su puerta (Ley 10). Las cifras base salen de la sección 3.

| Etapa | Cambio | Puerta (medible) | Se borra |
|---|---|---|---|
| **S0** | Verificador y cuadrante de estrés en `verify.ps1` y CI; sonda de layout para los offsets de `born` | Suite sin violaciones con N=256 y verificador; la sonda falla si se alteran los offsets | nada |
| **S1** | Ritmo en bytes, ledger externo, límite variable en el JIT | E1: pico 1192 MB → ≤ 100 MB. Corpus sin regresión > 2 % (A/B intercalado). Sin mayor inútil con supervivencia 100 % | umbral inmediato del JIT, `YOUNG_THRESHOLD`/`OLD_FLOOR` en celdas, `old_growth` |
| **S2** | Raíces volátiles, registro de aristas, barrera en upvalues/módulos/clases/generadores | E4 (400 k closures): 7,5 ms → ≤ 2,3 ms. E7 (1 M entradas): 3,8 ms → ≤ 2,0 ms | `scan_roots`, `needs_minor_scan`, `rebuild_scan_roots`, recorrido completo de mapas recordados |
| **S3** | Envejecimiento | E5: promovidos 4,0 M → ≤ 0,5 M; mayores 4 → ≤ 1; GC total 134 ms → ≤ 50 ms | `SlotState::Marked`, promoción en la primera supervivencia |
| **S4** | Heap viejo por bloques: bitmap, barrido perezoso, devolución de bloques | E3 (4 M vivos): pausa 61,6 ms → ≤ 40 ms. E2c: conjunto de trabajo tras soltar ≤ 50 % del anterior | `Vec` de muertas, `refs()` en barrido, `give_back` LIFO |
| **S5** *(condicional)* | Nursery por bloques sin lista `born` | Medir primero el coste de `born.push` y del barrido por celda; entra si la menor mejora ≥ 25 % | `born` y su sonda de layout |
| **S6** *(condicional)* | Mayor incremental | Solo con D1 definido | por decidir |

S0 va primero: es la red de seguridad de todo lo demás. S1 y S3 pueden ir en cualquier orden entre sí. S2 debe preceder a S3, porque el envejecimiento alarga la vida del estado joven y con él el coste de las aristas.

## 8. Alternativas descartadas

- **Generación joven por copia o compactación.** Los nativos guardan `VmValue` crudos y el JIT tiene registros con referencias; mover exige handles en todo el runtime.
- **Cuenta de referencias.** Coste en cada escritura, incompatible con el JIT actual y con fugas por ciclos (sección 5).
- **Escaneo conservador de la pila.** Perdería la precisión de la que dependen las raíces actuales y no resuelve H1 a H5.
- **Medir la memoria en el asignador global.** Solo existe en el binario (sección 6.1).
- **Marcado concurrente en otro hilo.** El heap usa `Rc`/`RefCell`.
- **Regiones al estilo G1.** Es el diseño más grande y no resuelve nada que S1 a S4 no resuelvan.

## 9. Decisiones abiertas

- **D1 — Objetivo de pausa del GC mayor.** Define si S6 se hace. Sin cifra no se hace.
- **D2 — Equilibrio memoria/velocidad del colchón de bloques** (10 % del heap por defecto).
- **D3 — Tamaño inicial del nursery en bytes.** Propuesta: calibrarlo con el nursery actual medido en bytes.

## 10. Objetivo: grado industrial

### 10.1 Qué significa "superior"

"Mejor que cualquiera diseñado antes" no se puede declarar: se demuestra o se pierde. Este ADR lo vuelve medible.

Referencias, en la misma máquina y con el mismo procedimiento (apéndice B): **V8** (Node 26.13), **CoreCLR** (.NET 10.0) y **Go** 1.27. Son tres diseños maduros y opuestos: generacional con copia, generacional con compactación y concurrente, y no generacional concurrente sin mover.

Un GC de Varn es **superior** si, en cada carga del conjunto fijo:

1. su tiempo y su pico de memoria quedan a **≤ 1,25×** del mejor de las tres referencias, y
2. gana a las tres en al menos una métrica de cola: pausa máxima o latencia p99 de un servidor bajo carga (carga W7, por construir), y
3. el verificador de invariantes (6.6) no registra ninguna violación en toda la suite bajo estrés.

Es una propuesta de criterio (decisión D4), no una garantía.

### 10.2 Línea base (commit `7dc95881`)

Mediana de 3 ejecuciones con `tests/benchmarks/gc/compare.ps1`. Tiempo en ms de la carga, pico del conjunto de trabajo en MB.

| Carga | Runtime | Tiempo | Pico | Pausa total | Pausa máx. | GCs |
|---|---|---|---|---|---|---|
| w1: 300 arrays de 4 MB, todo basura | Go | **90** | **22** | 8,3 | 1,0 | 150 |
| | .NET | 256 | 76 | 16,2 | n/d | 345 |
| | Node | 298 | 231 | 18,0 | 6,6 | 43 |
| | **Varn** | **518** | **1199** | 0,2 | 0,2 | 1 |
| w3: 4 M objetos vivos + 4 M de churn | Go | **139** | **132** | 0,6 | 0,6 | 17 |
| | .NET | 282 | 168 | 116,5 | n/d | 38 |
| | Node | 311 | 353 | 236,5 | 31,6 | 18 |
| | **Varn** | **287** | **360** | 221,0 | 74,6 | 13 |
| w5: ventana de 500 k nodos, 6 M de reemplazos | Go | **41** | **26** | 0,0 | 0,0 | 7 |
| | .NET | 187 | 49 | 144,7 | n/d | 25 |
| | Node | 184 | 159 | 129,7 | 11,2 | 18 |
| | **Varn** | **217** | **175** | 143,5 | 25,7 | 12 |

Ocho repeticiones de Varn en w5 dan 203–232 ms con ~138 ms de GC. Una medición previa de 404 ms se descartó por ruido de la máquina.

Validez: las pausas de Go y CoreCLR son solo las de parada total, porque marcan en otros núcleos. CoreCLR no expone la pausa máxima por colección sin un `EventListener`, así que no se midió. Los objetos pequeños sin punteros favorecen a Go (asignador de objetos diminutos y celdas sin escaneo). Las cargas son sintéticas; falta una de servidor (W7).

### 10.3 Lectura honesta

- **w1** es el defecto H1: 5,8× en tiempo y **54× en memoria** frente a Go.
- **w3**: Varn iguala a Node y a .NET en tiempo, pero gasta 2,7× la memoria de Go y su pausa máxima (74,6 ms) es la peor.
- **w5**: el GC consume ~66 % del tiempo (143,5 de 217 ms). Sin GC, Varn estaría en ~75 ms, por debajo de V8 y de CoreCLR. **El GC es hoy el cuello de botella en cargas que asignan.**
- Go es de otra liga en tres frentes a la vez: marcado concurrente en otros núcleos, objetos sin punteros que no se escanean y valores en línea. Igualar su pausa con un heap de hilo único no es realista sin trabajo incremental (6.5).

Por eso el criterio 10.1 pide ganar en **cola** y no en pausa media o rendimiento puro: es donde las ventajas de Varn (sección 11) pueden superar a un diseño concurrente.

## 11. Ventajas estructurales de Varn

Ninguna es una promesa: cada una trae la precondición verificada en el código y el estado actual.

| # | Ventaja | Precondición verificada | Estado hoy | Qué habilita |
|---|---|---|---|---|
| V1 | **Valores sin identidad** | ADR-0026 implementado: `record`, `struct` y tupla son valores (`==` estructural, `with`). E6: un literal `{ id: i }` sigue siendo una celda completa (616 frente a 394 MiB para 3 M objetos) | No explotada | Arrays y campos de valores **en línea**, como `[]struct` en Go o `struct[]` en C#. Sin celda por elemento, no hay nada que marcar ni promover |
| V2 | **Heaps por aislado, sin compartir** | `isolate_spawn.rs:26-30`: cada aislado es un hilo con su propia `Vm` y su propio heap; los valores cruzan como `SendValue` | Existe | Una pausa nunca detiene a otro aislado. El GC de un aislado no se sincroniza con nadie |
| V3 | **GC semánticamente invisible** | No hay `WeakRef`, ni registro de finalización, ni finalizadores en la VM ni en la std (la única `finalize` del código es `NativeCtx::finalize`, que devuelve su argumento sin hacer nada: `heap/native.rs:252`). Los recursos usan `using` (suite 247) | Cierto hoy | Recolectar cuando convenga, en ocio o de inmediato, sin cambiar el comportamiento del programa. Conviene elevarlo a invariante: nada del lenguaje observa cuándo corre el GC |
| V4 | **Un planificador que conoce sus ociosos** | `pump_until` llama a `host_wait` cuando no hay tareas listas (`drive.rs:178-181`) | Sin GC ahí | **GC incremental en tiempo ocioso**: rebanadas de marcado mientras se espera E/S. En un servidor la pausa visible tiende a 0 sin hilos adicionales |
| V5 | **Mapa de punteros por tipo** | `GcLayout.slots` lista solo los huecos `Ref` y `Boxed` (`instance.rs:227-236`) | Se usa para instancias | Instancias sin referencias que no se encolan en el marcado (*noscan*, como Go) |
| V6 | **Inmutabilidad ⇒ menos barreras** | Una celda inmutable se construye con hijos que ya existen, así que solo apunta a celdas más viejas y nunca crea aristas viejo→joven | No explotada: `HeapObj::Object` es mutable en ejecución | No recordar objetos inmutables; barrera solo en campos de referencia `mut` |
| V7 | **Observabilidad por tipo estático** | El compilador conoce cada tipo y su layout | No existe | `vn heap`: bytes vivos por tipo, rutas hasta la raíz, histograma de pausas. Hoy .NET y V8 necesitan herramientas externas |
| V8 | **Eliminar asignaciones** | Medido: 4 M de temporales que no escapan produjeron 3 000 216 asignaciones jóvenes en lugar de 7 M (análisis de escape del JIT) | Funciona | Ampliar a reemplazo escalar entre llamadas y a V1 |

Hasta donde sé, ningún runtime reúne las ocho: V8 no tiene valores en línea ni tipos estáticos, Go no es generacional ni compacta, y la JVM carece de valores en línea hasta que llegue Valhalla. La combinación es el diferenciador, no una pieza aislada.

## 12. Diseño ampliado

Cuatro capas, en orden de rentabilidad:

1. **No asignar**: escape (V8) y valores en línea (V1).
2. **No escanear**: *noscan* (V5), objetos inmutables fuera del conjunto recordado (V6).
3. **No pausar**: menor barata y acotada (sección 6), mayor incremental por rebanadas con presupuesto proporcional a la asignación y ejecutado también en ocio (V4).
4. **No retener**: ritmo en bytes y devolución de bloques (6.1, 6.4).

Etapas adicionales (se suman a la sección 7; **S6 se concreta en S9**):

| Etapa | Cambio | Puerta (medible) | Se borra |
|---|---|---|---|
| **S7** | Valores en línea (ADR propio, depende de ADR-0026): arrays y campos de `record`/`struct`/tupla con layout estático | w3 y w5 con elemento de valor: pico ≤ 1,25× Go y GC ≈ 0. Memoria de 3 M registros < 100 MB (hoy 616 MiB) | la celda por valor y su `Shape` en tiempo de ejecución para esos tipos |
| **S8** | *Noscan* + mapa de punteros en el marcado + barrera solo en referencias mutables | Medir primero el marcado de 4 M instancias escalares; entra si el mayor mejora ≥ 20 % | el encolado de celdas sin referencias; barreras sobre almacenes inmutables |
| **S9** | Mayor incremental: rebanadas ≤ 1 ms con presupuesto por asignación y en `host_wait` | p99 de pausa de W7 con 4 M vivos ≤ 5 ms. Coste total de la barrera ≤ 3 % en el corpus | el mayor de parada total |
| **S10** | `vn heap`: bytes por tipo, rutas a la raíz, histograma de pausas | Salida reproducible; encuentra una fuga sembrada en un servidor de prueba | nada |
| **S11** | Verificación diferencial: el corpus con y sin estrés y verificador debe dar salidas idénticas; generador de grafos de objetos aleatorios | 0 divergencias en una ejecución nocturna fija | nada |
| **S12** *(condicional)* | Preasignación en el viejo por sitio de asignación cuando todo sobrevive (w3: 100 % promovido) | Entra si w3 mejora ≥ 15 % | trabajo de marcado y promoción de objetos que siempre sobreviven |

Orden recomendado: **S0 → S1 → S3 → S2 → S4 → S8 → S7 → S9**, con S10 y S11 en paralelo desde S0. Primero se cierran las brechas medidas (54× de memoria en w1, 66 % de GC en w5); luego los diferenciadores, empezando por V1 por ser lo que más celdas elimina.

## 13. Riesgos y límites

- **No se puede prometer lo que depende de la medición.** El criterio 10.1 puede fallar en alguna carga; la etapa correspondiente no entra y se documenta.
- **El marcado concurrente en otro hilo queda descartado** (`Rc`/`RefCell`, estructuras internas mutadas por el mutador). Por eso la pausa de Go no se iguala con la misma técnica. La apuesta de Varn es ganar en cola con V4 y V2.
- **El marcado incremental cobra una barrera en cada escritura de referencia.** Su coste es una puerta numérica (≤ 3 %).
- **V1 cambia el contrato del JIT, de `VmValue` y de los nativos.** Es la etapa de mayor riesgo y por eso lleva su propio ADR.
- **GC en ocio no ayuda a cargas de CPU pura** (`bench_multiple` no asigna). Para ellas valen las rebanadas por asignación.
- **Las cargas son sintéticas.** Antes de dar por buena la superioridad hace falta W7, un servidor HTTP con carga sostenida y medición de latencia.

## 14. Decisiones abiertas adicionales

- **D4 — Criterio de "superior"** (10.1): ≤ 1,25× del mejor y ganar en cola. Alternativas: un múltiplo distinto, o exigir ganar también en rendimiento puro.
- **D5 — Orden**: se recomienda cimientos primero (S0–S4) y valores en línea (S7) antes que el mayor incremental (S9).
- **D1 queda informada por la línea base**: V8 tiene pausa máxima de 31,6 ms con 4 M vivos y Go 0,6 ms. Propuesta: ≤ 5 ms por rebanada, con p99 ≤ 5 ms en W7.

## Apéndice A — Experimentos

Binario: `target\release\vn.exe` (commit `7dc95881`). Contadores con `vn bench FILE --runs 1 --warmup 0 --allow-all -v`; trazas con `VARN_GC_TRACE=1 vn run FILE --allow-all`; memoria muestreando `WorkingSet64` cada 100 ms. Todos usan:

```
class Node {
    id: int
    constructor(id: int) { this.id = id }
}
const ring: Node[] = []
for (let i = 0; i < 1024; i = i + 1) { ring.push(new Node(0)) }
```

| Id | Programa | Medida | Resultado |
|---|---|---|---|
| E1 | 300 × `Array.filled(500000, i)` | pico | 1192 MB |
| E2 | 3 M nodos retenidos, `keep = []`, 6 M de churn | conjunto de trabajo | 10 → 186 → 229 MB, sin bajar |
| E2b | construcción pura de 3 M, y la misma con soltar + churn | mayores | 2 y 3 |
| E2c | E2 + otra construcción de 3 M | pico | 245 MB: la memoria muerta se reutiliza |
| E3 | N = 0 / 1 M / 2 M / 4 M vivos + 4 M de churn | pausa mayor máx. | 2,3 / 14,9 / 21,7 / 61,6 ms |
| E4 | N = 0 / 100 k / 400 k closures + 4 M de churn | pausa menor (últimas 3) | 1,9 / 3,0–4,6 / 7,3–7,8 ms |
| E5 | ventana de 500 k nodos, 6 M de reemplazos | promovidos / mayores / GC | 4,0 M / 4 / 134 ms |
| E6 | 3 M instancias contra 3 M literales | pico, pausa mayor | 394 contra 616 MiB; 60–74 contra 79–165 ms |
| E7 | `Map` viejo de 0 / 500 k / 1 M entradas | pausa menor (últimas 3) | 1,6–1,9 / 2,8–3,9 / 3,6–4,0 ms |
| Estrés | `VARN_GC_STRESS=2048` y `=256`, `vn run tests/main.vn` | resultado | `PASSED: 5542, FAILED: 0` en ambos (66 s y 62 s); con `=256` el programa E4 hace 15 629 menores en vez de 5 |

Pausas del mayor por fase (instrumentación temporal, descartada): `http_routing` marcado 4,6 ms + barrido 3,8 ms con 330 k supervivientes; `csv_pipeline` 1,8 + 3,1 ms con 250 k.

## Apéndice B — Comparación entre runtimes

Código: `tests/benchmarks/gc/` (`w1.vn`, `w3.vn`, `w5.vn`, `gcbench.js`, `go/main.go`, `net/Program.cs`, `compare.ps1`). Las tres cargas hacen lo mismo en cada lenguaje:

- **w1**: 300 veces, reservar un array de 500 000 enteros de 64 bits y descartarlo.
- **w3**: 4 M objetos `Node{id}` retenidos en una lista y 4 M más asignados a un anillo de 1024.
- **w5**: ventana de 500 000 `Node` y 6 M de reemplazos en círculo.

Ejecución: `.\tests\benchmarks\gc\compare.ps1 -Runs 3` (Varn con `target\release\vn.exe`, que debe existir). Métricas: tiempo de la carga dentro del proceso (en Varn, la fase `execute` de `vn bench`), pico del conjunto de trabajo leído del proceso, pausa total y máxima según lo que expone cada runtime (`PerformanceObserver` en Node, `MemStats` en Go, `GC.GetTotalPauseDuration` en .NET, contadores de `vn bench -v` en Varn).
