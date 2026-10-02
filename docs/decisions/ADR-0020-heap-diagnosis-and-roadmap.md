# ADR 0020: Diagnóstico medido del heap y hoja de ruta

## Estado
Diagnóstico cerrado y tres correcciones implementadas (2026-10-02, matriz 4/4
verde, 2027 aserciones). Las etapas H3–H5 están propuestas, no implementadas.

## Contexto
El heap es una tabla de handles: cada valor visible para la VM ocupa un slot
`Option<HeapObj>` de 48 B (nursery de 65 536 slots + `Vec` del old gen) cuyo
contenido casi siempre es un `Rc` a un payload asignado aparte (instancia
compacta, array, objeto, handle de tarea). El "GC" reubica y libera handles; el
payload vive en malloc. Alrededor hay dos mundos de valores: `VmValue` (palabra
empaquetada, dentro de la VM) y `Value` (enum portable, frontera con nativos,
tareas e isolates), unidos por `intern`/`extract`.

Se midió contra Node 24, Bun y Go en esta máquina (release, Windows x86-64):

| Carga | Varn antes | Varn ahora | Node | Bun |
| :--- | :--- | :--- | :--- | :--- |
| 20M `new P(i,1)` que no escapa (función) | 1253 ms | **12 ms** | 13 ms | 18 ms |
| 20M `new P(i,1)` que no escapa (módulo) | 1281 ms | **34 ms** | 13 ms | 18 ms |
| 3M objetos retenidos en un array | 481 ms · 502 MB | 412 ms · 502 MB | 255 ms · 324 MB | 120 ms · 255 MB |
| GC menor promoviendo 49 152 objetos con el array grande en old gen | 3.5 ms | **1.09 ms** | — | — |
| 3M arrays temporales + 1M retenidos | 216 ms · 172 MB | **81 ms** · 172 MB | 52 ms · 84 MB | 49 ms · 123 MB |
| 500k claves string en un `Map` | 358 ms · 155 MB | 323 ms · 155 MB | 226 ms | 301 ms |

## Hallazgos (cada uno con su evidencia)
1. **La eliminación de objetos que no escapan estaba muerta.** `passes::escape`
   solo reconocía `LoadGlobal(nombre)`; desde que el checker numera los globales
   el SSA carga la clase con `LoadGlobalIdx(slot)` y la pasada nunca disparaba.
   `tests/63-escape-analysis.vn` pasaba porque comprueba valores, no que la
   asignación desaparezca. Era la causa de los ~60 ns por objeto temporal.
2. **La barrera de escritura marca el array entero.** Un `push` sobre un array
   del old gen lo dejaba en el conjunto recordado y cada GC menor reexaminaba
   los N elementos, no los nuevos: coste cuadrático con el tamaño del array.
3. **`extract(Array)` copia el array completo** a un `ArrayRef` portable e
   `intern` lo copia de vuelta. No es un coste caliente en las llamadas
   ordinarias a nativos: `slice`, `includes` e `indexOf` sobre un array de 1M
   toman 0–1 ms por 2000 llamadas porque reciben el handle (`VnArray`). Solo
   afecta a los caminos que aún pasan `Value::Array` (argumentos de tareas,
   isolates), sin medir.
4. **Dos asignaciones y un hash por objeto de identidad.** Slot de 48 B más
   payload en malloc; `Task`, `TaskHandle`, `Class`, `Generator` y `VmClosure`
   además entran en `identity_index` (hash). Una tarea crea tres objetos así
   (~150 B cada uno), que es la brecha que queda frente a Node a 1M tareas.
5. **Asignar un objeto que sí escapa cuesta ~57 ns** (`jit_new_window`: closure
   del constructor, plan, `Rc`, slot, barrera) frente a ~2 ns de un bump.
6. Descartado: `update_interners_after_minor_gc` recorría las siete tablas en
   cada GC menor, pero solo `object_interner` puede contener índices de
   nursery; se sustituyó por una lista de entradas jóvenes. Sin ganancia
   medible (las tablas eran pequeñas): queda como simplificación.

## Decisión
Implementado (cada paso es parte del diseño final, ninguno es un parche):
- **H1** `CtorSummaries` resuelve la clase por nombre y por slot global y
  `escape::run` reconoce `LoadGlobalIdx`. Ganancia: 100× en objetos que no
  escapan. Verificación: `churn` 12 ms, matriz 4/4. Se borra: nada (la pasada
  ya existía; estaba inalcanzable).
- **H2** `BoxedElems` guarda un prefijo limpio (`clean_prefix`) junto al `Vec`
  (primer campo, los offsets del JIT no cambian). `set_vm` lo baja al índice
  escrito, `pop_vm` al nuevo largo, y el GC (`scan_dirty`) solo visita
  `[prefijo..len)`. El JIT solo escribe inline escalares en un buffer `Boxed`
  (sin referencias); todo valor de heap pasa por el helper con barrera.
  Ganancia: GC menor 3.2× con un array grande en old gen. Verificación:
  `tests/155-array-remembered-prefix.vn`, que revienta si se rompe el
  invariante (comprobado). Se borra: `VmArray::borrow_mut` (sin llamadores) y
  `as_boxed_mut`.

- **H6** `fixed_fields` reenvía las lecturas de índice constante de un literal
  de array que no escapa (`push`, `length`, `SetIndex` y cualquier otro uso lo
  descalifican; solo si el tipo SSA del elemento coincide con el de la lectura,
  para no saltarse una conversión de representación al guardar). Ganancia:
  `arrays` 210 → 81 ms. Verificación: `tests/156-array-literal-forwarding.vn`
  (bordes: `Array<float>` con literales int, índice fuera de rango, `push`,
  escritura, alias, paso a función, índice dinámico). Se borra: nada.

- **H4** Auditado quién consume `identity_index`: `intern` nunca deduplica
  `Task` ni `TaskHandle` (siempre asigna slot nuevo), así que su entrada solo
  servía para un marcado conservador del GC que no protegía nada (un valor
  portable ya posee su `Rc`; los `VmValue` vivos son raíces). Se quitaron esas
  dos entradas. `Class`, `Generator` y `VmClosure` conservan el índice: el GC
  lo usa para pasar de un puntero crudo de closure a su slot, y un frame que no
  posee su closure depende de ello. La idea original (guardar el slot en el
  payload) queda descartada: el GC mueve objetos y habría que corregir la pista
  en cada evacuación para ahorrar menos de lo que cuesta. Ganancia: 1M tareas ×
  8 s pasa de 9.14 s / 1021 MB a 8.85 s / 957 MB, spawn 259 → 187 ms.
  Verificación: matriz 4/4. Se borra: dos arms de `identity_key` y de
  `value_heap_idx`.

Propuesto, en este orden:
- **H3 Frontera de arrays sin copia (baja prioridad).** Medir primero el
  paso de un array grande como argumento de una tarea async; si cuesta O(n),
  pasar el handle. Verificación: microbench de `spawn` con un array de 1M. Se
  borra: la rama `Array` de `extract`/`intern` salvo para isolates.
- **H5 Instancias nuevas sin malloc.** Reservar el payload compacto de una
  instancia joven en el propio nursery (bump) y copiarlo al promover. Ganancia
  objetivo: de ~57 ns a ~10 ns por `new` que escapa. Verificación: `retain`.
  Es el cambio con más riesgo: toca `InstanceRef` (hoy `Rc<InstanceData>`
  compartido con `Value`), el JIT (`emit_nursery_alloc`) y la promoción.
Un heap con objetos de cabecera inline y copia generacional sin handles (el
diseño de V8/JSC) resolvería H3–H5 a la vez, pero obliga a reescribir `Value`,
los 521 usos de `HeapObj::` en 56 archivos, el JIT y los nativos. No se
recomienda empezar por ahí: H3–H6 dan la mayor parte de la ganancia medible
con cambios que se pueden validar uno a uno contra `tests/main.vn`.

## Consecuencias
- `tests/63-escape-analysis.vn` no detecta que una asignación desaparezca.
  Falta un contador de asignaciones visible desde `.vn`; sin él, H1 puede
  volver a romperse en silencio.
- `clean_prefix` es un invariante de `VmArray`: cualquier mutador nuevo debe
  bajarlo. Hoy están todos en `vm_value.rs`.
