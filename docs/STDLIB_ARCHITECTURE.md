# Arquitectura stdlib

Medido desde `std/` real. Código manda.

## Tamaño

- 30 dirs raíz: cli, collections, compress, crypto, db, encoding, env, error, ffi, fs, gpu, http, io, log, markdown, math, net, path, pm, process, reflect, regex, sqlite, sys, task, test, text, time, ui, ws.
- 112 `.vn` recursivos (`Get-ChildItem std -Filter *.vn -Recurse`).

## Submódulos detallados

### encoding (9 ficheros: `mod.vn` + 8 submódulos)

`mod.vn`, `base32.vn`, `base64.vn`, `csv.vn`, `hex.vn`, `html.vn`, `json.vn`, `msgpack.vn`, `toml.vn`.

### cli (`mod.vn` + 4)

`mod.vn`, `args.vn`, `color.vn`, `prompt.vn`, `table.vn`.

### task (`mod.vn` + 5)

`mod.vn`, `channel.vn`, `group.vn`, `sync.vn`, `cron.vn`, `combinators.vn`.

### net (`mod.vn` + 6)

`mod.vn`, `url.vn`, `ip.vn`, `socket.vn`, `udp.vn`, `query.vn`, `encoding.vn`.

### http (12 ficheros)

`mod.vn`, `auth.vn`, `client.vn`, `core.vn`, `headers.vn`, `mime.vn`, `multipart.vn`, `request.vn`, `response.vn`, `route.vn`, `server.vn`, `status.vn`.

### ws (`mod.vn` + 2)

`mod.vn`, `server.vn`, `client.vn`.

## Nativos (`crates/varn-builtins/src/modules/mod.rs`)

25 `pub mod` (mod.rs): buffer, compress, crypto, csv, errors, ffi, fs, gpu, globals, io, json, math, net, path, process, reflect, regex, sqlite, sys, task, testing, text, time, types, ui, ws.

`build_module` / `has_native_builder` delegan en `crate::dispatch` (mod.rs:51-57).

## GPU (`varn-gpu` + `std:gpu` + `runtime:gpu`)

- `varn-gpu` (hoja: `parking_lot`/`rustc-hash`/`wgpu`/`pollster`/`bytemuck`): `arena.rs` (handles generacionales `(index, epoch)`, pool global, `try_gpu` por costo), `graph.rs` (DAG lazy `Add/Mul/Scale{factor}/MatMul`, eval en `download`), `backend.rs` (sonda `wgpu Backends::all`, `backend_name`/`device_count`/`adapter_name` reales, `cpu-graph`/0 sin adaptadores), `execute.rs` (WGSL `add/mul/scale/matmul`, un `submit` por grafo, readback por staging, fallback CPU en cualquier error).
- Modelo de costo: `GPU_MIN_MACS = 200_000` (`lib.rs`). Debajo va CPU, encima va GPU si hay device; `engine()` reporta el motor real del último `resolve` (`cpu`/`gpu`).
- `runtime:gpu` (`varn-builtins/src/modules/runtime/gpu/`): `create/destroy/len/dims/reshape/upload/download/add/mul/scale/dot/matMul/adapterName/engine` sobre handles `int`. Shape vive en el slot (`dims`), `matMul` infiere `m,k,n`. Sin `flush` público: `download`/`dot` sincronizan.
- `std:gpu`: `device.vn` (`backend/deviceCount/adapterName/engine/isAvailable/getInfo`), `buffer.vn` (`GpuBuffer` con métodos `add/mul/scale/dot/matmul/download/shape/reshape/dispose` + `create/zeros/upload`). Sin funciones libres: un camino.
- Puerta: `tests/214-gpu-compute.vn` (incl. routing por costo + sonda estructural) + `tests/benchmarks/vn/bench_gpu_matmul.vn` (checksum con tolerancia `f32/f64`; medido `n=128 ~8x` vs loops JIT en Vulkan local, re-medir).

## UI (`varn-ui` + `std:ui` + `runtime:ui`)

- `runtime:ui` (`varn-builtins/src/modules/runtime/ui/`, patrón `DB_POOL` de `sqlite.rs`): ventanas como slots con cola de eventos + `Vec<DrawCmd>` estructurado (`draw.rs`: `Clear/Rect/Text`, `format`, `assemble`). `open/close/shouldClose/size/inject/poll/rect/text/clear/present/commands/real/keyCode` sobre handles `int`. `inject` = misma cola que el backend real: un camino. `text` lleva `color` (breaking: el dibujo lo exige). `keyCode` expone la misma tabla nativa que usan las ventanas reales (nombre → código, nombre desconocido tira).
- `varn-ui` (hoja: `winit`+`wgpu`+`pollster`+`bytemuck`): `window.rs` (loop por pump en `thread_local`, `Box<WindowState>` estable, superficie persistente por ventana, `configure` una vez por tamaño, caché de último frame para repaint en resize, afinidad de hilo), `events.rs` (imprimibles por `to_text` + tabla única `KEYS` de 10 teclas nombradas: controles ASCII 8/9/13/27/32/127 + flechas U+2190–2193; repeats descartados, nombrada desconocida → 0), `render.rs` (superficie persistente `'static` sobre ventana boxeada, `AutoVsync`, `Lost/Outdated` → reconfigura), `comp.rs` (fachada: `quad.rs` tipos `Rect/Glyph`, `pipes.rs` 2 pipelines WGSL cacheados por formato, `present.rs` quads instanciados sin vertex buffers, atlas R8 1024 shelf, una submit).
- Compositing: `assemble` (builtins, con `varn_text`: baseline `y+ascent`, `draw at pen+ox+bx/baseline-by`) → `draw_now` → pipelines. Sin vertex/index buffers; buffers de instancia + uniforme + atlas por present. Pools persistentes descartados con medida: vacía 18.0ms vs 50 rects+texto ~18ms (domina `AutoVsync`); `configure`-una-vez 26→18ms ya captura lo medible. Rects con `radius` (SDF en el fragment, clamp en `assemble`): `rect$` lleva radio, default `0.0`.
- Backend real tras `VARN_UI_REAL=1`, si no headless; `real()` lo reporta. Ventana macOS fuera: gap documentado. `using` a top-level dispone al final del módulo (fix en `varn-emit`): la ventana real se cierra y el proceso sale limpio. Capacidad `ui` (`CAP_UI = 1<<7`): `runtime:ui` entero tras el gate de capabilities; sin flags todo permitido (igual que hoy), `--sandbox` o flags restrictivos sin `--allow-ui` → `E_HOST_PERMISSION_DENIED`. El gate vive en un punto único (`invoke_native`, tabla única `dispatch`): antes los métodos nativos por vtable lo saltaban (hueco que cerraba `--allow-ui` por fuera).
- `std:ui/window.vn`: `UiWindow` (`open/size/poll/rect/text/clear/present/commands/real/close/dispose`, `UiEvent`). `std:ui/keys.vn`: `KEY_BACKSPACE/TAB/ENTER/ESCAPE/SPACE/DELETE/ARROW_LEFT/UP/RIGHT/DOWN`, `MOUSE_LEFT/RIGHT/MIDDLE`, `keyCode(name)`, `hit(x,y,rx,ry,rw,rh)` (borde izq/sup dentro, der/inf fuera). `std:ui/frame.vn`: `UiFrame` retenido por ventana (`update` drena `poll` y calcula flancos: `leftPressed/leftReleased`, `pressed[]`, `held()`, `clicked()` exige down+up dentro, `closed` pegajoso, `resized` + `w/h`). `std:ui/focus.vn`: `Focus` retenido (`claim/has/current/next/prev/clear`, `-1` sin foco). `std:ui/layout.vn`: `pad/row/col` puros (`gap`; peso negativo = píxeles fijos, positivo = flex que reparte el resto; vacío → `[]`, flex ≤ 0 → 0) + `textX/textY` (centrado con métricas reales: `win.text` pone el top en `y`, baseline en `y+ascent`) + `ellipsize`. `inject` acepta `mouseDown/mouseUp` (misma cola y forma que el backend real; `mouse` solo es movimiento). `std:ui/edit.vn`: `TextEdit` (`update(fr, focus, id, rect)` reclama foco al click, pierde foco al pulsar fuera, inserta imprimibles vía `str.fromCharCode`, `Backspace/Delete`/flechas, `Enter` parte líneas, ignora sin foco; repeticiones del SO repiten la tecla vía `UiFrame.repeated`; `lines/caretLine/caretCol` para render). `std:ui/widgets.vn`: `Label/Box/Column/Row/Button/Field` (datos + nombrados con defaults) + `render` (una pasada layout+draw+input por `row/col`; `onPress/onClick` closures, `Field` integra `TextEdit` con ids auto ≥1000) + `App.run` (dueño del loop: ventana+frame+foco+Escape), `List/Checkbox/Spacer/Divider`. Named-args parciales vía `interface`+`??` (csv.vn).
- Puerta: ventana real pinta rect+texto (`present()=3` sin abort de validación), `215` verde en ambos modos (tabla de teclas + `hit` + round-trip de eventos + `UiFrame`/`Focus`/`layout` + `TextEdit` con repetición y multilínea), `217` (sandbox niega / `--allow-ui` otorga / defecto permite, por subproceso), `218` (using top-level + `while(&&)` + `break`: regresión del branch-block rancio en `from_tir`), métricas `ascent/descent` + colocación `textX/textY/ellipsize` + filas fijo/flex + widgets declarativos + radio SDF, suite 3408/0, corpus de errores verde, clippy cero. Siguiente: `--allow-ui` + señales (F5) y perf harness. Demo visible: `examples/ui-demo.vn` (caja rebotando + click + campo editable, `VARN_UI_REAL=1`).

## Texto (`varn-text` + `std:text` + `runtime:text`, C1)

- `varn-text` (hoja: `parking_lot`/`fontdb`/`swash`): `shape.rs` con DB sistema cargada una vez (`OnceLock`), `fonts()` (familias ordenadas/dedup), `shape_run(text, size)` (sans-serif → `with_face_data` + `swash` shaper → `ShapedRun{glyphs{id,advance,offset_x,cluster}, width}`). IDs dependen de la fuente del sistema: tests estructurales, nunca IDs fijos.
- `runtime:text`: `fontCount/fonts/shape` (objeto `{glyphs,advances,clusters,width,count}`) + `rasterGlyph` (objeto `{w,h,bx,by,advance,bitmap}`) + `glyphCacheSize` + `ascent/descent` (métricas swash, size inválido tira) + `wrap` (greedy por palabras, palabra larga se parte por glifo; size/max inválidos tiran). `std:text/shape.vn`: `fontCount/fonts/shape/measure/rasterGlyph/glyphCacheSize/ascent/descent/wrap`.
- Puerta: `tests/216-text-shaping.vn` (1 glifo por char ASCII, determinismo bit a bit, vacío, size inválido tira, raster cacheado/determinista con `bitmap.length == w*h`, `wrap` respeta max y `""` → `[]`).

## Macros

- `varn-macros` (`crates/varn-macros`): único crate proc-macro (fusión de `varn-contract` + `varn-op-macros`). Expone `varn_contract!` (`src/varn_contract/` + `contract_members.rs` + `contract_parse.rs`, que parsea el `.vn` con el parser real), `#[jit_slow]` (`src/jit_slow.rs`) y `jit_helper_table!` (`src/jit_helper_table.rs`). Consumidores: `varn-builtins` (contratos) y `varn-vm` (helpers JIT).
