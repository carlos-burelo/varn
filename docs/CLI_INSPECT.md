# Inspección fases (`vn debug`)

Código manda. Fuentes: `crates/varn-cli/src/cli.rs:99-113`, `crates/varn-cli/src/commands/debug.rs:4-42`, `crates/varn-debug/src/registry.rs:182-199`, `crates/varn-debug/src/flags/phases.rs:5-71`, `crates/varn-core/src/debug_flags/state.rs:1-56`, `crates/varn-cli/src/debug_sink.rs:6-12`, `crates/varn-debug/src/clif.rs:15-32`.

## CLI (`cli.rs:99-113`)

```rust
pub struct DebugArgs {
    pub file: Option<String>,
    pub eval: Option<String>,      // -e/--eval CODE
    pub phase: String,             // -p/--phase, default "all"
    pub fn_filter: Option<String>, // --fn NAME
    pub list_phases: bool,         // --list-phases
}
```

Flujo (`commands/debug.rs:4-42`): `--list-phases` → `print_phases()` y sale. Si no: `parse_debug_opt(phase)` (`debug_sink.rs:6-12` → `parse_debug_flags`) + `fn_filter`, requiere file o `--eval`, `no_run = !needs_execution()` (solo `gc` necesita correr, `state.rs:61-63`).

## Uso

```bash
vn debug --list-phases
vn debug -p <fase> file.vn
vn debug -p <fase> -e '<codigo>'
vn debug -p bytecode --fn NAME file.vn
```

Lista fases nunca se copia de docs: obtener con `--list-phases` (imprime por stage + grupos + sub-fases, `flags/phases.rs:5-71`).

## Fases registradas (`registry.rs:182-199`)

`ALL` hoy (16 ids):

| Id | Título | Stage | En `all` |
|---|---|---|---|
| `tokens` | flujo tokens lexer | Lex | sí |
| `ast` | árbol sintáctico | Parse | sí |
| `modules` | imports/exports | Parse | sí |
| `symbols` | tabla símbolos + tipos | Check | sí (grupo `check`) |
| `check:types` | tabla tipos diffeable | Check | no |
| `bytecode` | bytecode por función | Compile | sí |
| `scope` | árbol scopes | Compile | sí |
| `caps` (alias `cap`, `cap-trace`) | traza capabilities | Compile | sí |
| `graph` | grafo dependencias | Compile | sí |
| `summary` | tamaños, exports, top-10 | Compile | sí |
| `typeloss` | opcodes genéricos con equivalente tipado | Compile | no |
| `tir` | IR tipado del checker | Compile | no |
| `tiers` | tier por función clif/gate/bail | Compile | sí |
| `bails` | lo que no rutea, por causa | Compile | sí |
| `clif` | lowering Cranelift | Compile | sí |
| `gc` | young/old-gen/interners al terminar | Exec | no (única que ejecuta) |

Grupos: `check` = símbolos+tipos, `all` = fases con `in_all` (`flags/phases.rs:33-44`).

## Sub-fases (`flags/phases.rs:46-61`)

```bash
vn debug -p check:types file.vn
vn debug -p tir:check file.vn
vn debug -p clif:route file.vn
vn debug -p clif:kinds file.vn
vn debug -p clif:ir file.vn
vn debug -p clif:asm file.vn
vn debug -p clif:check file.vn
vn debug -p clif:all file.vn
```

`clif:check` solo informa bails sin volcado (`clif.rs:59-67`).

## JIT (`clif`)

```bash
vn debug -p clif --fn NAME file.vn
```

`debug_clif` (`clif.rs:15-32`): ISA host vía `shared_isa()`, recorre proto + funciones anidadas, filtra por `--fn` (substring sobre nombre, `clif.rs:40-46`), inspecciona con `NoLinker`. Flags `DebugFlags` (`state.rs:39-46`): `clif`, `clif_route`, `clif_kinds`, `clif_ir`, `clif_asm`, `clif_check`.

## Flags resto (`state.rs:1-56`)

`tokens ast bytecode symbols symbols_all binds modules types types_all types_range expr expr_range check_types errors trace calls consts scope graph cap_trace info lsp* tir tir_check tiers bails summary typeloss gc fn_filter`. Parseo en `varn-debug/src/flags/spec.rs`; rangos `types:N`, `expr:N` (`flags/phases.rs:63-65`).

## Ejemplos

```bash
vn debug -p tokens hola.vn
vn debug -p ast hola.vn
vn debug -p bytecode hola.vn
vn debug -p tiers hola.vn
vn run --trace hola.vn
```
