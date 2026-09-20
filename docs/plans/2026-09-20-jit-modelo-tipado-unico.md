# Diseño A — un motor de ejecución tipado (colapsar rutas nativas)

> Objetivo: máxima superficie en código nativo, **una sola ruta** por hecho.
> Ley 8: se corrige la causa raíz con un único mecanismo y se **borran** los
> caminos paralelos. Cada commit de abajo es atómico, verde, y declara qué
> elimina.

## Causa raíz

`varn-jit` baja de **bytecode** y reconstruye el tipo de cada registro con una
lattice (`clif/kinds.rs`) sobre `register_meta`. Como no hay un valor tipado
único circulando, cada op/frontera re-deriva "de dónde sale el valor y con qué
tag": `box_or_load_home`, `store_home`, `use_int`, helpers por clase, variantes
de marshal, fast-paths de intrínsecos. De ahí las N rutas para el mismo hecho
("invocar/leer con un valor tipado") y los value-flow bugs (un `str` llega
`heap[46]` en una ruta y bien en otra).

El diseño correcto ya está a medio construir: `BackendTy`/TIR tipado,
`register_meta` (`SlotKind`/`SlotClass`), `FrameStore` particionado con homes
inline. Falta **unificar valor, llamada y marshalling**, y bajar el JIT de
**SSA/TIR** en vez de bytecode.

## Modelo objetivo

```
TIR (BackendTy por valor, obligatorio)
  → SSA tipada (una)
  → dos bajadas hermanas con el MISMO contrato:
      intérprete (FrameStore)   y   CLIF (mismo layout por clase)
```

- **Frame**: partición por clase, homes inline. ✅ (hecho en `jit-fase-b`)
- **Valor en registro**: escalar para `Gpr`/`Fpr`; **par tag+payload** para todo
  heap (`Ref` y `Dyn`). Una representación, no dos.
- **Llamada**: una convención (slots por clase). Intérprete y JIT comparten la
  materialización de frame y la copia de args.
- **Nativas/intrínsecos**: **una** tabla ABI + **un** marshal. El `this` viaja
  por el mismo camino que los args.
- **Sin lattice de tipos en el JIT**: el tipo viene en el SSA/TIR.

## Commits (ordenados; cada uno verde)

### C1 — Par de valor para todo heap (`Ref`+`Dyn`) · rama `jit-pair-ref`
- `vars.rs`: variable `I128` para clase `Ref`/`Dyn`.
- `def_result`/`reload_boxed`/`box_or_load_home`/`store_home`/`use_int`/
  `def_const_int`/`def_const_bool`/`Move`/comparaciones/bitwise/`this`/params
  operan el par.
- **Borra**: el camino "payload-only" para heap; `store_home` boxing por `K`;
  `box_or_load_home` fallback a home; `dest_is_ref`-como-peek (pasa a
  "es par").
- **Cierra el value-flow**: la ruta `CallMethod`→nativa marshala el receiver por
  el mismo camino que los args (causa actual: `this` no-str).
- Estado: en curso (matrix 12 ms vs Bun 15.4; tests 1–41 verdes; markdown en
  `CallMethod` nativa).

### C2 — Una convención de llamada (VM)
- Un único `ExecCtx::invoke(callee, args_window)` que hace staging→
  `prepare_call`→`run_until`; el intérprete (`exec_call_reg`/
  `exec_call_method_reg`) y el JIT lo usan.
- El "fast path" del JIT (`jit_prepare_static_call`) pasa a ser **la misma**
  operación con la activación ya empujada, no un camino paralelo.
- **Borra**: `clif_call_fallback`/`call_vm_window` como rutas distintas;
  `jit_call_native_fast`; duplicación de ventanas contiguas.
- Archivos: `exec/jit_helpers/calls.rs`, `exec/calls.rs`,
  `exec/dispatch/reg_ops/*`, `clif/alloc/calls.rs`.

### C3 — Un ABI nativo y un marshal
- `CallNativeOp`, métodos nativos y `CallMethod`-nativo entran por **una**
  función con la **misma** extracción de `[this, args...]` y **un** `FromVm`.
- **Borra**: helpers `str_*_intrinsic` dedicados como rutas (quedan, si acaso,
  como inline CLIF que comparte la extracción); los `jit_call_native_*`
  duplicados; el marshal del `this` por camino aparte.
- Archivos: `exec/jit_helpers/natives.rs`, `intrinsics.rs`,
  `exec/intrinsics/str.rs`, `varn-types/src/marshal.rs`,
  `clif/body/*`, `clif/alloc/native.rs`, `clif/methods.rs`.

### C4 — Bajar el JIT de SSA/TIR (el grande)
- `varn-jit` consume SSA/TIR tipado (o serializa clase/tipo en `.vnc`), no
  bytecode + lattice.
- **Borra**: `clif/kinds.rs` (lattice `K`), `box_or_load_home` heurístico,
  `state`/`box_for_target`, re-derivación por op.
- **Borra**: bajada desde bytecode en `clif/body/*` en favor de una bajada
  tipada.
- Es el commit que vuelve "un valor tipado ⇒ un lowering" y elimina la clase de
  bug por construcción.

### C5 — `Ref`+`Dyn` globales y gate fuera
- Quitar `VARN_JIT_ALLOW_REF` y el baile de `Ref` en `clif/lower.rs`.
- Quitar codegen de bytecode no usado.
- Re-correr `compare.ps1` (matriz 4 cuadrantes + suite + benchmarks).

## Qué se borra (lista de rutas paralelas)
- Representación payload-only para heap (`Ref`/`Dyn`).
- `clif_call_fallback` / `call_vm_window` como caminos separados del call.
- Helpers `str_*_intrinsic` dedicados como ruta propia.
- `clif/kinds.rs` (lattice) y la re-derivación de tipos por op.
- `has_boxed_slots` como heurística de frame-aware (pasa a "¿hay par?").
- Fast-paths de nativa duplicados (`jit_call_native_fast`, fnptr vs op).

## Gate por commit
`cargo check --workspace --exclude varn-lsp --all-targets` sin warnings;
`tests/main.vn` verde (con el gate de `Ref` hasta C5); tier-parity en
`56,58,62,65,101`; en C1+ `VARN_JIT_ALLOW_REF=1` avanza la suite.

## Riesgos
- C4 es grande (toca todo `clif/`); se puede subdividir por familia de ops.
- C2/C3 tocan el intérprete: cualquier cambio de convención debe mantener la
  suite del intérprete verde sin JIT (`VARN_NO_JIT=1`).
