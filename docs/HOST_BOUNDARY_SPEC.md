# Frontera host / VM

Código manda. Fuentes: `crates/varn-types/src/capabilities.rs:4-73`, `crates/varn-cli/src/cli.rs:58-96`, `crates/varn-cli/src/commands/run.rs:40-135`, `crates/varn-builtins/src/dispatch/mod.rs:203-245`.

## Regla

Nativo corre solo si `CapabilitySet` del proceso lo permite. Sin firmas host inventadas: cada módulo expone lo que su contrato `.vn` declara (ver `LBI_ARCHITECTURE.md`); frontera solo define gate + error.

## Máscara (`capabilities.rs:4-11`)

```rust
CAP_FS_READ CAP_FS_WRITE CAP_NET_CLIENT CAP_NET_SERVER
CAP_SYS_ENV CAP_SYS_EXEC CAP_SYS_FFI CAP_UI
```

`has_capability` acepta `fs.read|fs:read`, `net.client|net:client|net.connect`, `net.server|net:server|net.listen`, `sys.env|sys:env`, `sys.exec|sys:exec`, `sys.ffi|sys:ffi`, `ui`. Desconocido → `false`.

Filtros granulares: `check_fs_read/write` (prefijo path canonicalizado), `check_net_connect` (host, sufijo/subdominio), `check_net_listen` (puerto), `check_env` (nombre var), `check_ffi/exec/ui` (máscara).

## CLI (`cli.rs:58-96`, `run.rs:40-135`)

```bash
vn run file.vn                          # sin flags allow → allow_all
vn run file.vn --sandbox                # mask 0, todo denegado
vn run file.vn -A                       # --allow-all explícito
vn run file.vn --allow-read[=PATHS]     # default "*" si sin valor
vn run file.vn --allow-write[=PATHS]
vn run file.vn --allow-net[=HOSTS]
vn run file.vn --allow-env[=VARS]
vn run file.vn --allow-ffi
vn run file.vn --allow-ui
```

Semántica `build_capabilities`: `--sandbox` gana a todo. Sin ningún `--allow-*` (y sin `-A`) → `allow_all()`. Con algún allow → máscara solo con lo pedido (`--allow-net` activa client+server; hosts `h,host:puerto` parten en `net_hosts`/`net_ports`). `debug`/`eval`/`repl`/`check` usan `allow_all()` o default, sin flags (`commands/debug.rs:37`, `eval.rs:14`, `repl.rs:67`).

## Error

Denegado en `invoke_native` (punto único, `varn-vm/src/exec/ctx_gc.rs`), tabla única `dispatch` (`fn` → capability) → `E_HOST_PERMISSION_DENIED:id=...:capability=...`. VM lo eleva a excepción recuperable `try/catch` en Varn. Sin flags host inventadas: cada módulo expone lo que su contrato declara; la frontera solo define gate + error.

## Chequeo manual

```bash
vn run app.vn --sandbox
vn run app.vn --allow-read=/tmp --allow-env=HOME
```
