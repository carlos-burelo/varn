# ADR 0021: Eliminación de `Value`

## Estado
Implementado (2026-10-02). Matriz de 4 cuadrantes verde: 2031 aserciones,
0 fallos, con `tests/157-task-value-handles.vn` como regresión de los tres
fallos de corrupción que motivaron el cambio.

## Contexto
`Value` se presentaba como el valor portable, pero parte de sus variantes
guardaba handles del heap de la VM sin regla de propiedad ni de enraizado.
Un handle crudo almacenado en un `Value` sobrevivía a un GC menor sin
reescribirse. Se midieron tres corrupciones silenciosas: argumentos de tarea,
resultados de tarea resueltos y la recolección de `parallel`. La auditoría
completa está en `docs/notes/2026-10-02-value-vmvalue-audit.md`.

## Decisión
Un solo mecanismo por frontera:

- Dentro de la VM todo valor es `VmValue`. Los datos ligados al heap nunca
  viajan en un tipo que aparente ser portable.
- Entre heaps (isolates, hilos) solo existe `SendValue`, una copia profunda
  construida y consumida a través de `NativeCtx`.
- Una tarea es un `TaskCell` del lado de la VM; una tarea aparcada guarda solo
  sus slots vivos (`Frozen`). El GC la recorre por la lista `young_cells` /
  `young_lazies`, no por conversión.
- `ClassObj`, `BoundMethod` y `EnumVariantData` guardan `VmValue`. El GC menor
  reescribe esos campos igual que los de un array o instancia
  (`ClassObj::for_each_value_mut` es el único recorrido, usado por el marcado y
  por el nursery).
- El punto de entrada de `Vm::run` recibe un `FunctionProto`; el `Closure`
  portable con upvalues desaparece.

## Qué se borra
`Value`, `ArrayRef`, `AllocVtable` y su registro global, `Closure`/`Upvalue`
portables, `VmValueRef`, el trait `VmValuePayload`, `HeapObj::VmValue`,
`NativeCtx::extract`/`intern`/`intern_value`, `heap/intern.rs` como puente,
los interners de array, objeto, mapa y conjunto, `value_heap_idx`,
`ModuleLoader::native`, `FrozenExport::VmClosure`, `init_heap`.

## Qué se añade a `NativeCtx`
Accesores tipados que sustituyen a `extract`: `as_char`, `as_bigint`,
`as_decimal`, `as_range`, `as_map`, `as_set`, `as_generator`,
`is_static_receiver`, `alloc_range_data`, `range_element`, `map_for_each`.

## Verificación
- `cargo check --workspace --all-targets` sin errores ni avisos.
- `vn run ./tests/main.vn` en los cuatro cuadrantes de `CONTRIBUTING.md`.
- Medido antes de este paso (misma rama): 1M tareas × 8 s, 21.3 s / 5.0 GB al
  inicio, 8.7 s / 1.0 GB tras la fase de tareas.

## Defectos hallados al migrar
- `alloc_decimal` interno fusionaba `3.00` con `3` (igualdad de `BigDecimal`
  ignora la escala). El interning queda solo para claves de mapa
  (`intern_decimal`); el resultado aritmético conserva su escala.
- `alloc_empty_map_vm` compartía un mismo `MapRef` entre mapas vacíos distintos.
  Cada uno recibe ahora el suyo.
- `Heap` implementa `NativeCtx`: con el trait importado, `heap.alloc_bound_native`
  resolvía al valor por defecto del trait (`null`) en vez del método real. La
  implementación de `Heap` reenvía ahora. Deuda abierta: `Heap` como
  `NativeCtx` a medias es la causa raíz de esa trampa.
