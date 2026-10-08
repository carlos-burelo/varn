# Plan proyecto real — Varn evoluciona (2026-10-08)

Origen: auditoría código + ejecución 2026-10-08. Cifras propias, no copiadas de `.md`.
Si algo contradice al código, el código manda: parar, reportar archivo:línea, preguntar.

Línea base medida:
- 24 crates workspace, 1111 archivos rs, 130679 LOC.
- Gigantes: checker 31850/248f, vm 21271/174f, lsp 12418/110f, compiler 11251/91f = 59%.
- Gobierno tamaño OK: 4 archivos >400, max 483 `varn-vm/src/exec/jit_helpers/intrinsics.rs`, cero >1000.
- Suite canónica: `vn test` 155 passed, 0 failed, 153 suites, 4.4s (`target/debug/vn.exe`).

Reglas operación: Ley 8 absolutista (un mecanismo canónico, borrar caminos viejos, sin adapters/flags duales),
Ley 9 commits atómicos, Ley 11 cero comentarios código + cero `#[test]` (regresiones `.vn` en `tests/`),
Ley 13 perfil `quick` para iterar (`cargo build --profile quick --bin vn`), matriz completa solo antes commit
(`verify.ps1 -Fast`, release). Paso verde = check + fmt + build + `vn test` + puertas propias fase.

---

## F0 — Higiene [ ]

- [x] Borrar comentario desync `crates/varn-compiler/Cargo.toml:8` (`varn-regalloc` no existe, código `src/lib.rs:7` `pub mod regalloc`)
- [x] Renombrar 7 genéricos por dominio (`50bd6143`):
  - [ ] `varn-checker/src/checker/compat/helpers.rs`
  - [ ] `varn-checker/src/checker_expressions/helpers.rs`
  - [ ] `varn-parser/src/expressions/helpers.rs`
  - [ ] `varn-vm/src/jit/helpers.rs`
  - [ ] `varn-cli/src/bench/source/helpers.rs`
  - [ ] `varn-jit/src/clif/from_ssa/extra/common.rs`
  - [ ] `varn-tir/tests/verify_coherence/common.rs`
- [x] Migrar 36 `std::collections` → Fx/BTree (`d3728072` + `a1879997` absoluto: cero std en `lsp/src`, wire via `document_changes` ordenado por URI; quedan solo tests, xtask, BTree/VecDeque ordenados)
- [x] Auditar 318 `_ =>` (Ley 7, cada brazo explícito ante variante nueva) → 586 brazos expandidos + lint `wildcard_enum_match_arm` permanente en cero (`e31475c6` lint, `939fd34e` compiler, `93555c5f` checker emit+binder, `d118d220` checker core, `42d7dd0c` runtime, `bf171c00` tooling). Externos `non_exhaustive` (io::ErrorKind, syn) no los flaggea el lint: se dejan.
- [x] Puerta F0: `vn test` 155/0

Deuda preexistente detectada (no de este plan, no tocar aquí): `design_audit_test::h7_small_types_are_copy_sized` falla en HEAD limpio
(`size_of::<Atom>()` 16 vs 4 esperado). Toca F2 (identidad portable / NameId), no F0.

## F1 — ABI hoja, rompe checker→builtins [x]

Causa: `checker/emit/body/scope.rs:216` contra orden owned por `builtins/dispatch/modules.rs:96,132`.
(`1a1bcaa0`: const `NATIVE_GLOBALS` en `varn-abi`, checker→abi, borradas funciones builtins,
fail-fast en arranque VM si falta global, checker compila aislado `cargo check -p varn-checker`.)

Causa: `checker/src/emit/body/scope.rs:216` `varn_builtins::native_global_index` contra orden owned por `builtins/src/dispatch/modules.rs:96,132`.
Canónico: layout en `varn-abi/src/lib.rs` (hoy 151 LOC, solo→core). Sin `OnceLock`, sin `all_native_ops`.

- [ ] Añadir tabla estática + `index()` en `varn-abi`
- [ ] Checker + vm consumen `varn-abi`
- [ ] Borrar `native_global_layout/index` de `varn-builtins` (sin re-export shim)
- [ ] Puerta: `vn debug -p bytecode` idéntico + `vn test`

## F2 — Loader único, núcleo [ ]

- [x] `PipelineLoader` único sobre registry canónico (`5342032c`): File/Std fuera, caché instancia, `VmFactory` contra trait
- [x] Sesión explícita en pipeline (`e607ec59`): `Session::new()` por run, fuera `thread_local`/`with_resolver`/`reset`; `DiskResolver::with_registry`
- [ ] Workspace LSP dueño del resolver (fuera global `OnceLock`)
- [ ] Una sola función compila-módulo en pipeline (dedup `compile_source_inner`)
- [ ] Puerta: `vn test` 155/0 (verde) + pipeline tests (verde salvo preexistentes abajo)

Deuda preexistente (HEAD limpio, no tocar aquí): `bytecode_layout_agrees` pánico Atom interner; `h7` tamaño Atom.

Causa: 3 traits mismo hecho: `modules/loader.rs:94` `ModuleLoader: Send+Sync resolve+source` (canónico, existe)
vs `vm/loader.rs:22` `resolve+load→Proto` vs `checker/module_resolver/resolver_trait.rs:9` `ImportResolver for DiskResolver`
(`resolver_disk.rs:5`). Más `pipeline/resolver.rs:4` `thread_local! RESOLVER`, `stdlib_loader.rs:12` `PROTO_CACHE`,
`:18` `COMPILED_BYTES`, `:151` `compile_source_inner` compila dentro orquestador.

- [ ] `Checker::check` recibe `&dyn modules::loader::ModuleLoader`, borra `ImportResolver` propio
- [ ] Mover `DiskResolver` a pipeline/cli como wrapper sobre `ModuleRegistry`, borrar `checker/module_resolver/resolver_disk.rs,resolver_embed.rs,resolver_access.rs,cache/cache_io.rs:34`
- [ ] Borrar `vm/loader.rs:22,27` trait+`Composite`, `FileLoader/StdlibLoader` (`pipeline/stdlib_loader.rs:61,98`) pasan a funciones sobre canónico
- [ ] Eliminar `pipeline/resolver.rs:4` + `stdlib_loader.rs:12` thread_locals, dueño único presta por parámetro
- [ ] Pipeline solo orquesta lexer→parser→checker→compiler→vm, no emite
- [ ] Puerta: `VARN_CACHE_DIR=<temp> vn test` reproduce, `vn run -v` motivo miss intacto

## F3 — Pipeline→debug [ ]

Causa: `pipeline/compile.rs:8,58,63,67,71,77,80,85,97,101,105,109` 20+ `varn_debug::`.
Canónico: pipeline retorna `PipelineOutput` (proto+program+graph+helpers), CLI/debug renderiza.

- [ ] Añadir struct `PipelineOutput`
- [ ] Migrar call sites por grupo (bytecode, clif, typeloss, summary, tiers, tir, caps, binds, consts, scopes)
- [ ] Borrar dep `pipeline→varn-debug` en `Cargo.toml`
- [ ] Puerta: `vn debug -p all,bytecode,clif,tir,gc` idéntico

## F4 — Runtime→frontend [ ]

Causa: `vm/Cargo.toml:14,11` → builtins → `op-macros/Cargo.toml:18,19` → lexer+parser. Compilar VM compila frontend.
Canónico: vm nunca toca sintaxis. Split `builtins-rt` (impl, solo abi+types+runtime) vs `builtins-def` (tablas+`varn_contract!`, único que ve parser).

- [ ] Medir `cargo tree -p varn-vm`
- [ ] Extraer tabla/registro abajo
- [ ] Mini-parser firmas en core o pre-gen `build.rs`, `varn-op-macros` deja de ver parser en ruta runtime
- [ ] Puerta: `cargo build -p varn-vm` sin compilar parser + `vn test`

## F5 — Robustez [ ]

- [ ] `unwrap 218 + expect 230`: focos `builtins/fs/fs.rs:59,65,68` lock, `modules:25`, `vm:59`, `checker:35` → `DiagnosticBag`/error tipado, Ley5
- [ ] `unsafe 377` (vm 279+types 44): inventario perímetro heap/arch/JIT, cada uno justificado eje runtime Ley10, resto borrar
- [ ] `thread_local 13` restantes (`modules/std_root.rs:102`, `compiler/from_tir/compile.rs:15`, `regalloc_post/mod.rs:20`, `vm/pumps.rs:23,clif_link.rs:11,alloc_profile.rs:34`, `types/shape.rs:116,class.rs:11`, `pipeline/stdlib_loader.rs`, `jit/clif/mod.rs:56,emit/payload.rs:16`, `vm/ctx_json/mod.rs:13`): dueño explícito o borrado
- [ ] No partir gigantes por tamaño (techo 400 cumple). Formalizar `checker/{binder,types,checker,emit,module_resolver}` → crates solo tras F2
- [ ] Puerta final: `verify.ps1 -Fast` 4 cuadrantes + `vn test` 155/0

---

Orden: F0 → F1 → F2 → F3 → F4 → F5. Si un paso pide adapter/flag dual, parar: modelo mal, simplificar Ley8.
Estado se actualiza marcando `[x]` arriba conforme avanza cada paso verde.
