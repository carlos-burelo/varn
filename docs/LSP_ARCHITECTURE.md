# Tooling: LSP / DAP / PM (estado código, sin timings)

## `varn-lsp` — `crates/varn-lsp/src/`

Directorios: `analysis, backend, db, document, features, index, pipeline, query, util, workspace` (+ `constants.rs`).

- `backend/`: `capabilities, lifecycle, settings, progress, state, protocol, edit, insight, navigate, mem, sync/`.
- `analysis/`: hilo dedicado dueño del `Workspace`; peticiones como closures `FnOnce(&mut Analyzer)->R, R: Send`.
- `features/`: `hover, completion, definition, references, rename, diagnostics, semantic_tokens, inlay_hints, folding, formatting, symbols`, etc.
- `document/position.rs`: único mapeo UTF-16; sync incremental (`TextDocumentSyncKind::Incremental`).
- `pipeline/`: lex+parse+check por archivo; invalidación dirigida (editado + importadores transitivos).

## `varn-dap` — `crates/varn-dap/src/`

3 ficheros (módulos en `lib.rs`): `adapter.rs`, `driver.rs`, `protocol.rs`. `Driver` reexportado.

## `varn-pm` — `crates/varn-pm/src/`

6 módulos (`lib.rs`): `cache, fetcher, installer, lockfile, manifest, resolver`.

## CLI (`crates/varn-cli/src/cli.rs`)

- `Lsp(LspArgs)` (`cli.rs`): `--port/-p`, `--tcp`, `--stdio` (`cli.rs`). Ejecución en `commands/lsp.rs`: port→`run_server_tcp(127.0.0.1:port)`, tcp→`run_server_tcp(tcp)`, defecto stdio.
- `Dap` (`cli.rs`): variante unitaria, sin args. Ejecución `commands/dap.rs`: `adapter::run_stdio()`.
- `Pkg(PkgCommands)` (`cli.rs`): 7 subcmds (`cli.rs`): `Add, Remove, Install, Update, Tree, Doctor, Clean`.
