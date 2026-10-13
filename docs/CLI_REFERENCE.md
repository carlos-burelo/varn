# CLI Reference (`vn`)

Fuente: `crates/varn-cli/src/cli.rs:16-299`. Código manda.

## Comandos (`Commands`, cli.rs:16-55)

Run / Check / Eval / Repl / Bench / Debug / Build / Test / Fmt / Pkg / Init / Doctor / Cache / Lsp / Completions / Dap / StdBundle / GenContractTables.

## Flags reales

### Run (`RunArgs`, cli.rs:58-96)

`vn run [file] [-e CODE] [-- ARGS] [-v] [--trace] [--allow-*] [--sandbox] [--compare-tiers]`

- `file: Option<String>` (posicional)
- `-e, --eval CODE`
- `script_args` tras `--`
- `-v, --verbose`
- `--trace`
- `-A, --allow-all`
- `--allow-read [PATHS]` (=`*` si sin valor)
- `--allow-write [PATHS]` (idem)
- `--allow-net [HOSTS]` (idem)
- `--allow-env [VARS]` (idem)
- `--allow-ffi`
- `--sandbox`
- `--compare-tiers`

### Check (`CheckArgs`, cli.rs:116-121)

`vn check <file> [-v]` — `file: String`, `-v/--verbose`.

### Eval (`EvalArgs`, cli.rs:124-129)

`vn eval <code> [-v]` — `code: String`, `-v/--verbose`.

### Repl (`ReplArgs`, cli.rs:132-135)

`vn repl [--debug-bytecode]`.

### Bench (`BenchArgs`, cli.rs:138-158)

`vn bench [file] [-e CODE] [--runs N] [--show-output] [-v] [--all-rows] [--min-clif-coverage PCT]`

- `file` opcional, `-e/--eval CODE`
- `--runs N` default `10`
- `--show-output`, `-v/--verbose`, `--all-rows`
- `--min-clif-coverage PCT: Option<f64>`

### Debug (`DebugArgs`, cli.rs:99-113)

`vn debug [file] [-e CODE] [-p PHASE] [--fn NAME] [--list-phases]`

- `file` opcional, `-e/--eval CODE`
- `-p, --phase PHASE` default `all`
- `--fn NAME` (`fn_filter`)
- `--list-phases`

### Build (`BuildArgs`, cli.rs:192-206)

`vn build <file> [-o PATH] [-t TARGET] [--native] [-v]`

- `file: String`
- `-o, --output PATH`
- `-t, --target TARGET` default `bytecode`
- `--native`, `-v/--verbose`

### Test (`TestArgs`, cli.rs:253-267)

`vn test [path] [--filter X] [-j N] [--fail-fast] [-v]`

- `path: Option<String>`
- `-f/--filter`, `-j/--jobs: Option<usize>`, `--fail-fast`, `-v/--verbose`

### Fmt (`FmtArgs`, cli.rs:270-278)

`vn fmt [path] [--check] [-v]` — `path` opcional, `--check`, `-v/--verbose`.

### Pkg (`PkgCommands`, cli.rs:221-235; `AddArgs` 209-213, `RemoveArgs` 216-218)

- `pkg add <alias> <origin>`
- `pkg remove <alias>`
- `pkg install` / `update` / `tree` / `doctor` / `clean`

### Init (`InitArgs`, cli.rs:161-166)

`vn init [dir] [--name X]` — `dir: Option<String>`, `--name: Option<String>`.

### Doctor (cli.rs:41)

`vn doctor`. Sin args.

### Cache (`CacheCommands`, cli.rs:238-240)

- `cache clean`. Único subcomando.

### Lsp (`LspArgs`, cli.rs:174-189)

`vn lsp [-p PORT] [--tcp HOST:PORT] [--stdio]`

- `-p/--port: Option<u16>`, `--tcp: Option<String>`, `--stdio: bool`

### Completions (`CompletionsArgs` + `Shell`, cli.rs:169-171,242-250)

`vn completions <shell>` — `Bash | Zsh | Fish | PowerShell | Elvish`.

### Dap (cli.rs:50)

`vn dap`. Sin args.

### StdBundle (`StdBundleArgs`, cli.rs:281-287)

`vn std-bundle [--std-dir D] [--out F]` — defaults `std` / `dist/std.vnb`.

### GenContractTables (`GenContractTablesArgs`, cli.rs:290-299)

`vn gen-contract-tables [--crate-dir D] [--out F] [--check]` — defaults `""` / `""`, `--check: bool`.

## Verificación

`scripts/verify.ps1` (params verify.ps1:34-39):

- `-Quick`: perfil `quick`, omite lint+bench, solo cuadrante 1 (dev-checkout+JIT). No sustituye matriz completa.
- `-Fast`: omite benchmark.
- `-SkipLint`: omite `cargo fmt --check` + `clippy`.
- `-CleanCache`: `vn cache clean` antes de cada cuadrante.

`scripts/verify.sh` (verify.sh:14-29): `--fast|-f`, `--skip-lint`, `--clean-cache`. Sin modo `-Quick`.
