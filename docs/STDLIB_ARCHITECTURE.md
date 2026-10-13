# Arquitectura stdlib

Medido desde `std/` real. Código manda.

## Tamaño

- 27 dirs raíz: cli, collections, compress, crypto, db, encoding, env, error, ffi, fs, http, io, log, markdown, math, net, path, pm, process, reflect, regex, sqlite, sys, task, test, time, ws.
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

23 `pub mod` (mod.rs:1-47): buffer, compress, crypto, csv, errors, ffi, fs, globals, io, json, math, net, path, process, reflect, regex, sqlite, sys, task, testing, time, types, ws.

`build_module` / `has_native_builder` delegan en `crate::dispatch` (mod.rs:51-57).

## Macros

- `varn_contract!` vive en `crates/varn-contract/src/lib.rs:8` (`#[proc_macro] pub fn varn_contract`).
- `varn-op-macros` solo expone dos proc-macros (`crates/varn-op-macros/src/lib.rs:1-14`): `#[jit_slow]` (attribute) y `jit_helper_table` (function-like). Módulos: `jit_helper_table.rs`, `jit_slow.rs`.
