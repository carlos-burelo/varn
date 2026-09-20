# Varn — Plan pendiente (JIT nativo, motor único)

Fecha: 2026-09-20. Estado: `main` consolidado con **trabajo WIP mezclado
(roto)** a propósito, para reiniciar con contexto limpio y sin ramas.

> **Progreso (misma fecha, sesión de implementación):** B0 + B1/C1 cerrados.
> `main` compila, `cargo check` limpio, `tests/main.vn` **verde en ambas
> tiers** (`PASSED: 1223, FAILED: 0`) con `Ref`+`Dyn` activos y el gate
> `VARN_JIT_ALLOW_REF` **eliminado**. Ver §7 para los defects concretos
> corregidos.

Prioridad de verdad: código > tests > comportamiento > docs.

---

## 0. Estado real de `main`

**Funciona (verde antes del merge `jit-pair-ref`):**
- Fase B del frame por clases: JIT reactivado (`FRAME_LAYOUT_V2_JIT_BAIL=false`),
  subset gate, backstop de helpers deshabilitados.
- **Homes inline** (`home_addr` = `class_vec_ptr + (base[class]+idx)*elem`),
  sin FFI por acceso a home.
- Llamada directa compiled→compiled (`jit_prepare_static_call`/
  `jit_finish_static_call` sobre `FrameStore`).
- Fix de layout compacto: campos de instancia y `Get/SetProperty` por helper
  compact-aware; loop-cache de objeto desactivado.
- Excepciones, `Intrinsic`, `CallNativeOp`, `CallMethod`, `InvokeVirtual`,
  `MakeClosure`, `CallSelf`.
- Limpieza de dead code / warnings rustc.
- `Ref` **gated** por defecto (escape dev `VARN_JIT_ALLOW_REF`).

**Roto ahora (por el merge del par):**
- La rama `jit-pair-ref` (par `Ref`/`Dyn` tag+payload) se mergeó **sin estar
  verde**: markdown falla (`expected str, got heap[46]`), y el intento de C4
  (neutralizar la lattice) se revirtió, así que ese frente no está en `main`.
- `main` compila? VERIFICAR primero; el merge tomó la versión del par en los
  ficheros de `varn-jit`, con las trazas y cambios de `safepoints`/`vars`/
  `op_dispatch`/`emit`/`lower` del par.

**Ramas:** solo quedan ajenas (`tir-stage-3-the-cut`, `worktree-*`). Las de esta
sesión (`jit-fase-b`, `jit-typed-model`, `jit-pair-ref`) ya no existen.

**Trazas disponibles (env-gated):** `VARN_HOME_TRACE` →
`HOME`, `DEFRESULT`, `OPIP`, `PREPCALL`, `INVOKE_NATIVE`, `METHODNATIVE`,
`NATIVEOP`, `INTRINSIC`, `STR BAD`. `VARN_JIT_ALLOW_REF` para bajar `Ref`.

---

## 1. Objetivo

**Máxima superficie en código nativo, una sola ruta por hecho, sin fronteras que
se crucen repetidamente.** Intérprete y CLIF como dos bajadas hermanas del
**mismo SSA tipado**. Nada interno de una fase/ruta cruza a otra (nombres y
sintaxis + clase física, no ids ni tags ambiguos).

Medible: `tests/benchmarks/compare.ps1` — hoy Varn gana 1/12 (arranque + csv_etl).
Meta: ganar a Bun en todo. `matrix` ya se midió **12 ms vs Bun 15.4** en la rama
par (aunque luego falla markdown).

---

## 2. Modelo objetivo (diseño A)

```
TIR (BackendTy por valor, obligatorio)
  → SSA tipada (una)
  → intérprete (FrameStore)  y  CLIF   ← MISMO contrato
```
- Registro: escalar `i64`/`f64` para `Gpr`/`Fpr`; **par tag+payload** para todo
  heap (`Ref` y `Dyn`). Una representación.
- Llamada: una convención (slots por clase); el intérprete y el JIT comparten
  materialización y copia de args.
- Nativas/intrínsecos: **una** tabla ABI + **un** marshal; `this` por el mismo
  camino que los args.
- JIT sin lattice de tipos: el tipo viaja en el IR.

Detalle: `docs/plans/2026-09-20-jit-modelo-tipado-unico.md`.

---

## 3. Pendiente, por bloques (orden de ejecución)

### B0 — Sanear `main` (primero)
- Verificar `cargo build -p varn-jit -p varn-vm` y `cargo check --workspace
  --exclude varn-lsp --all-targets` (0 errores; el merge pudo dejar usos rotos).
- Correr `tests/main.vn` **con `Ref` gated**; si falla por el par mergeado,
  decidir: (a) completar C1, o (b) volver `vars.rs`/`safepoints` a payload-only
  (revertir el par) hasta cerrar C1. Objetivo: `main` verde con `Ref` gated o
  con el par completo — no a medias.

### B1 — C1: par de valor para todo heap (`Ref`+`Dyn`)
Sitios que aún asumen "Dyn/Ref = una palabra (payload)" y hay que migrar a par
(I128). Cada uno: leer con `box_or_load_home` (par), escribir con `def_result`
(o `box_*` + `def_var` I128), y actualizar el home.
- `clif/body/op_dispatch.rs`: aritmética (`AddInt`/`SubInt`/`MulInt`/`ModInt`/
  `Negate`/bitwise/shifts) cuando el dest es clase `Dyn`; `AddImm`/`SubImm`;
  `CallSelf` ramas de dest; `Return`.
- `clif/generic.rs`: `def_boxed`/`box_operand` con par; comparaciones.
- `clif/emit.rs`: `use_int`/`use_f64` sobre registro par (extraer payload);
  `def_const*`; `box_or_pass`.
- `clif/floats.rs`, `clif/strings.rs`, `clif/strconcat.rs`, `clif/methods.rs`,
  `clif/arrays.rs`, `clif/globals.rs`, `clif/fields.rs`, `clif/alloc/*`:
  dest/lectura por clase.
- **Bug activo**: en `tests/36`/markdown, `str.split` recibe un **Instance**
  (`heap[46]`) como `this` (native `split` en el punto único `invoke_native`).
  Un registro `Dynamic` (`reg1` de `parse`, `source`) se **clobbea** antes del
  `split`. El intérprete funciona → es value-flow del JIT.
  - Hipótesis: `Move`/`def_result` con par escribe/lee el registro equivocado, o
    liveness/regalloc reusa un registro vivo.
  - Traza: `OPIP`/`PREPCALL`/`INVOKE_NATIVE` con `VARN_HOME_TRACE`.
- Al cerrar: quitar `VARN_JIT_ALLOW_REF` y el baile de `Ref` en `clif/lower.rs`
  (`uses_disabled_opcode`/gate). Suite verde con `Ref`+`Dyn` activos.

### B2 — C2: una convención de llamada (VM)
- Introducir `ExecCtx::invoke(callee, args_window)` y hacer que:
  - el intérprete (`exec_call_reg`, `exec_call_method_reg`) y
  - el JIT (`clif_call_fallback` → `call_vm_window`) y
  - las nativas (staging `[this, args...]`)
  usen esa **misma** materialización.
- **Borrar**: `clif_call_fallback`/`call_vm_window` como caminos distintos;
  `jit_call_native_fast`; ventanas contiguas duplicadas.
- Archivos: `exec/jit_helpers/calls.rs`, `exec/calls.rs`,
  `exec/dispatch/reg_ops/{calls,method_calls}.rs`, `clif/alloc/calls.rs`.

### B3 — C3: un ABI nativo y un marshal
- `CallNativeOp`, métodos nativos y `CallMethod`-nativo entran por una función
  con la **misma** extracción de `[this, args...]` y **un** `FromVm`.
- **Borrar**: `str_*_intrinsic` dedicados como ruta propia; `jit_call_native_op`
  vs `fnptr` duplicados; el marshal del `this` por camino aparte.
- Archivos: `exec/jit_helpers/{natives,intrinsics}.rs`,
  `exec/intrinsics/str.rs`, `varn-types/src/marshal.rs`, `clif/body/*`,
  `clif/alloc/native.rs`, `clif/methods.rs`.

### B4 — C4: bajar el JIT de SSA/TIR (el grande)
- Consumir SSA/TIR tipado (o serializar **clase/tipo** en `.vnc`, no
  bytecode+lattice).
- **Borrar**: `clif/kinds.rs` (lattice `K`), `state`/`box_for_target`,
  `box_or_load_home` heurístico, re-derivación por op.
- Sub-paso ya intentado y **revertido**: neutralizar la lattice (`state` =
  proyección de clase) — rompe N sitios de B1 aún sin migrar. Hacer B1 primero.
- Ver `docs/plans/2026-09-20-jit-modelo-tipado-unico.md` (C4).

### B5 — C5: `Ref`+`Dyn` globales y flip
- Quitar `VARN_JIT_ALLOW_REF` y el gate de `Ref`.
- Re-correr `compare.ps1` y la matriz de validación.

### B6 — Cobertura de opcodes aún gated / nativas
- `CallSpread`, `MakeClass`/clases, `InvokeRuntimeStatic`: implementar nativo.
- generator/async: suspensión/reanudación por clases.
- `Intrinsic`/`CallNativeOp` calientes (str/array): inline en CLIF o vía el ABI
  único (B3), sin rutas dedicadas.

### B7 — `str_ops` (benchmark 33x más lento)
- Especializar concat/repeat/slice/indexOf/replaceAll (hoy ruta genérica).
- Medir con `compare.ps1 -Only str_ops`.

### B8 — Pendientes del audit (menores, documentados)
- `u64`: aritmética sin signo (`BackendTy::UInt64`); hoy `TypeTag::U64`→`Int`.
- `ArrayRepr` angosto **escritura** (`set_vm`/`push_vm` migran a `Boxed`).
- Limpiar comentarios NaN-box legacy (`vm_value.rs:475`, `object.rs`, etc.).
- Regenerar `docs/AUDIT_RESPONSE.md` con el estado real.

---

## 4. Validación (cada commit)

1. Compilar primero: `cargo build -p varn-jit -p varn-vm`, `cargo check
   --workspace --exclude varn-lsp --all-targets` (0 warnings).
2. Ejecutar el `.exe` **directo** con timeout (no `cargo run` encadenado):
   `.\target\debug\vn.exe run .\tests\main.vn` (da PASSED/FAILED).
3. Tier-parity: `56-tier-parity`, `58-clif-range`, `62-jit-osr`,
   `65-safepoint-roots`, `101-self-recursion-return-kind`.
4. Benchmarks: `tests/benchmarks/compare.ps1 -Only matrix,str_ops,...`.
5. Matriz release (4 cuadrantes std/JIT) antes de declarar terminado.

---

## 5. Ramas

- **`main`**: único destino. Trabajar aquí o en **una** rama, nunca varias.
- Ajenas existentes: `tir-stage-3-the-cut`, `worktree-*` (atadas a worktrees;
  no tocar sin dueño).

---

## 6. Riesgos

- B1 y B4 se tocan: el par de valor (B1) es prerrequisito de neutralizar la
  lattice (B4). No hacer B4 antes de B1.
- B2/B3 tocan el intérprete: mantener `VARN_NO_JIT=1` verde.
- El crash de markdown (`str.split` con `this` Instance) es el bug que bloquea
  `Ref` global; localizado con `INVOKE_NATIVE` (`VARN_HOME_TRACE`).
- `main` está roto por el merge del par; B0 lo resuelve (completar B1 o revertir
  el par).

---

## 7. Progreso B0+B1/C1 (cerrado)

Rama de trabajo: local (sin commits nuevos; cambios en el árbol).

Defects concretos corregidos, todos consecuencia de que un valor de KIND
`Bool`/`Int` puede vivir en un registro de clase física `Dyn`/`Ref`
(`I128` par) y los consumidores lo leían como una sola palabra:

1. `clif/body/mod.rs` — `JumpIfFalse`/`JumpIfTrue`: una condición `K::Bool`
   con clase `Dyn` es un par; `brif` sobre el par es siempre verdadero (bucle
   infinito en `tests/09`). Ahora extrae el payload (`isplit`).
2. `clif/floats.rs` — comparaciones `*Float` y `Intrinsic*` math:
   el resultado bool se escribe con `box_bool` + `def_result`/`def_boxed_leaf`
   cuando el destino es heap-classed, en vez de un `I64` crudo.
3. `clif/emit.rs` — helpers nuevos `def_boxed_leaf`, `def_int_result`,
   `def_bool_result`: una sola proyección destino-clase para resultados
   crudos (Ley 6).
4. `clif/body/op_dispatch.rs` — aritmética (`AddInt`/`SubInt`/`MulInt`,
   `AddImm`/`SubImm` en su fast-path de inducción, `ModInt`, `Negate`),
   antes escribían `I64` crudo en el par.
5. `clif/generic.rs` — `def_boxed`/comparaciones/`IsNull`/unario bool en la
   ruta leaf.
6. `clif/fields.rs` — `GetFixedField` ruta `narrow` (`Int`/`Bool`/`Float`):
   `Bool` es `Dyn` (par) y debe boxearse.
7. `clif/alloc/safepoints.rs` — **bug de layout**: `store_boxed_home` para la
   clase `Ref` escribía un `I64` (8 bytes) en un slot `u32` (4 bytes), pisando
   el slot vecino. Causaba `arr=heap[0]` en `ArrayPush` (`tests/42`). Ahora
   `istore32`.
8. `clif/lower.rs` — gate de `Ref` y `VARN_JIT_ALLOW_REF` eliminados; el
   `eprintln!("REFMETA …")` de diagnóstico también.

Pendiente de este bloque: nada. B2/B3 siguen sin tocar.

---

## 8. Progreso B2/C2 (cerrado)

Commits: `61e2f466` (C2).

- **Una invocación**: `ExecCtx::invoke(callee, window)` es la única ruta
  run-to-completion. `clif_call_fallback` (JIT), `NativeCtx::call_vm` (host),
  `spawn_internal` e isolates pasan por ella; `call_vm_window` eliminada y
  `call_vm` pasa a construir la ventana `[callee, args...]`.
- **Una materialización de frame tipada**: `ExecCtx::push_call_frame`
  (push de activación + `mov_cross` por clase). La usan el intérprete
  (`exec_call_reg`, `exec_call_self`) y el JIT (`jit_prepare_static_call`),
  en vez de repetir el bucle de copia.
- **Borrado**: `jit_call_native_fast` (tripwire fase-A sin llamador en el
  lowering) — fuera de la lista ABI, de la tabla y del cuerpo.

`tests/main.vn`: 1223/0 en JIT y `VARN_NO_JIT=1`.

No abordado en C2 (queda para C3/resto): `exec_call_method_reg` conserva su
materialización con receiver+owner_class; `invoke_vm_method_fast` no usa aún
`push_call_frame` (forma distinta: `this` en r0). Son candidatos de un
sub-paso si se busca C2 al 100%, pero no cambiaron de comportamiento.


