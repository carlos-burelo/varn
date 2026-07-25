# CLIF native f64 — plan de ejecución

## Contexto

Hoy en el backend Cranelift solo el `int` llega a la instrucción sin box: la
aritmética entera se desboxea (i48 = `shl16/sar16`) y usa registros nativos. El
`float`, en cambio, rutea por `generic.rs::emit_binop` a un helper boxed
(`h.add`/`h.sub`/…): cada op float = call + box/unbox. No hay un solo `fadd`/`fmul`
en `clif/`. Esto es correcto pero lento — es el asesino de cargas float-densas
(matrix/dot).

Esta fase hace que los registros float vivan **desboxeados como `f64` en Variables
de Cranelift** y que Add/Sub/Mul/Div/comparaciones float bajen a instrucciones SSE
nativas (`fadd`/`fmul`/`fdiv`/`fcmp`), manteniendo identidad byte-a-byte con el
intérprete y el template.

## Verdad del intérprete (tier-identity, verificada file:line)

- **Representación**: NaN-boxing canónico (`vm_value.rs:74-80`). Un float `VmValue`
  = los bits crudos del `f64`, EXCEPTO que `from_f64` canonicaliza cualquier
  `(bits & QNAN) == QNAN` (quiet-NaN) a `null()`. ⇒ **unbox = `bitcast i64→f64`
  puro; box = canonicalizar (`band QNAN` + `icmp` + `select null`) + `bitcast
  f64→i64`.**
- **Aritmética tipada** (`ops_math_cmp.rs:186-218`): `from_f64(to_f64(a) op
  to_f64(b))` para Add/Sub/Mul. ⇒ el resultado nativo debe pasar por `box_f64`
  (canon) al materializarse.
- **DivFloat/ModFloat**: **TRAP en divisor `== 0.0`** (`Err("division by zero")`)
  ANTES de computar. ⇒ `fdiv` nativo debe guardar `b == 0.0` y desviar al helper
  `h.div` (que hace el trap por longjmp); solo la rama `b != 0` hace `fdiv`.
- **Operandos**: `to_f64_val(a)` acepta `is_f64()` O `is_int()` (int→f64). ⇒ un
  operando `K::Int` se convierte con `fcvt_from_sint`; `K::Float` se usa directo;
  cualquier otro (`K::Boxed`) ⇒ bail al helper genérico (correcto).

## Linchpin de arquitectura

`RegisterMeta` lleva **un `SlotKind` por registro para toda la función**
(`meta[r].kind`), y `SlotKind::Float` ya existe (`register_meta.rs:4`). ⇒ cada
registro float es monomórfico → su Variable de Cranelift se declara **`F64`** (vs
el `I64` uniforme de hoy, `lower.rs:341`) sin conflicto de tipos por reuso de
registro. La representación la decide `meta`, no el flujo: **un registro float es
`K::Float` en todas partes**.

## Invariante de representación (la regla que hace esto correcto)

- Variable de registro `r` es `F64` ⟺ `meta[r].kind == SlotKind::Float`; sino `I64`.
- Un registro `F64` **siempre** contiene un `f64` crudo (posiblemente no
  canonicalizado — un `fadd` puede producir un NaN cualquiera).
- **Todo** paso `F64-registro → i64-boxed` aplica `box_f64` (canon + bitcast):
  return, flush a home slot (safepoint/alloc), store a heap, arg a helper/call
  genérica.
- **Todo** paso `i64-boxed → F64-registro` aplica `unbox_f64` (bitcast puro): param
  de entrada, load de home slot, load de heap/field/global/array a registro float,
  resultado de helper float.
- El lattice `K::Float` se siembra en `entry0[r]` desde `meta` (como ya se siembra
  el `this` y los params) y se preserva por `Move`/ops float. `merge(Float,Float)=
  Float`; `merge(Float, otro)=Mixed` (imposible por estática → señal de bail).

## Alcance

**Dentro (v1):**
- `K::Float` en el lattice + flow + merge.
- Variables `F64` para registros meta-Float; zero-def de entrada por tipo
  (`f64const 0.0`); unbox de params float en entrada.
- Native `fadd`/`fsub`/`fmul` (Add/Sub/MulFloat) y `fdiv` (DivFloat) con guarda de
  cero → helper.
- Native `fcmp` (Lt/Gt/Lte/Gte/Eq/NeqFloat) → resultado 0/1 (`K::Bool`).
- Constante float → `f64const`.
- `box_f64`/`unbox_f64` (emit.rs); `box_or_pass`/`emit_return_value` con arm float.
- Fronteras en alloc.rs (flush/reload float), fields/arrays/globals/loads (unbox a
  f64 cuando el dest es meta-Float).

**Fuera (siguiente, documentado):**
- `ModFloat`/`PowFloat` nativos: Cranelift no tiene `frem`/`powf` → siguen en
  helper (correcto, no es gap). Operandos boxed.
- `IntToFloat`/`FloatToInt` explícitos: no existe tal opcode en `varn-core` (grep
  vacío) — la coerción int→float en op float se maneja con `fcvt_from_sint` en el
  operando; no hay opcode de cast dedicado que portar.
- Arrays/Maps tipados `F64` (repr desboxeada en heap) — es otra fase.

## Archivos

- `clif/kinds.rs`: variante `K::Float`; `merge`; `apply_kinds` (ops float →
  `K::Float` cuando corresponde, LoadConst float → `K::Float`); `kind_flow`
  siembra `entry0[r]=K::Float` para `meta[r]==Float`.
- `clif/emit.rs`: `box_f64`/`unbox_f64`; arm `SlotKind::Float` real en
  `emit_return_value` (box_f64); `box_or_pass` arm `K::Float`; helper `use_f64`.
- `clif/floats.rs` (NUEVO, governance): arms nativos `fadd/fsub/fmul/fdiv/fcmp`,
  con conversión `fcvt_from_sint` de operandos `K::Int` y guarda de cero para div.
- `clif/lower.rs`: declarar Variables por tipo (F64/I64) desde `meta`; zero-def por
  tipo; unbox de params float en entrada; despachar ops float a `floats.rs`;
  saltar el helper genérico para ops float con operandos `K::Float/K::Int`.
- `clif/alloc.rs`: `materialize_reg_to_home`/`reload_reg_from_home` con box_f64/
  unbox_f64 cuando el registro es `K::Float`.
- `clif/generic.rs`: `try_emit` deja de capturar `*Float` cuando el lowering ya los
  ruteó nativos (dispatch por kind en lower.rs antes de generic).

## TDD — tests de corrección (tier-identity, byte a byte)

Cada uno corre en 3 tiers (clif / `VARN_NO_CLIF=1` / `VARN_NO_JIT=1`) y compara
salida idéntica:
1. Aritmética float básica: `(a*b + c) / d` con finitos.
2. Producto punto en loop (float acumulador) — el microbench de perf.
3. Comparaciones float en condiciones de branch/loop.
4. Mixto int/float en op float (operando int → `fcvt_from_sint`).
5. Param float + return float; float que cruza varias llamadas.
6. **NaN**: `0.0/0.0`… — de hecho DivFloat trap; usar `sqrt(-1)`/`0.0*inf` u op
   que produzca NaN sin trap → confirmar canonicalización (clif == interp).
7. **Div por cero**: `x / 0.0` → mismo `RuntimeError` en los 3 tiers.
8. Float a través de safepoint (función alloc con float vivo) — box/unbox en flush.

## Microbenchmark de perf (scratchpad, función-envuelto)

```
fn dot(n: int): float {
  let acc = 0.0
  for (let i = 0; i < n; i = i + 1) { acc = acc + 1.5 * 2.0 }
  return acc
}
```
Medir pareado clif vs `VARN_NO_CLIF=1` vs `VARN_NO_JIT=1`, máquina en corriente,
min-based. Confirmar con `VARN_CLIF_TRACE=1` que `dot` **rutea** y que el disasm
muestra `mulsd`/`addsd` (no calls a helper). Objetivo: ganancia grande vs helper
boxed; sin regresión en fib/matmul (int, no tocan el carril float).

## Verificación estándar
1. Purgar `.vnc`. 2. `run` + `bench` de `tests/main.vn` en 3 tiers, salida
idéntica. 3. `cargo run --release --bin xtask -- build-std` tras rebuild. 4. Suite
CLIF verde 701/701/763. 5. `vn debug -p clif` sobre `dot` muestra kinds `Float` +
`mulsd`/`addsd` en el disasm. 6. Governance: `floats.rs` nuevo, ningún archivo
>1000.

## ESTADO (2026-07-25): backend ATERRIZADO y validado; activación pendiente

El lowering nativo f64 está implementado y es **correcto**: `clif/floats.rs`
(fadd/fsub/fmul/fdiv nativos + fdiv con guarda de cero → helper; mod/pow vía
helper con unbox; fcmp nativo), `box_f64`/`unbox_f64`/`use_f64` con
canonicalización quiet-NaN idéntica a `from_f64`, Variables `F64` para registros
meta-Float, unbox de params float en entrada, box en return, y `check_float_writes`
como red de seguridad contra el pánico de `def_var(F64, i64)`.

**Validado (3 tiers byte a byte):** float_ok.vn idéntico en clif/template/interp
(incl. `nanToNull → null`, div=3.5, mod=1.5); div-por-cero levanta el mismo
`RuntimeError` en clif e interp; `tests/main.vn` = **709/709/771, 0 fallos**.

**Bloqueo de activación (el nativo casi nunca dispara todavía):** el pipeline de
tipos **borra** el tipo float antes de `register_meta`, así que los registros
rara vez se marcan `Float` y las ops caen al helper (correctas, no nativas). Dos
sitios verificados:
1. **Params forzados a `Dynamic`** en `derive_register_meta` (ssa/emit.rs:134-137,
   "caller-written slots"). El tipo real vive solo en `param_kinds`. → poly: params
   Boxed, solo el temp resultado sale Float.
2. **Floats loop-carried (phi)** terminan `Dynamic` en `register_meta` pese a que
   el SSA los tipa Float (`AddFloat r4=r3` en dot, pero r3/r4 = Dynamic) — se pierde
   en `split_phi_edges`/asignación de registros/coalescing.

Activar = des-borrar esos tipos (toca el contrato compartido `register_meta` y
arriesga el template JIT), validando 3 tiers. Es trabajo de Fase 2.

## Riesgos
- **NaN no canonicalizado en box** → divergencia con interp en ops que producen
  NaN. Mitig: `box_f64` replica `from_f64`; test #6 es el gate.
- **Div-por-cero sin trap** → clif produce Inf donde interp errora. Mitig: guarda
  `b==0.0`→helper; test #7 gate.
- **Variable F64 def'd con valor I64** (o viceversa) → type error de Cranelift.
  Mitig: zero-def y todas las defs por tipo derivado de `meta`; el frontend valida.
- **Registro float que fluye a `use_int`/`box_or_pass` sin arm float** → type error.
  Mitig: arms float explícitos; ops float con operando no-`Float/Int` bailan.
