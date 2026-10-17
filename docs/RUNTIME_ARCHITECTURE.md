# Runtime Varn (estado código)

## No existe `varn-runtime`

`Cargo.toml` lista 29 miembros workspace (28 crates + `xtask`); ninguno es `varn-runtime`. Cero hits a `varn-runtime|varn_runtime` en manifests. Miembros incluyen `crates/varn-rt`, `crates/varn-vm`, `crates/varn-builtins`.

## Runtime real

- `crates/varn-rt/src/lib.rs`: mínimo AOT. Solo `main`→`_varn_main`, `varn_rt_print{,_int,_bool}`, `varn_rt_str_concat`, `varn_rt_panic`. Sin scheduler, sin canales, sin GC.
- `crates/varn-vm/src/exec/host/isolates.rs:230-358`: `spawn_isolate` crea thread OS (`std::thread::Builder::new().stack_size(VM_STACK_BYTES).spawn`), nueva `Vm` + heap propio, `isIsolate=true`, corre export y liquida `HostPromise`.
- `crates/varn-builtins/src/runtime/channel.rs:9-19`: `SendOutcome {Sent,Parked,Closed}`, `RecvOutcome {Item,Parked,Closed}`; registry global por id, `Mutex<ChannelState>` (`:29-37`), `create/send/try_receive/close`.
- `crates/varn-builtins/src/modules/runtime/task/task.rs`: `spawn` (`:13-15`), `sleep` (`:17-19`), `parallel` (`:21-23`), `spawnIsolate` (`:35-76`, exige referencia a fn exportada top-level), `channel` (`:78-91`), `Sender::send/close/dispose` (`:196-227`), `Receiver::next/receive/dispose` (`:235-278`), `ChannelClosed extends Error` (`:288-294`).
- `crates/varn-vm/src/exec/scheduler/`: `drive,mod,mount,pumps,queue,suspend.rs`. Cooperativo; `spawn_if_task`/`enqueue_detached` en `exec/host/isolates.rs:140-194`.

## `TaskGroup`

0 hits a `TaskGroup` en `crates/**/*.rs`. Vive en Varn: `std/task/group.vn` (`export class TaskGroup<T>`, `spawn/join/cancel`), sobre `parallel$/sleep$` de `runtime:task`. Usado en `tests/21-async.vn:2,43-81`, `42-*.vn:149`, `97-*.vn:36`.
