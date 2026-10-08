# Plan desmezcla fases 9–12 (auditoría solo-código 2026-10-08)

Estado medido: `vm` 25780, `checker` 14415, `lsp` 14163, `compiler` 14014 LOC.
DAG limpio: `sem` hoja (solo `core`); `binder(sem+modules)`;
`checker(sem+binder+modules)`, `emit(sem+tir+modules)`;
`checker→resolver/emit` no existe (usa trait). Cero ciclos.
`lsp` delega 100% (cero inferencia propia). Lo de abajo, verificado
archivo:línea, es lo que queda mezclado.

Cierra `2026-10-08-plan-proyecto-real.md:132`
(`Resto >400 fuera de este workstream`).

## Fase 9 — `vm/error.rs` + barrido std único (pequeña, primero)

1. `error.rs:57-81`: `Display` hace `fs::read_to_string(top.file)`.
   Fijar Ley 1: capturar `src_line: Option<Arc<str>>` en el frame al crear
   el error (el loader ya tiene el módulo). `Display` puro, cero `fs`.
   Spike previo: contar sitios que construyen `Frame`; si salen de 1–2
   constructores, directo; si no, por módulo.
2. Barrido std x4 idéntico (`class_hierarchy.rs:50`, `member_exists.rs:56`,
   `member_named.rs:193`, `member_util.rs:29` + `is_known_module` en
   `member_named.rs:47`, `member_exists.rs:151`): un solo
   `ImportResolver::find_in_stdlib(name, check)` como método default en
   `sem/resolver.rs:27-44` (sin dep nueva: la enumeración la aporta el
   implementor vía `each_stdlib_bind`; único implementor hoy
   `resolver_trait.rs:9` `DiskResolver`, que ya depende de `modules`).
   Borrar los 4 loops. `is_known_module` (`member_named.rs:47`,
   `member_exists.rs:151`) se queda: clasifica stdlib-vs-archivo para
   `*`-imports, no conoce inventario.
3. Regla: `sem` sigue sin depender de `modules`; `checker` deja de
   conocer el inventario std.

## Fase 10 — `vm/exec/props/mod.rs:17-606` por dominio (mecánica)

Partir (cada uno invariante propio, nada compartido nuevo):
- `get_set.rs`: `get_property/get_property_maybe/set_property:121-156`.
- `intrinsic.rs`: `resolve_own_data/class_for_property/resolve_intrinsic_method:233-326` + `get_class/bind_method:533-583`.
- `specialized.rs`: `resolve_specialized_property:376-443` (Str/Array/Buffer/Map).
- `enum_payload.rs`: `enum_variant_property:445-519`.
- `generator_next:521-531` fuera: mover al módulo del driver de
  generadores (auditar destino en sesión), no queda en props.
- `meta.rs` (354 líneas) NO se parte: un solo dominio (meta
  reflection); las 4 nativas solo se registran ahí (`meta.rs:201-245`).
  Criterio manda: 400 dispara revisión, partir solo si >1 dominio.
- Criterio general (vale para `parser/decls/class.rs:637`,
  `types/disasm.rs:640`): tamaño ≠ mezcla; 400 dispara revisión,
  se parte solo si hay >1 dominio. Pases de compiler (`const_fold:493`,
  `fixed_fields:560`) son un dominio: no se tocan.

## Fase 11 — contrato `jit↔vm` explícito (rompe ciclo encubierto)

Hecho: `jit/lib.rs:320` genera su tabla escrutando
`"../varn-vm/src/exec/jit_helpers"` por path literal, y
`vm/lib.rs:42` re-exporta `varn_jit`. Añadir `jit→vm` en Cargo
sería ciclo real (hoy `vm→jit` por `clif_link.rs:6`).
Fijar Ley 1/8 con tercer crate hoja `varn-jit-abi` (solo `core+types`):
tabla de descriptores + layouts (`JitArrayLayout`, `JitHelpers`, …,
hoy en `jit/lib.rs:146-320`) viven ahí; `vm/jit_helpers` (22 ficheros,
~2587 LOC) implementa contra ella; `jit` baja código contra ella.
Borrar el path literal y el re-export. Lo demás de `jit` no se mueve.

## Fase 12 — `Checker` deja de ser god-object (riesgo alto, última)

Hecho: `checker/mod.rs:38-89`, 46 campos (binding+contexto+flujo+IDE+
salida+9 cachés+profiler); `check_internal:117-301` orquesta
bind+merge+enrich+check+foreign+finalize en ~185 líneas;
`check/mod.rs:20-60` chequea e inserta `expr_table` a la vez.
`narrowing/` 0 funciones libres: todo es `&mut self`.

1. E1 perfil fuera: `Instant/Duration` (`mod.rs:21,124-282`) a
   `checker/profile.rs` con guard RAII en los 7 sitios. Datos
   (`CheckProfile`) se quedan en `sem::output`.
2. E2 `check_internal` en etapas con I/O explícita:
   `prepare()→(BindResult)`, `run_pass()`, `collect_foreign()`,
   `finalize()`; cada una recibe lo que usa, nada de `&mut self`
   transversal nuevo.
3. E3 `Recorder` separado: `expr_table/expr_types/node_scopes/
   scope_spans/symbol_types/member+call_resolutions/match_gaps/
   desugar/call_mappings` salen del struct a `Recorder` dueño único;
   métodos reciben `rec: &mut Recorder` explícito (~60 ficheros,
   mecánico, disjuntos para borrowck). Cachés se quedan: memo de
   sesión, no el pecado.
4. Prohibido puentear con flags duales o `Deref` al struct viejo
   (Ley 8): cada paso es el diseño final.

## Validación obligatoria por fase (no negociable)

1. `cargo check --workspace --all-targets` cero errores y cero `unused_*`.
2. `cargo fmt --all` (renames por `edit`, nunca `` `n `` en PowerShell).
3. `cargo build --profile quick --bin vn` + `doctor` + `gen-contract-tables --check`.
4. Suite 2290/0 `run tests/main.vn`: fase 9 siempre 4 cuadrantes
   (dev-checkout/`@embedded` × JIT/`VARN_NO_JIT=1`); fases 10–11
   cuadrante 1 + matriz completa antes de commit; fase 12 (toca
   `checker`, fingerprint de `dist/std.vnb`) regenerar bundle y 4/4
   como en fase 7.
5. `verify.ps1 -Quick` al cierre de cada fase; `-Fast`/release antes
   de cada commit. Sin `git` sin autorización. Un commit por fase
   verde (Ley 9); E1/E2/E3 cada uno el suyo.

## Riesgos conocidos

- E3 toca ~60 ficheros: hacerlo último, sobre árbol verde, en una
  sola pasada mecánica; cualquier `pub(crate)` que escape lo canta
  el check.
- Fase 11 cambia el contrato JIT: validar cuadrantes JIT + No-JIT
  (un helper caído solo falla en JIT).
- `Display` sin `fs` cambia output si el fichero cambió en disco
  tras el raise: es lo correcto (foto al raise, Ley 6).
- No bloquear por `vm` 25k/`compiler` 14k restantes: son grandes
  pero de un dominio; fuera de alcance tras fase 10.
