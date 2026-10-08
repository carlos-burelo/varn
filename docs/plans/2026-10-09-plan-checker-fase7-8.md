# Plan sesión siguiente — checker fases 7–8 (split final)

Estado al cierre 2026-10-08: 25 crates. `checker` 23.177 LOC tras extraer
`sem` (4.705: types/scope/symbol/semántica/bind/exports/portable/codec/cores/
resolver/compat), `binder` (6.851: binder/+core natives+paths) y `resolver`
(941). DAG `resolver→binder→sem`. Todo verde: `check --workspace --all-targets`,
`fmt`, suite 2290/0 en 3 modos, `std.vnb` bytes idénticos.

## Fase 7 — `sem::output` + `varn-emit` (~9.5k fuera de checker)

Base medida: `emit_module(program, arena, bind, expr_table, call_mappings,
desugar) -> TirModule` solo toma datos (cero `Checker`). `emit/` usa de
`checker` solo 3 nombres: `TypeEntry, Desugarings, InheritedField`. Deps de
`emit/`: `core+sem+tir+modules` (`layer::Layer`, `prelude_modules`).

1. `sem::output` (nuevo módulo en `varn-sem`): mover ÍNTEGRO
   `checker/records.rs` (`ExprInfo, TypeEntry, ScopeSpan, Desugarings,
   TestTarget, CheckResult+impl, CheckProfile, CheckOptions`) + structs
   `ForeignEnum` (`checker/foreign_enums.rs`) e `InheritedField`
   (`checker/foreign_classes.rs`). Todo ya es `sem/core` puro. Los métodos
   `collect_foreign_*` (necesitan `&mut Checker`) SE QUEDAN en checker.
2. `checker/records.rs`: borrar. `foreign_*.rs`: solo structs fuera, métodos
   dentro (importan structs de `sem::output`).
3. `varn-emit` (nuevo crate, ~8.7k): mover `emit/` tal cual (`mod.rs→lib.rs`).
   Deps: `sem+core+tir+modules`. Renombres: `crate::checker::{TypeEntry,
   Desugarings,InheritedField}` → `crate::output::`… no: `varn_sem::output::`.
   `crate::emit::` internos → `crate::`.
4. `checker/lib.rs`: fuera `pub mod emit`; fuera re-exports movidos
   (`CheckResult/CheckProfile/CheckOptions/ExprInfo/TypeEntry/ScopeSpan/
   Desugarings/TestTarget` → `sem::output`). Quedan `Checker` y
   `get_members_of_type`.
5. Repoint (~25 sitios, todos mecánicos):
   - `pipeline/compile.rs`, `debug/tir.rs`, `lsp/compiler_inspect`:
     `emit::emit_module` → `varn_emit::emit_module` (+dep `emit` en los 3).
   - `CheckResult/CheckProfile/CheckOptions/ExprInfo/TypeEntry/ScopeSpan/
     Desugarings/TestTarget` → `varn_sem::output::` en `pipeline/{debug_sink,
     check,compile,module_precompile}`, `cli/{debug_sink,bench/*}`,
     `debug/{expr,phases/*}`, `lsp/document/*`, `checker/` interno
     (`super::records::`, `checker::TestTarget`, etc.).
   - `ForeignEnum/InheritedField` → `sem::output` donde se nombren fuera.
6. Manifests: workspace `members/default-members` +`varn-emit`; `checker`
   +`emit`? NO (`checker` no usa `emit`; al revés). Revisar warnings
   `unused` resultantes (`tir` aún usado en checker? `modules`?).
7. Regla hierro: cero re-exports puente en `checker`. Todo directo a `sem`.

## Fase 8 — resto `checker` (~14k, una sola struct)

- `checker/` (5.9k) + `checker_expressions/` (8.2k) son `impl Checker` en
  ~60 ficheros: NO partible por movimiento. Criterio: extraer solo helpers
  sin `Checker/self` (auditar `narrowing/`, `members/`, `checker_call_types/`,
  `checker_generics`, `generic_substitution` en sesión). Lo que quede (~12-14k)
  es el tamaño terminal honesto del checker (comparar: `parser` 9k, `vm` 25k,
  `lsp` 14k: el monolito deja de ser anomalía).
- Futuro fuera de alcance: `vm` 25k y `lsp` 14k merecen el mismo tratamiento
  algún día; no bloquear esta sesión por ellos.

## Validación obligatoria por fase (no negociable)

1. `cargo check --workspace --all-targets` cero errores y cero `unused_*`.
2. `cargo fmt --all` (los renames por script rompen formato; prohibido
   `` `n `` literal en reemplazos PowerShell: usar `edit` o `[Environment]::NewLine`).
3. `cargo build --profile quick --bin vn` + `doctor` + `gen-contract-tables --check`.
4. `run tests/main.vn` 2290/0 en dev-checkout; regenerar `dist/std.vnb`
   (fingerprint cubre `checker`: el bundle viejo falla con `Superseded` por
   diseño) y repetir en `@embedded` + `VARN_NO_JIT=1`.
5. Extra fase 7: `cargo test -p varn-resolver -p varn-lsp` (caché y LSP tocan
   los tipos movidos).
6. `verify.ps1 -Quick` al final; matriz completa (`-Fast`, release) antes de
   commit. Sin comandos `git` sin autorización (AGENTS.md).

## Riesgos conocidos

- `checker/records.rs` lo tocan `lsp/document/*` (caliente) y `pipeline/*`:
  cualquier `pub(crate)` que se escape rompe fuera; preferir `pub` y podar
  después que al revés (el check lo canta).
- `Desugarings.foreign_enums` ata structs movidos con métodos que se quedan:
  no mover métodos, solo datos.
- `varn-types/chunk/proto/definition.rs:39,128` menciona `checker::` (parece
  comentario): verificar en sesión que no es dependencia real.
