# JIT fase B — lower sobre el frame por clases

Estado (2026-09-20): **JIT reactivado** (`FRAME_LAYOUT_V2_JIT_BAIL = false`).
Suite e2e 1223/1223 verde (debug y release); tier-parity idéntico en los tests
numéricos; bench `vn bench tests/main.vn` reporta **JIT 94.6% (35/37 fns)**.

Implementado: contrato compartido (`SlotClass`/`FrameLayout`/`REF_UNINIT`,
`FrameStore`/`FrameAlloc` `repr(C)`), probes ABI, helpers `home_store`/`home_load`,
home traffic por helper, `run_compiled_frame` sobre `FrameStore`, subset gate
(denylist de opcodes + flag de helper deshabilitado), y el flip.

Habilitado además: `Call` (vía `clif_call_fallback` → `call_vm_window`),
`CallNativeOp`, `Intrinsic`, `GetProperty`/`SetProperty`, `CallMethod`,
`InvokeVirtual`, `MakeClosure`, `CallSelf` (frame-aware) y `Try`/`Throw`/`PopTry`.
Todos leen/escriben los homes por `FrameStore` (act_id + registro).

Pendiente (fase B completa):
- **`Ref`** (mayoría de bails restantes). Ya corregido el bug de layout que lo
  hacía inviable: `InstanceData` es **compacto** (`class_field_repr`) pero el
  inline de campos y el constructor inline del JIT seguían usando stride 16B;
  las instancias ahora van al helper compact-aware y los `Object` dinámicos
  conservan el inline. El resto del bloqueo es de **procedencia `K`/
  `register_meta`**: la lattice `K` (flujo) y `register_meta` (meet por SSA)
  pueden discrepar en un registro — `Cons.length`'s `Move r2 = r6` acaba
  boxeando el payload de un campo `Ref` como `bool` y `set_addr` lo rechaza.
  Trazas (`VARN_HOME_TRACE`) muestran que en la iteración que falla un store a
  `r2 (Ref)` recibe `tag=bool` con payload basura, y que `r6` (la lectura del
  campo) no pasa por el helper compacto (toma el inline/Objeto). Es value-flow
  del JIT: la variable de un registro y/o el camino del acceso de campo
  divergen. Arreglarlo desbloquea clases/objetos/arrays/mapas/métodos.
- **Llamada nativa directa** compiled→compiled: **HECHO**. `emit_vm_call` pide a
  `jit_prepare_static_call` que empuje la activación del callee por `FrameStore`
  (`mov_cross` de los argumentos) y devuelva la entrada del wrapper compilado;
  el call site invoca el wrapper directamente y `jit_finish_static_call` hace el
  pop. Solo cae a la ventana VM (`clif_call_fallback`) para no-closure,
  async/generator/rest o callee sin código. Cobertura JIT del bench 97.6%
  (83/85).
- **Homes inline** `(clase, base+idx)` en vez de `home_store`/`home_load`.
- **generator/async**: suspensión/reanudación por clases.
- `CallSpread`, `InvokeRuntimeStatic`: sin migrar.

Este documento fija el contrato y el orden de commits atómicos.
Complementa `docs/AUDIT_RESPONSE.md` §K paso 9 y
`crates/varn-vm/src/jit/tiering.rs` (`TODO(fase-B)`).

## Por qué existe fase B

El frame pasó de un `Vec<VmValue>` contiguo (stride 16B, tag+payload) a un
`FrameStore` particionado en cuatro vectores: `gpr: Vec<i64>`, `fpr: Vec<f64>`,
`refs: Vec<u32>`, `dyn_: Vec<VmValue>`. El JIT direccionaba la pila contiguo
(`stack_ptr + reg*16`) y su ABI, sus home slots, sus call windows y ~25 helpers
de runtime asumían ese layout. Compilar contra memoria que ya no existe sería
generar código contra basura, así que toda compilación baila.

Lo que el lowering SÍ sigue haciendo bien: el cuerpo opera sobre `Variable`s SSA
de Cranelift (registros nativos), no sobre memoria. La memoria del frame aparece
solo en fronteras:

1. carga de argumentos en el prólogo (wrapper),
2. home slots (flush/reload para GC, OSR y resumption interpretada),
3. call windows (staging de args para llamadas a helpers/VM),
4. push/pop de `CallFrame`.

Eso acota el trabajo a esas cuatro fronteras más los helpers y el ABI.

## Contrato nuevo (única fuente de verdad)

Compartido en `varn-types::register_meta` (ya introducido):

- `SlotClass { Gpr, Fpr, Ref, Dyn }` — proyección física de `SlotKind`.
  `Bool` y `Str` siguen en `Dyn` a propósito (siguiente paso, no este).
- `FrameLayout { slots: Vec<(SlotClass, u32)>, counts: [u32;4] }` —
  `(clase, índice-dentro-de-clase)` por registro, puro de `register_meta`.
- `REF_UNINIT = u32::MAX` — sentinel de `Ref` nunca escrito, idéntico en VM y JIT.

Frontera JIT↔VM (a implementar):

- `base: usize` deja de ser un offset de pila y pasa a ser el **id de activación**
  (`FrameStore::push_frame`), que los helpers resuelven contra `FrameStore`.
- Los **bases por clase** de la activación viven en `FrameAlloc.bases: [u32;4]`;
  el lowering obtiene `(clase, idx)` del `FrameLayout` y direcciona
  `vec[base[clase] + idx]` (stride 8/8/4/16 según clase).
- Los **punteros de datos** de los cuatro `Vec` se recargan desde
  `ExecCtx.stack` después de cualquier llamada/safepoint que pueda empujar un
  frame y reasignar (misma disciplina que el actual `stack_data_offset`), y se
  pueden cachear solo en funciones leaf alloc-free.
- Roots GC por clase: `Gpr`/`Fpr` nunca son raíz; `Ref` siempre (saltando
  `REF_UNINIT`); `Dyn` filtrado por `is_heap`.
- Windows de llamada por clase: staging de argumentos, retornos y receiver
  usan `mov_cross`/`unbox_into_reg` como el intérprete, no copia cruda de 16B.

## Semántica de referencia (ya correcta en el intérprete)

Los helpers restaurados deben espejar estos caminos, no reinventarlos:

- `exec/calls.rs::materialize_frame` — staging→frame particionado canónico.
- `exec/reg_ops/calls.rs` (`exec_call_reg`/`exec_call_self`) — fast path de
  llamada (`push_frame` + `mov_cross`) y slow path (`stage` + `prepare_call`).
- `exec/frame_ctrl.rs::dispatch_prepared_call` — `Frame`/`Constructor`/nativo.
- `exec/frame_ctrl.rs::resolve_constructor_return`.
- `dispatch/mod.rs::reg_return` — box del resultado, pop, cierre de upvalues,
  `resolve_constructor_return`, escritura al `return_reg` del caller.
- `exec/calls.rs::try_prepare_call_fast` / `prepare_call`.
- `exec/ctx_frames.rs` — `push_frame`, `capture_upvalue`, `close_upvalues_in`.

## Orden de commits (cada uno verde con el bail aún en `true`)

1. **[hecho]** Contrato compartido: `SlotClass`/`FrameLayout`/`REF_UNINIT` en
   `varn-types`; `varn-vm` los re-exporta. Sin cambio de comportamiento.
2. **ABI de frame por clases**: `JitFn`/`raw_signature`/`build_wrapper` — `base`
   = id de activación; carga de argumentos por clase desde `FrameStore`;
   `CallSelf` reenvía los cuatro bases. Retarget de `stack_data_offset` y
   `JitFrameLayout.stack_{len,cap}` a los cuatro `Vec` de `FrameStore`.
3. **Home slots por clase**: `frame_base_addr`/`load_home`/`store_home`/
   `def_result`/`flush_boxed`/`reload_boxed` despachan por clase; `vars.rs`
   lleva `SlotClass` junto a cada `Variable`; safepoint roots por clase;
   `osr.rs` recarga registro a registro desde su vector de clase.
4. **Call windows e inline frame push**: `emit_helper_call_window`,
   `emit_vm_call`, `emit_inline_frame_push`, `emit_wrapper_call_and_finish` y
   `alloc/*` empujan/leen la activación vía `FrameStore`/`mov_cross`;
   `methods.rs`/`native.rs` dejan de calcular ventanas contiguas.
5. **Helpers VM**: reimplementar los ~25 tripwires de
   `exec/jit_helpers/{frames,calls,values,construct,natives,intrinsics,ic}.rs`
   contra `FrameStore`, espejando la semántica de referencia.
6. **`run_compiled_frame`**: restaurar el cuerpo (setjmp, 4 finales, unwind,
   `resolve_constructor_return`) sobre `FrameStore`.
7. **OSR**: prologue y `reload_boxed` por clase (paso 3 ya lo cubre, verificar).
8. **Flip**: `FRAME_LAYOUT_V2_JIT_BAIL = false`; re-auditar `jit_layout`,
   `frame_layout`, `emit`, `safepoints`, `clif_link`.
9. **Gates**: matriz `tests/main.vn` (4 cuadrantes), `56-tier-parity`,
   `58-clif-range`, `62-jit-osr`, `65-safepoint-roots`, `53-int-overflow`,
   `101-self-recursion-return-kind`, y la suite de benchmarks.

## Verificación por commit

Con el bail en `true`: `cargo check --workspace`, `cargo test -p varn-jit
-p varn-compiler`, `vn debug -p clif` (compila sin ejecutar), y
`cargo run --bin vn -- run .\tests\main.vn` verde. El flip (paso 8) exige la
matriz completa de `CONTRIBUTING.md`.

## Subset activo (fase B)

`clif::lower::try_compile` deja al intérprete un proto cuando:

1. es `generator`/`async`;
2. contiene un opcode aún sin migrar (`CallSpread`, `InvokeRuntimeStatic`,
   `MakeClass`, `Inherit`, `Method`, `Define*`, `DeclareField`, `BindMethod`,
   `GetSuper`, `GetSymbol`, `Yield`/`Await`/`Spawn`, módulos, `LoadStaticFn`,
   `MakeClosure`… ya habilitado, ver arriba);
3. tiene algún registro `Ref` (el único home con validación estricta;
   `register_meta` no es autoritativo todavía).

Backstop independiente del denylist: `build_jit_helpers` pone a `0` la dirección
de los helpers aún tripwired, y `call_helper` marca un flag cuando el lowering
toca uno; `try_compile` mira el flag tras bajar y balea. Así ningún código
generado puede llamar a `unreachable!`/null por una omisión del denylist.

## Riesgos

- **Reasignación de los `Vec` de `FrameStore`** al empujar frames: los punteros
  cacheados quedan obsoletos. Mitigación: recargar en toda frontera de
  llamada/safepoint; cachear solo en leaf alloc-free (invariante verificable en
  `alloc::has_alloc` + ausencia de `has_boxed_slots`).
- **ABI Windows** (`StructReturn` para `VmValue` de 16B): el wrapper y el clif→clif
  deben declarar la convención por target, como ya hacen.
- **OSR mid-loop**: la elección de clase de cada registro debe coincidir
  exactamente con la del frame vivo; el `FrameLayout` compartido lo garantiza.
