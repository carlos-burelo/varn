# Auditoría de crates y tiempos de compilación — 2026-10-08

Metodología (Ley 12): solo código + ejecución. Ningún `.md` previo como evidencia.
Fuentes: `Cargo.toml` workspace + 26 manifiestos, conteo `*.rs`, `Cargo.lock`, tamaños `target/`, `build.rs`.

## 1. Cifras medidas

- Miembros workspace: 26 (`Cargo.toml:2-29`).
- Total: ~150.735 LOC en ~1.079 ficheros `.rs` bajo `crates/`.
- Distribución (LOC / ficheros):
  - `varn-checker` 35.902 / 258
  - `varn-vm` 25.780 / 174
  - `varn-lsp` 14.144 / 110
  - `varn-compiler` 14.014 / 91
  - `varn-parser` 9.291 / 51, `varn-jit` 9.291 / 73
  - `varn-types` 7.166 / 57, `varn-builtins` 5.948 / 53
  - `varn-cli` 5.498 / 59, `varn-core` 4.958 / 49
  - `varn-tir` 4.212 / 29, `varn-debug` 3.794 / 44
  - `varn-pipeline` 2.057 / 22, `varn-lexer` 1.729 / 12
  - `varn-modules` 1.641 / 14, `xtask` 1.567 / 11
  - `varn-contract` 1.061 / 7, `varn-pm` 714 / 7
  - `varn-dap` 646 / 4, `varn-debug-flags` 298 / 3
  - `varn-runtime` 235 / 3, `varn-op-macros` 219 / 3
  - `varn-fmt` 210 / 1, `varn-abi` 184 / 1
  - `varn-shadow` 111 / 3, `varn-rt` 65 / 1
- `Cargo.lock`: 317 paquetes.
- `target/`: ~96.8 GB totales. `debug` ~68.3 GB, `release` ~8.1 GB, `quick` ~4.1 GB, más `errcorpus/dbg/prof/dev-it/audit/flycheck0/tmp`.
- Síntoma duplicación: 10+ directorios `target/debug/build/varn-cli-*/out/std.vnb` (1.2–2.6 MB cada uno). Cada uno implica re-ejecución de `build.rs` con compilación stdlib completa.
- Sin `default-members` ni `exclude`: cualquier `cargo build/check/test/clippy` sin `-p` compila los 26 miembros.

## 2. P0 — `varn-cli/build.rs` recompila stdlib en cada build

- `crates/varn-cli/Cargo.toml:57-60`: build-deps `varn-pipeline + varn-modules + varn-builtins`.
- `crates/varn-cli/build.rs`: `rerun-if-changed=../../std` + `../../crates/varn-builtins`, luego `compile_stdlib_bundle()` y escribe `std.vnb` en `OUT_DIR`.
- Efecto: tocar `std/` o `builtins` invalida `vn` entero. Iteración frontend paga pipeline + backend + bundle.
- `crates/varn-cli/Cargo.toml:37,40`: `parser` y `vm` siempre con `features=["profiling"]`. Hoy `profiling=[]` vacío, pero fuerza unificación y cierra puerta a builds mínimos.
- Acción: bundle fuera de `build.rs` (`xtask dist` + embed precompilado), o `rerun-if-changed` por fichero, o feature `embed-std`. Quitar `profiling` forzado de `cli`.

## 3. P0 — `varn-vm` arrastra `varn-jit` (Cranelift) sin opción

- `crates/varn-vm/Cargo.toml:21`: `varn-jit` no opcional.
- `crates/varn-jit/Cargo.toml:11-15`: `cranelift-codegen/frontend/native/module/object 0.136.2`.
- `VARN_NO_JIT=1` es flag runtime: sigue pagando compilación Cranelift.
- Cadena: `crates/varn-cli/Cargo.toml:43,48` → `varn-debug` + `varn-jit`; `crates/varn-debug/Cargo.toml:10-14` → `checker+compiler+jit+iced-x86`. `varn-debug` solo lo usa `cli`, pero `cli` paga `iced-x86` + Cranelift siempre.
- Acción: `jit = optional` en `vm`, feature `jit` en `vm/cli/debug`. Medir `cargo build -p varn-cli --no-default-features` antes/después con perfil `quick` (Ley 13).

## 4. P0 — `default=["lsp"]` contamina iteración compilador

- `crates/varn-cli/Cargo.toml:19,44,49`: default `lsp`, `varn-lsp` + `tower-lsp-f` opcionales pero activos por defecto.
- `crates/varn-lsp/Cargo.toml:18-22`: `tower-lsp-f + tokio(full) + dashmap + once_cell`. `workspace.dependencies` fija `tokio full`.
- `crates/varn-cli/Cargo.toml:45-46`: `varn-dap + varn-pm` incondicionales. `varn-dap → pipeline+vm`; `varn-pm → ureq+tar+flate2+sha2`.
- Efecto: `cargo check` tras tocar `lexer` compila stack async + PM + DAP.
- Acción: invertir default (`vn` mínimo sin lsp), bin `vn-lsp` o `--features lsp`; `dap/pm` opcionales o bins separados. `tokio full` → features mínimos (`macros, rt-multi-thread, io-util, sync` según uso real).

## 5. P1 — `varn-contract` proc-macro depende del frontend

- `crates/varn-contract/Cargo.toml:6-7` proc-macro; `10-12` `proc-macro2/quote/syn full`; `20-22` `varn-core+varn-lexer+varn-parser`.
- Comentario `14-19` declara intención correcta (runtime nunca compila frontend), implementación inversa: cambio en `lexer/parser/core` invalida macro host + `builtins` (`varn-builtins/Cargo.toml:34`) + todo downstream (`vm, pipeline, cli, lsp`).
- Acción (Ley 8): macro solo `proc-macro2/quote/syn`; parse `.vn` a `TypeNode`/texto en `build.rs` o `builtins`; borrar dependencia frontend de macro en mismo esfuerzo.

## 6. P1 — `varn-checker` monolito bloquea paralelismo

- 35.9k LOC / 258 ficheros en un crate. Desviación ya conocida en `AGENTS.md §1`: mezcla `binder+types+checker+module_resolver+emit`.
- Todo downstream (`pipeline, vm, cli, lsp, debug, dap`) espera un solo `rustc` largo. En `release` (`codegen-units=1`, `lto=thin`) peor.
- Acción: split por dominio (`binder/types/checker/emit/module_resolver`) respetando Ley 1–4. Cada paso commit atómico (Ley 9).

## 7. P2 — Micro-crates: overhead sin ganancia

- `varn-abi` 184, `varn-rt` 65, `varn-shadow` 111, `varn-fmt` 210, `varn-runtime` 235, `varn-debug-flags` 298, `varn-op-macros` 219.
- Cada crate: metadata, fingerprint, LTO. Dos caminos runtime (`varn-runtime` vs `varn-rt` vs `varn-abi`) violan Ley 8 (una sola identidad).
- `varn-vm/Cargo.toml:15` usa `varn-runtime`; `varn-builtins` con `runtime` también tira de medio grafo externo.
- Acción: fusionar `abi+rt+runtime → varn-rt`; `fmt → core`; `debug-flags → debug`; `shadow+xtask` fuera de `members` (`exclude` o repo aparte).

## 8. P2 — Perfiles y `target/` multiplican costo

- `Cargo.toml:84-138`: `release opt-3/lto-thin/cgu-1/panic-abort`; `quick inherits release opt-3/lto-off/cgu-16`; `dev opt-1` pero `dev.package."*" opt-3`; `dev-it opt-0` existe pero `verify` fija `quick/release`.
- Efecto: build fresco optimiza terceros (Cranelift, `rusqlite bundled 0.31` con C, Tokio) aunque iteres `lexer`. `quick` no es iterativo pese al nombre.
- 7+ perfiles vivos duplican las dependencias más caras. Sin `sccache`, sin `CARGO_TARGET_DIR` separada por perfil documentada, sin `.cargo/config.toml`.
- Acción: borrar perfiles muertos (`dbg/prof/errcorpus/flycheck0/tmp/audit` si no se usan), `cargo clean -p` selectivo, evaluar `sccache`, `rusqlite` sin `bundled` en dev, documentar `quick` vs `dev-it` (Ley 13).

## 9. Plan propuesto (atómico, medible)

1. `vm/jit optional` + `debug` opcional en `cli`. Medida: `cargo build --profile quick --bin vn` antes/después.
2. `cli` sin `lsp` por defecto + `tokio` mínimo + `dap/pm` opcionales.
3. `contract` sin `lexer/parser/core`.
4. `build.rs` stdlib fuera del camino crítico.
5. Fusiones micro-crates + `exclude xtask/shadow`.
6. Split `checker` por dominio.
7. Limpieza perfiles + `sccache` + `rusqlite` dev.

Cada paso: un commit (Ley 9), declara ganancia + verificación + qué borra (Ley 10), sin adaptadores paralelos (Ley 8). Validación iteración: `cargo build --profile quick --bin vn` + `verify.ps1 -Quick`; matriz completa solo antes de commit.

## 10. Fase 1 aplicada — `build.rs` fuera del camino crítico

- Borrado `build-deps pipeline/modules/builtins` de `varn-cli`. `build.rs` trivial solo `rerun-if-changed=build.rs`.
- `main.rs` ya no exige `OUT_DIR/std.vnb` en compilación. Nueva `register_embedded_stdlib_if_available()`: feature `embed-std` incluye `dist/std.vnb`; sin feature solo registra si `VARN_STD_BUNDLE=/path` existe. Dev usa `SourceTree` sin bundle.
- Nuevo comando explícito `vn std-bundle --std-dir std --out dist/std.vnb` (`commands/std_bundle.rs`, `cli.rs:StdBundleArgs`). Reemplaza compilación implícita por invocación manual/CI.
- Verificado: `cargo check -p varn-cli` y `--no-default-features` verdes. `cargo build --profile quick --bin vn` en curso para probar `Dir` + `std-bundle`.
- Validado 2026-10-08: `quick --bin vn` en 1m43s (sin compilar std en build). `doctor` resuelve `source tree std v0.3.0` ok. `vn std-bundle` escribe `dist/std.vnb` 2.730.418 bytes. `VARN_STD=@embedded` sin bundle falla accionable; con `VARN_STD_BUNDLE=dist/std.vnb` resuelve `embedded v0.3.0` ok. Feature `embed-std` compila. `vn check tests/main.vn` corre.

## 12. Fase 3 aplicada — `varn-contract` sin frontend (`contracts.json`)

- `varn-contract/Cargo.toml`: fuera `varn-core/lexer/parser`. Dentro `serde_json`. Host proc-macro: `syn/quote/proc-macro2/serde_json`.
- Nuevo `vn gen-contract-tables` (`cli/commands/contract_tables.rs`): parsea 34 `.vn` con parser real, clasifica con código movido (no copiado) de `mapping.rs`/`contract_members.rs`, escribe `crates/varn-builtins/contracts.json` (114KB, versionado) con `{version, files: {path: xxh3}, contracts: {path: {classes, functions}}}`. Tipos como códigos (`int/float/bool/char/str/array/dynamic/void/opt(x)`). `--check` para CI.
- Macro lee JSON (`contract/tables.rs`, vía `CARGO_MANIFEST_DIR` del consumidor) en vez de parsear. Mismos mensajes de error para clase/funciones ausentes. `include_bytes!` del `.vn` intacto. Contratos inline (cero usos): error accionable.
- `builtins/build.rs`: gate duro — verifica xxh3 de cada `.vn` contra JSON y `panic!` accionable si stale. Imposible deriva silenciosa.
- `verify.ps1/verify.sh`: `gen-contract-tables --check` siempre; `std-bundle` + `VARN_STD_BUNDLE` en cuadrantes `@embedded`.
- Verificado: `check -p varn-contract/builtins/workspace` verdes. Gate negativo probado (newline en `int.vn` → `contract tables stale`). Tras `touch varn-core`, `check -p varn-contract` en 0.15s sin recompilar: desacople probado.
- Validado runtime 2026-10-08 con glue JSON: `check tests/main.vn` ok; `run` dev-checkout+JIT 2290/0; `@embedded`+JIT 2290/0; `VARN_NO_JIT=1` 2290/0; `std-bundle` regenera 2.730.418 bytes idéntico.

## 13. Fase 4 aplicada — grafo mínimo por defecto + fusión micro-crates

- `tokio`: `full` → `macros/rt-multi-thread/net/io-util/io-std/sync/time`. `full` venía solo de `varn-lsp` (`cargo tree -i tokio`); `tower-lsp-f` no exige features. Sin `fs/process/signal`.
- `default-members`: 22 crates. Fuera del default `xtask`, `varn-shadow`, `varn-rt` (cero tests en los tres, siguen con `-p`). `cargo build/check/test` pelado ya no los toca.
- `varn-cli`: features `dap`/`pm` (default-on, conducta idéntica). Loop light `--no-default-features`: 187 vs 318 crates (-41%). Borrado `commands/inspect.rs` muerto (fuera de `mod.rs`).
- Fusiones (menos crates, cero nuevas aristas): `varn-fmt→varn-core::fmt` (2 ficheros), `varn-debug-flags→varn-core::debug_flags` (11 ficheros; a `debug` no: `pipeline` habría pagado `checker/compiler/jit`), `varn-abi→varn-core::abi` (5 ficheros; a `types` no: `checker` habría ganado arista a `types` e invalidación frecuente), `varn-runtime→varn-builtins::runtime` (5 ficheros; a `vm` no: ciclo `vm↔builtins`). 26→22 miembros.
- Contrapeso honesto: fusionar en `core` agranda su unidad (tocar `fmt/flags/abi` invalida `core` entero). Se acepta: cambian raramente; el build limpio pierde 4 unidades.
- Verificado: `check --workspace --all-targets` cero errores y cero `unused_crate_dependencies`.
- Validado runtime 2026-10-08 tras fusiones: `quick --bin vn` 1m26s; `doctor` ok; `gen-contract-tables --check` ok; `run` dev-checkout 2290/0 (un 2293 aislado no reprodujo: flake de timing); `@embedded` exigió regenerar `dist/std.vnb` (fingerprint protege contra bundle stale: correcto por diseño) y dio 2290/0, bytes idénticos 2.730.418.

## 14. `varn-checker`: partido en 3 etapas (ver §§15-17)

- Grafo interno medido: `types/` hoja pura, ciclo `binder↔module_resolver` a nivel módulo, `lsp` con `CheckerTyId` directo.
- La trampa Ley 2 se disolvió al medir: ids por contenido, no por índice (§15). Partición viable sin interfaz portable previa.

## 15. Fase 5 aplicada — `varn-sem`: capa semántica fuera del monolito

- Hallazgo que reencuadra Ley 2: `CheckerTyId` es xxh3-128 de contenido (`types/interned/hash.rs:14-28`) y `Atom` es xxh3-128 del texto (`core/atom.rs:20-22`). Ids deterministas entre tablas/sesiones (test `ids_are_order_independent_across_tables`). Los síntomas históricos (unión mal leída, `Dynamic` fantasma) están fijados por diseño; quedaba el monolito + ciclo a nivel módulo.
- Nuevo crate `varn-sem` (3.291 LOC, deps `core/serde/postcard/rustc-hash/xxhash`): `types/` íntegro, `scope`, `symbol`, `semantic_info`, `bind` (ex-`binder/types.rs`: `BindResult`+datos; más `declares_type/enum_layout` movidos por regla huérfana), `exports` (`ExportMap`+`assign_slots`), `portable` (codec `PortableModule`), `codec` (ser/de), `cores` (structs `CoreMembers/Exports`), `resolver` (trait `ImportResolver`).
- `checker` 35.902→32.401 LOC. Sin puentes: cero re-exports; ~200 usos re-apuntados (`crate::` + `lsp/pipeline/cli/debug/tests`). `postcard/xxhash` fuera de `checker` (warning `unused` → deps borradas).
- Grafo resultante: `binder→ImportResolver` ya era trait; falta etapa 2 (trait `ModuleBinder` + crates `varn-binder/varn-resolver`) que requiere re-cablear `Session`/constructores: ciclo `binder↔module_resolver` medido a nivel módulo, no introducible como crates hoy sin romper.
- `debug_binder` (dev-tool roto preexistente: aridad `bind` + API `Symbol`) reparado a API actual.

## 16. Fase 6 aplicada — split `binder`/`resolver` SIN trait (ciclo ya muerto)

- Decisivo: el ciclo murió en fase 5 al bajar `ImportResolver` a `sem`. `binder/` solo necesita `sem+core+modules`; `Binder::bind` ya tomaba `&dyn ImportResolver`. Trait `ModuleBinder` innecesario (YAGNI): DAG `resolver→binder→sem`, una dirección. Sin re-cablear `Session` (firmas intactas).
- `varn-binder` (6.851): `binder/` aplanado + `core/` + `paths.rs`; `compat/` entero a `sem::compat` (era puro `sem`; `keyed_access` lo usaba: dependencia oculta `binder→checker` eliminada). `type_inference` público para 3 fns usadas por `checker`.
- `varn-resolver` (941): resto `module_resolver/`.
- `varn-checker` 32.401→23.177 (−35% desde 35.902). Sin `lexer/parser/parking_lot` (warnings→fuera); `resolver` solo en dev-deps (tests de caché movidos a `resolver/tests`, su dominio).
- `rustc-hash` opcional tras `clif` en `jit`. `binder/resolver` opcionales tras `dev-tools`/`lsp` en `cli`.
- `target/` extra: de entorno, no del workspace. No se toca.
- `check --workspace --all-targets` cero errores y cero `unused`; `cargo fmt` limpio.
- Validado runtime 2026-10-08 tras split: `quick --bin vn` 1m25s; `run` dev-checkout 2290/0, `@embedded` 2290/0 (bundle regenerado, bytes idénticos), `VARN_NO_JIT=1` 2290/0.

## 11. Fase 2 aplicada — Cranelift opcional (`clif`)

- `varn-jit/Cargo.toml`: deps `cranelift-*` opcionales tras feature `clif` (default vacío). Sin `clif`, `varn-jit` es ABI ligera: `JitHelpers/JitFn/layouts/stats/stack_roots/loop_hoist/mem` sin código nativo.
- `varn-jit/src/lib.rs`: `mod clif` / `mod aot` / `OwnedTargetIsa` con stub sin `clif` (`enabled=false`, `shared_isa/host_isa=Err` accionable, `ClifLinker/NoLinker/gate_reason` tamaño, `debug::inspect` ruta `Err`). `compile()` sin `clif` cae a `Err` → intérprete. `vm/clif_link.rs` intacto: implementa trait stub.
- Features: `varn-vm/clif`, `varn-debug/clif` reenvían a `varn-jit/clif`. `varn-cli` default `["lsp","clif"]`, `clif` los habilita. Conducta default idéntica (JIT activo). `--no-default-features` salta Tokio+LSP+Cranelift.
- Verificado: `check -p varn-jit` (light, 7 warnings dead-code), `--features clif`, `-p varn-cli` default/`embed-std`/`--no-default-features` verdes. `quick --no-default-features` en 1m17s. Light: `doctor` ok, `run 01-arithmetic` ok en intérprete, `debug --phase tiers` vacío sin error.
- Queda: versionar `dist/std.vnb` en CI dist + job `--check` bundle fresco; resto pecados (globals, `DefaultHasher`, `contract`, `jit optional`, `lsp default`, split `checker`) pendientes fase 2.
