# Arquitectura VM (`crates/varn-vm`)

## 1. Frames y registros

`FrameStore` (`frame_store.rs:26-33`): 4 arenas por clase + metadatos.

```rust
pub struct FrameStore {
    pub gpr: Vec<i64>,
    pub fpr: Vec<f64>,
    pub refs: Vec<Option<HeapRef>>,
    pub dyn_: Vec<VmValue>,
    pub(crate) allocs: Vec<FrameAlloc>,
}
```

`FrameAlloc`: `bases: [u32; 4]` (índice por `SlotClass::index()`) + `layout: Rc<FrameLayout>`.
`FrameLayout` (`register_meta.rs:54-58`): `slots: Vec<(SlotClass, u32)>`, `counts: [u32; 4]`.
`SlotClass`: `Gpr | Fpr | Ref | Dyn`; `SlotKind::of_kind`: Int→Gpr, Float→Fpr,
Ref→Ref, Dynamic|Bool|Str→Dyn (`register_meta.rs:26-46`).

`CallFrame` (`frame.rs:9-17`): `closure_ptr`, `_owned_closure`, `ip`, `base`,
`current_class`, `return_reg`. `MAX_CALL_DEPTH = 10000`, `VM_STACK_BYTES = 64 MiB`.

## 2. `VmValue` (128-bit)

`vm_value/mod.rs:1-6`: `#[repr(C)] struct VmValue { tag: u64, payload: u64 }`.
Kinds: `KIND_NULL 0`, `KIND_BOOL 1`, `KIND_INT 2`, `KIND_FLOAT 3`, `KIND_HEAP 4`,
`KIND_SSO 5`, `KIND_SYMBOL 6`. SSO: len en bits 8-15 (`SSO_LEN_SHIFT = 8`),
`SSO_MAX_LEN = 5` bytes inline en payload.

## 3. GC en dos niveles

Sin nursery contigua: el bump vive en lanes por size-class (`cells/blocks.rs:16-20`
`AllocLane { free, bump, end }`, `SizeClass { blocks, lane }`).

- Minor: `HeapInner::minor_gc` (`heap/minor.rs:8-52`): marca jóvenes desde roots +
  `remembered` + `scan_roots` + `young_cells`/`young_lazies`, luego `sweep_young`
  (promociona supervivientes).
- Major: `mark_and_sweep` (`heap/major.rs:7-30`): mark tricolor por worklist +
  `sweep_major`.
- `collect` (`heap/gc.rs:53-68`): major + vacía `born`/`remembered`/`young_cells`/
  `young_lazies`, compacta interners, reconstruye `scan_roots`, rearma
  `gc_threshold = max(live*2, 65536)`.
- Disparo minor por conteo: `YOUNG_THRESHOLD = 786432` sobre `born.len()`
  (`heap/young.rs:3,21-23`).
- `write_barrier` (`heap/gc.rs:30-35`): si parent es `Old` y el valor es joven,
  parent pasa a `Remembered` y entra en `young.remembered`.

## 4. Excepciones: doble mecanismo, un despacho

- `try_handlers: Vec<TryHandler>` en `ExecCtx` (`exec/ctx.rs:34`);
  `TryHandler { catch_ip, frame_depth, err_reg }` (`frame.rs:56-60`).
  `Try`/`PopTry` los empujan/sacan (`dispatch/ops_misc.rs:17-35`,
  `exec/exceptions.rs:7-22`).
- `exception_table: Vec<ExceptionRange>` en el proto
  (`chunk/proto/definition.rs:41`); `ExceptionRange { try_start_ip, try_end_ip,
  catch_ip, err_reg }` (`chunk/proto/records.rs:8-13`).
- `dispatch_to_handler` (`exec/exceptions.rs:137-178`): primero `try_handlers`
  con `frame_depth > depth`, después `exception_table` por `ip`, desenrollando
  frames hasta `depth`. `Throw` (`dispatch/mod.rs:339-356`) lo invoca; si no hay
  handler retorna `Err`.

## 5. Interop JIT

`run_compiled_frame` (`dispatch/jit_frame.rs:72-140`): ejecuta el frame compilado;
si `jit_error_flag != 0` lo limpia, toma el error de `jit_panic_exception_error`
o `jit_native_result` (`exec/ctx.rs:55-56`) y lo redirige al handler o lo devuelve
como `Failed`. Código longjmp 2 = suspend (retoma en `jit_panic_suspend_resume_ip`);
otro código = `panic`. El `setjmp`/`longjmp` vive en `varn-vm/src/arch/`
(`vm_setjmp`/`vm_longjmp`); `varn-jit` no contiene `longjmp` ni `personality`.

## 6. Async, generadores, isolates

- `Yield` (`exec/dispatch/mod.rs:354`) fija `ip`, escribe
  `ctx.vm_suspend = Yield { value, dest_reg }` y retorna.
- `Await` (`exec/dispatch/mod.rs:369`) delega en `begin_await`
  (`exec/ctx_tasks.rs`): valor no-tarea → `Ready`; `TaskHandle` resuelto/rechazado
  → `Ready`/`Thrown` (este último por `exceptions::raise`); tarea perezosa →
  `scheduler::mount` monta su cuerpo como frame de quien aguarda (`Entered`);
  solo una celda pendiente suspende (`vm_suspend = Await { cell, dest_reg }`).
  Los drivers entregan con `resolve_awaiter`/`reject_awaiter`. El JIT usa la
  misma `begin_await` (`jit_helpers/suspend.rs`).
- Generadores (`generator.rs`): `GenState` con fases `Fresh | Suspended |
  Running | Done`. `next` monta el frame sobre la pila de quien llama
  (`enter_generator`); `yield` congela solo ese frame (`freeze_frame`, términos
  relativos) y lo desmonta (`park_generator`); el siguiente `next` lo descongela
  (`thaw_frame`). Sin `ExecCtx` propio. El resultado es `{value, done}`
  (`make_iter_result`). Con código JIT, `generator_entry` entra directo (OSR en
  modo `Resume` si `ip != 0`).
- Isolates: un thread OS por isolate (`exec/host/isolates.rs:230-358`,
  `thread::Builder` con `VM_STACK_BYTES`, `Vm` nuevo, `isIsolate = true`).
  `TaskGroup` vive en la std (`std/task/group.vn`), no en Rust. No existe crate `varn-runtime`
  (el runtime auxiliar es `varn-rt`).
