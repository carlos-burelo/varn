# Tooling: LSP / DAP / PM (estado código, sin timings)

## `varn-lsp` — `crates/varn-lsp/src/`

10 dirs: `analysis, backend, db, document, features, index, pipeline, query, util, workspace`.

- `backend/`: `capabilities, lifecycle, settings, progress, sync/`, handlers vía `Backend::query`.
- `analysis/`: hilo dedicado dueño del `Workspace`; peticiones como closures `FnOnce(&mut Analyzer)->R, R: Send`.
- `features/`: `hover, completion, definition, references, rename, diagnostics, semantic_tokens, inlay_hints, folding, formatting, symbols`, etc.
- `document/position.rs`: único mapeo UTF-16; sync incremental (`INCREMENTAL`).
- `pipeline/`: lex+parse+check por archivo; invalidación dirigida (editado + importadores transitivos).

## `varn-dap` — `crates/varn-dap/src/`

3 ficheros (módulos en `lib.rs`): `adapter.rs`, `driver.rs`, `protocol.rs`. `Driver` reexportado.

## `varn-pm` — `crates/varn-pm/src/`

6 módulos (`lib.rs`): `cache, fetcher, installer, lockfile, manifest, resolver`.

## CLI (`crates/varn-cli/src/cli.rs`)

- `Lsp(LspArgs)` (`cli.rs:46`): `--port/-p`, `--tcp`, `--stdio` (`cli.rs:173-189`). Ejecución en `commands/lsp.rs`: port→`run_server_tcp(127.0.0.1:port)`, tcp→`run_server_tcp(tcp)`, defecto stdio.
- `Dap` (`cli.rs:50`): variante unitaria, sin args. Ejecución `commands/dap.rs`: `adapter::run_stdio()`.
- `Pkg(PkgCommands)` (`cli.rs:36-37`): 7 subcmds (`cli.rs:221-235`): `Add, Remove, Install, Update, Tree, Doctor, Clean`.
