# Plan deuda restante fases 13–17 (auditoría solo-código 2026-10-08)

Cierra `2026-10-08-plan-proyecto-real.md:132`
(`Resto >400 fuera de este workstream`).
23 ficheros >400 (techo AGENTS §6; `verify.ps1` solo falla >1000:
alinear o justificar, fase 17).
DAG limpio, `lsp` delega, `checker` sin god-state global
(`Checker` 34 campos + `Recorder` 13). Todo abajo verificado
archivo:línea, suite 2290/0 x4 al cierre fase 12.

## Fase 13 — test-infra (primero: gate rojo intermitente)

1. Suite 112 (`tests/112-canonical-bytes-and-streams.vn:117`,
   helper `tests/helpers/tcp_binary_echo_server.vn:15`):
   `TcpListenError` puerto 19877 en `TIME_WAIT` tumba corrida.
   Fijar Ley 8 (un mecanismo): reintento con backoff en el helper
   o `SO_REUSEADDR` en runtime net; borrar camino que falle sin
   reintentar. Prohibido `sleep` fijo mágico sin medir.
2. Conteo suite fluctúa 2290/2293/2296 con 0 failed: root-cause
   antes de partir nada (¿suites condicionales a JIT/dev-checkout?
   contar con `--list` si existe, o diff de suites corridas).
3. Validación: 3 corridas seguidas Q1 verdes + 4 cuadrantes.

## Fase 14 — `vm` dispatch (25.7k, mayor crate)

Auditar en sesión (Ley 12) `exec/dispatch/` + `exec/advanced.rs:399`:
- `ops_math_cmp/arith_int.rs:543`, `jit_helpers/intrinsics.rs:481`
  (slow-paths ya son un dominio: confirmar, no partir por partir).
- Criterio fase 10: tamaño ≠ mezcla; partir solo >1 dominio
  (`props/mod.rs` fue el modelo).
- `vm/error.rs` ya sin `fs`; `generator_next` ya en driver.

## Fase 15 — `compiler` 14k + `lsp` 14.1k

- `compiler`: `passes/fixed_fields.rs:560`, `const_fold.rs:493`,
  `ssa/portable/inst/composite.rs:519`, `ssa/portable/mod.rs:466`,
  `ssa/emit/mod.rs:402`. Pases de un dominio no se tocan; auditar
  `fixed_fields` y `composite` (nombres huelen a 2 dominios).
- `lsp`: `features/completion/members.rs:501` (presentación ya
  delegada: partir formato vs cascada `dot_receiver`, no lógica).
- Riesgo: `lsp` toca `CheckResult` caliente; preferir `pub` y podar
  después (lección fase 7).

## Fase 16 — resto >400 uno por uno

- `parser`: `decls/class.rs:637`, `patterns.rs:567`,
  `decls/type_decls.rs:518`, `stmts/dispatch.rs:417`,
  `decls/modules.rs:411`.
- `types/disasm.rs:640`, `sem/compat/compat_lookup.rs:415`.
- `emit`: `body/stmt_decl.rs:499`, `namespaces.rs:484`,
  `functions.rs:447` (recién movido en fase 7: solo si >1 dominio).
- `checker`: `decorator_signature.rs:475`, `decl_class.rs:442`,
  `check/members.rs:444` (post-Recorder: re-auditar, quizá ya de
  un dominio).
- `cli/commands/contract_tables.rs:407` (+ sus 3 clippy, fase 17).

## Fase 17 — higiene y cierre

1. Clippy `wildcard_enum_match_arm` en `contract_tables.rs:238,264`:
   brazos explícitos (Ley 7).
2. `vm/lib.rs:42` `pub use varn_jit`: re-export puente que `cli/bench`
   usa (`varn_vm::varn_jit::`). Decidir dueño único o quitar.
3. Macro `fill` aún lee `fs` propio: evaluar lista explícita
   (120 helpers compiler-checked) vs scan; lo que quede debe ser
   el mecanismo único.
4. Gobernanza: alinear AGENTS §6 (400) con `verify.ps1` (700/1000)
   o justificar por escrito. Sin dos varas.
5. Cierre: `verify.ps1 -Fast` + release + 4 cuadrantes; commit por
   fase (Ley 9); marcar `:132` del plan proyecto-real `[x]`.

## Validación obligatoria por fase (no negociable)

1. `cargo check --workspace --all-targets` cero errores y cero `unused_*`.
2. `cargo fmt --all` (renames por `edit`, nunca `` `n `` en PowerShell).
3. `cargo build --profile quick --bin vn` + `doctor` + `gen-contract-tables --check`.
4. Suite `tests/main.vn` 2290/0 (o conteo root-caused fase 13):
   fases que tocan `checker` regeneran `dist/std.vnb` + 4/4;
   resto Q1 + matriz completa antes de commit.
5. `verify.ps1 -Quick` por fase; `-Fast`/release antes de commit.
   Sin `git` sin autorización. Sin comentarios código (Ley 11).

## Cierre fases 13–17 (2026-10-08, implementado)

- Fase 13: causa raíz real del gate rojo NO era `TIME_WAIT` (hipótesis `.md`,
  descartada por Ley 12): el driver declaraba éxito connects rehusados
  (`event_loop.rs`: `take_error()==Ok(None)` → connId bogus; más vía rápida
  síncrona `peer_addr().is_ok()` en `tcp.rs`). El cliente escribía sobre un
  socket muerto → RST → `TcpWriteError`. Fix Ley 8 (mecanismo único):
  completar connects solo en el event-loop verificado + reintento con backoff
  en `TcpStream.connect` (25 intentos) y en el helper `listenWithRetry`;
  `SO_REUSEADDR` vía `socket2` en `listen`/`udp_bind`; `catch/kill` anti-fuga
  en el test 112. Conteo root-caused: 2287/0 x3 + Q1 final 2287/0 estables
  (la fluctuación 2290/2293/2296 era la cascada abortos+ugas del mismo bug).
- Fase 14: `arith_int`/`intrinsics`/`advanced` auditados; `advanced` partido
  en `advanced_typechecks`/`symbol_iterator`/`method_bind`; `ctx_modules`
  extrae `module_freeze`. Resto 1 dominio (no partir por partir).
- Fase 15: `compiler` 5 ficheros 1 dominio (no se tocan); `lsp/members`
  partido en `members` (formato, 119) + `receivers` (cascada, 253) +
  `receiver_literals` (vocabulario, 174).
- Fase 16: 15 splits por movimiento puro (`params`, `enum_decl`, `block`,
  `import`/`export`, `class_member`/`class_key`/`class_mods`, `opcode_*` x3,
  `pattern_bind`, `extensions`, `free_functions`, `decorator_purity`,
  `decl_class_decorators`/`decl_class_members`, `member_index`/`members_assign`,
  `contract_classify`/`contract_members`, `compat_fn_sig`).
  Hallazgos extra de medición fiable: `cfg_dom`, ya partidos.
  1-dominio justificados sin partir: `decl.rs`/`op.rs` (vocabulario),
  `scalar.rs`/`aot` (dispatch único), `insight.rs` (fachada fina).
- Fase 17: clippy `wildcard_enum_match_arm` en `contract_tables` con brazos
  explícitos; `pub use varn_jit` de `varn-vm` borrado (dueño único `varn-jit`,
  `cli` ya dependía directo); macro `fill` queda scan (mecanismo único, lista
  explícita duplicaría hecho Ley 6); gobernanza alineada a techo 400
  (`verify.ps1/sh`, `ci.yml` avisan >400, fallan >1000; `CONTRIBUTING.md`).
