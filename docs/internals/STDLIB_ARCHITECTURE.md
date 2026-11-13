# Arquitectura de la stdlib

Tres capas, de abajo arriba:

| Capa | Dónde | Qué es |
|---|---|---|
| `runtime:*` | `crates/vn-builtins/src/modules/runtime/<mod>/` | Nativos Rust con contrato `.vn` (`declare function …`) |
| `core:*` | `crates/vn-builtins/src/modules/core/` | Tipos y globales del lenguaje (métodos de `int`, `str`, `Array`, `Regex`, errores, decoradores) |
| `std:*` | `std/` | Biblioteca en Varn sobre `runtime:*`; es lo que importa el usuario |

La std se distribuye siempre como imagen `std.vnb` (ver
[`MODULES_SYSTEM.md`](MODULES_SYSTEM.md)); los nativos, ver
[`LBI_ARCHITECTURE.md`](LBI_ARCHITECTURE.md).

## `std/` (29 módulos, 152 archivos `.vn`)

| Módulo | Archivos |
|---|---|
| `cli` | `args`, `color`, `prompt`, `table` |
| `collections` | `list`, `lru`, `priority_queue`, `queue`, `stack` |
| `compress` | `gzip`, `tar`, `zip` |
| `crypto` | `hash`, `jwt`, `password`, `totp`, `uuid` |
| `db` | `redis` |
| `encoding` | `base32`, `base64`, `csv`, `hex`, `html`, `json`, `msgpack`, `toml` |
| `env` | `env` |
| `error` | — |
| `ffi` | `library`, `pointer`, `types` |
| `fs` | `async_ops`, `file`, `glob`, `ops`, `stats` |
| `gpu` | `buffer`, `device` |
| `http` | `auth`, `client`, `core`, `headers`, `mime`, `multipart`, `redirect`, `request`, `response`, `route`, `server`, `status`, `wire` |
| `io` | `stream` |
| `log` | `level`, `logger` |
| `markdown` | `ast`, `inline`, `parser`, `renderer`, `utils` |
| `math` | `constants`, `intrinsics`, `stats` |
| `net` | `encoding`, `ip`, `query`, `socket`, `udp`, `url` |
| `path` | `utils` |
| `pm` | `semver` |
| `process` | `child` |
| `reflect` | `introspect`, `meta` |
| `signals` | `effect`, `graph`, `owner`, `reactive`, `scheduler`, `state`, `task` |
| `sqlite` | `db` |
| `sys` | `args`, `env` |
| `task` | `channel`, `combinators`, `cron`, `group`, `sync` |
| `test` | `assert`, `suite` |
| `time` | `core`, `datetime`, `duration`, `instant` |
| `ui` | `component`, `edit`, `focus`, `frame`, `keys`, `layout`, `mount`, `navigation`, `render`, `responsive`, `state`, `style`, `text`, `window`, `widget/` (14 archivos) |
| `ws` | `client`, `server` |

Cada módulo tiene además su `mod.vn`, que reexporta la superficie pública. La
tabla se generó del árbol de `std/` el 2026-11-13. `regex` ya no es un módulo de
`std`: es un tipo de `core` (`modules/core/types/regex`). La medición de texto vive
en `std:ui/text`.

## Nativos (`vn-builtins/src/modules/mod.rs`)

26 entradas en `native_modules!`: `buffer`, `compress`, `crypto`, `csv`, `errors`,
`ffi`, `fs`, `globals`, `gpu`, `io`, `json`, `math`, `net`, `path`, `process`,
`reflect`, `sqlite`, `stderror`, `sys`, `task`, `testing` (sin entradas nativas),
`text`, `time`, `types` (`array`, `bigint`, `bool`, `bytes`, `char`, `decimal`,
`float`, `int`, `iteration`, `map`, `range`, `regex`, `set`, `string`), `ui` y `ws`.

## Resolución estática de llamadas a nativos

Una función de `std` cuyo cuerpo es una sola llamada a un nativo importado
(`return nativo$(a, b)`, con `as T` opcional sobre un parámetro de tipo, o la
llamada como sentencia) se registra como *forwarder* en el binder
(`vn-binder/src/forwarder.rs`, `vn_sem::symbol::Forward`). El emisor
(`vn-emit`, `NativeTarget` en `imports_exports.rs`) resuelve la llamada al nativo
en el sitio de uso, sin pasar por la función envoltorio. La variante tipada de un
nativo se declara en su contrato con `@decodes("<op>")` (`runtime:json`, `parse$`
→ `parse-typed`); la tabla de ops de runtime está en `vn-core/src/runtime_ops.rs`.

## GPU (`vn-gpu` + `runtime:gpu` + `std:gpu`)

- `vn-gpu`: arena de buffers con handles generacionales (`arena.rs`), grafo perezoso `Add/Mul/Scale/MatMul` evaluado al descargar (`graph.rs`), sonda de adaptadores `wgpu` (`backend.rs`) y ejecución WGSL con un `submit` por grafo y fallback a CPU ante cualquier error (`execute.rs`).
- Modelo de costo: `GPU_MIN_MACS = 200_000` (`lib.rs`); por debajo calcula la CPU. `engine()` informa qué motor resolvió la última operación.
- `std:gpu`: `device.vn` (`backend`, `deviceCount`, `adapterName`, `engine`, `isAvailable`, `getInfo`) y `buffer.vn` (`GpuBuffer`).
- Puerta: `tests/214-gpu-compute.vn`; bench `tests/benchmarks/vn/bench_gpu_matmul.vn`.

## UI (`vn-ui` + `runtime:ui` + `std:ui` + `std:signals`)

- `vn-ui`: ventana `winit`, render `wgpu` y `egui` 0.37 (`window.rs`, `render.rs`, `paint.rs`, `canvas.rs`, `input.rs`, `events.rs`, `comp.rs`, `quad.rs`, `present.rs`).
- `runtime:ui` (`vn-builtins/src/modules/runtime/ui/`): ventanas como handles, cola de eventos y lista de comandos de dibujo. Toda la capa está tras la capability `ui` (`--allow-ui`).
- `std:ui`: componentes, foco, edición de texto, layout, montaje, navegación, estado, estilos, `text` y una familia de widgets (`widget/`). `std:signals`: señales, efectos, estado y planificador reactivos.
- Puertas: `tests/215-ui-headless.vn`, `217-ui-capabilities.vn`, `219-widgets.vn`, `220-ui-declarative.vn`, `232-ui-layout.vn`, `235-signals.vn`, `241-ui-controls.vn`, `245-ui-scroll.vn`, `246-ui-overlays.vn`, `250-layout-parity.vn`, `254-ui-reactive.vn`. Demos: `examples/ui-demo.vn`, `examples/ui-declarative-demo.vn`, `examples/ui-all-components.vn`.
- [`UI-SPEC.md`](UI-SPEC.md) es la especificación objetivo; su estado frente al código no se volvió a auditar el 2026-11-13.

## Texto (`vn-text` + `runtime:text` + `std:ui/text`)

- `vn-text` (`lib.rs`): definiciones de fuentes y medición/forma de texto con el contexto de `egui`.
- `std:ui/text` (`std/ui/text.vn`): `fontCount`, `fonts`, `shape`, `measure`, `ascent`, `descent`, `wrap`.
- Los ids de glifo dependen de la fuente del sistema: los tests son estructurales. Puerta: `tests/216-text-shaping.vn`.

## Macros

`vn-macros` es el único crate proc-macro: `vn_contract!` (`src/vn_contract/`,
`contract_parse.rs` parsea el `.vn` con el parser real), `#[jit_slow]` y
`jit_helper_table!`. Lo consumen `vn-builtins` (contratos) y `vn-vm` (helpers JIT).
