# ARCHITECTURE — Varn (sync código: 2026-10-13)

Mapa sobrio del compilador. Toda cita es archivo verificable en el repo.

## Pipeline

Orquestador: `compile_source` en `crates/varn-pipeline/src/lib.rs:133-162`.
Fases, en orden:

1. **lex** — `crates/varn-pipeline/src/lex.rs` llama `varn_lexer::scan`
   (`crates/varn-lexer/src/scan.rs:3-14`): `source -> (tokens, lexeme_buf, errors)`.
2. **parse** — `crates/varn-pipeline/src/parse.rs` llama `varn_parser::parse`:
   `(tokens, lexeme_buf) -> (Program, AstArena, AtomInterner)`.
3. **check** — `crates/varn-pipeline/src/check.rs:48-67` llama
   `Checker::check_with(program, arena, interner, resolver, CheckOptions::compile())`
   y devuelve `varn_sem::output::CheckResult`.
4. **emit_tir** — `crates/varn-pipeline/src/compile.rs:47-60` llama
   `varn_emit::emit_module(program, arena, bind, expr_table, call_mappings, desugar)`
   (`crates/varn-emit/src/lib.rs:33-40`) y devuelve `TirModule`.
5. **compile_tir** — `crates/varn-pipeline/src/compile.rs:144-153` llama
   `varn_compiler::from_tir::compile_module(tir, export_names, source, measure)`
   (`crates/varn-compiler/src/from_tir/compile.rs:112-117`) y devuelve `FunctionProto`.
6. **execute** — `crates/varn-pipeline/src/execute.rs`: `boot_machine` crea `varn_vm::Vm`
   con `PipelineLoader`, corre builtins (`core_protos_owned`), `enter_main` registra el
   módulo y `drive_main` ejecuta. El tier JIT vive dentro de `varn-vm`
   (`crates/varn-vm/src/jit/tiering.rs` compila vía `varn_jit::compile` al calentarse);
   no hay fase JIT separada en el pipeline.

## Crates (28 members en `Cargo.toml:2-30`)

varn-core, varn-lexer, varn-parser, varn-checker, varn-emit, varn-types, varn-tir,
varn-sem, varn-binder, varn-resolver, varn-macros, varn-modules,
varn-builtins, varn-cli, varn-lsp, varn-vm, varn-debug, varn-pm, varn-jit,
varn-jit-abi, varn-pipeline, varn-compiler, varn-gpu, varn-text, varn-rt,
varn-shadow, varn-dap, xtask.

`default-members` (`Cargo.toml:32-57`) excluye `varn-rt`, `varn-shadow` y `xtask`.
No existe `varn-runtime` ni `varn-regalloc` como crates: regalloc es un módulo
interno (`crates/varn-compiler/src/regalloc/`, con `run_post_passes` aplicado al
final de `compile_module`).

## Bytecode

`OpCode`: 144 variantes en `crates/varn-core/src/opcode.rs:3-203` (`repr(u8)`,
índices 0-143). Excepciones: `Try` (línea 129, idx 93), `Throw` (línea 131, idx 94),
`PopTry` (línea 133, idx 95).

## Checker: qué produce

El checker NO produce ningún `TypedAST` (0 apariciones en `crates/`). Produce
`varn_sem::output::CheckResult` (`crates/varn-sem/src/output.rs:88-102`) con
`bind: BindResult`, `diagnostics`, `expr_table`, `call_mappings`, `desugar`,
`test_targets`. El TIR lo emite después `varn-emit` a partir de esos cuatro
artefactos, no el checker.
