# Plan de implementación — TIR, etapas 0 y 1

> **Para agentes:** SUB-SKILL REQUERIDA: usa `superpowers:subagent-driven-development` (recomendado) o `superpowers:executing-plans` para implementar este plan tarea a tarea. Los pasos usan sintaxis de checkbox (`- [ ]`) para seguimiento.

**Objetivo:** cerrar las dos divergencias entre tiers que son independientes del rediseño, y construir el crate `varn-tir` — tipos, nodos, resoluciones y verificador — sin todavía conectarlo a nadie.

**Arquitectura:** la etapa 0 corrige el JIT y el parser sobre el pipeline actual, con el corpus verde. La etapa 1 crea un crate nuevo que nadie importa: `BackendTy` (el tipo único del backend), `Resolution` (lo que el checker ya prueba), los nodos `Tir*` con tipo y resolución obligatorios, las tablas del módulo, y un verificador que rechaza TIR incoherente. Al terminar, el crate compila y sus tests pasan; el lenguaje sigue funcionando exactamente igual que hoy.

**Stack:** Rust 2021, workspace de 19 crates. Cranelift para el JIT. `cargo` para tests de Rust; `vn run` y `vn debug` para el corpus `.vn`.

**Spec:** `docs/TIR_CONTRATO_TIPADO.md` — este plan implementa sus etapas 0 y 1 (§10). Léela antes de empezar: el plan argumenta desde ella.

## Restricciones globales

- **El corpus `.vn` es la prueba de integridad, no `cargo test`.** Un cambio que compila y pasa los tests de Rust puede seguir siendo un miscompile. Etapa 0: `target/release/vn.exe run tests/main.vn` debe pasar antes y después de cada tarea.
- **`cargo test` es el instrumento correcto sólo en la etapa 1**, porque el crate aún no tiene lenguaje que ejecutar.
- Rust edition 2021. `rust-toolchain.toml` fija la versión; no la cambies.
- El workspace declara `unused_crate_dependencies` como lint: una dependencia declarada que ningún target importa es un warning. Añade dependencias sólo cuando las uses en el mismo commit.
- Nada en la etapa 1 modifica `varn-checker`, `varn-compiler`, `varn-vm` ni `varn-jit`. Si una tarea te empuja a tocarlos, es señal de que el diseño del TIR se está filtrando — para y repórtalo.
- Mensajes de commit en inglés, resto de la documentación en español, siguiendo lo que ya hay en el repo.

---

# ETAPA 0 — Corrección, con el corpus verde

## Task 1: El JIT deja de saltarse el guard de overflow

**Contexto.** `VmValue` migró de NaN-boxing de 64 bits a dos palabras, y con ello `int` pasó de un payload de 48 bits a un `i64` nativo. Cinco lugares del JIT siguen razonando con la premisa vieja: si ambos operandos son `K::Int`, «la suma no puede desbordar un i64», así que se saltan la comprobación. Con `i64` completo eso es falso, y el resultado es que el intérprete lanza `integer overflow` mientras el JIT envuelve en silencio.

**Files:**
- Modify: `crates/varn-jit/src/clif/body/op_dispatch.rs:268-290` (`AddInt`/`SubInt`/`MulInt`)
- Modify: `crates/varn-jit/src/clif/body/op_dispatch.rs:266-289` (`AddImm`/`SubImm`)
- Create: `tests/102-int-overflow.vn`
- Modify: `tests/main.vn` (añadir el import)

**Interfaces:**
- Consumes: `guard_overflow(b, cc, exec_ctx, helper, r, overflow, s1, s2)` — ya existe en `clif/emit.rs`, levanta `integer overflow` si el flag del CPU quedó puesto; los caminos que no toman el atajo ya la usan.
- Produces: nada nuevo. Es una eliminación de casos especiales.

- [ ] **Step 1: Escribir el test que falla**

Crea `tests/102-int-overflow.vn`. El bucle de calentamiento lleva las tres funciones al JIT; el `try/catch` deja que el fallo sea un `assert` en vez de un aborto, para que el archivo pueda vivir dentro de `tests/main.vn`.

```
// Un `int` desborda a i64, no a i48: la premisa que el JIT usaba para
// saltarse el guard murió con el NaN-boxing. Si un tier envuelve en
// silencio donde el otro lanza, es un bug de generación de código.

function addHot(a: int, b: int): int { return a + b }
function subHot(a: int, b: int): int { return a - b }
function mulHot(a: int, b: int): int { return a * b }
function incHot(a: int): int { return a + 1 }
function decHot(a: int): int { return a - 1 }

const MAX: int = 9223372036854775807
const MIN: int = 0 - 9223372036854775807 - 1

// Calentar: el JIT sólo compila lo que se ejecuta lo suficiente.
let sink: int = 0
let i: int = 0
while (i < 500000) {
    sink = addHot(i, 1)
    sink = subHot(i, 1)
    sink = mulHot(i, 2)
    sink = incHot(i)
    sink = decHot(i)
    i = i + 1
}
assert("warmup ran", sink !== 0)

function overflows(f: () => int): bool {
    try {
        f()
        return false
    } catch (e) {
        return true
    }
}

assert("add overflow raises", overflows(() => addHot(MAX, 1)))
assert("sub overflow raises", overflows(() => subHot(MIN, 1)))
assert("mul overflow raises", overflows(() => mulHot(MAX, 2)))
assert("inc overflow raises", overflows(() => incHot(MAX)))
assert("dec overflow raises", overflows(() => decHot(MIN)))

// El camino que NO desborda sigue dando el mismo resultado.
assert("add still works", addHot(2, 3) === 5)
assert("sub still works", subHot(9, 4) === 5)
assert("mul still works", mulHot(6, 7) === 42)

print("[PASSED] 102. Int overflow across tiers ")
```

- [ ] **Step 2: Ejecutarlo y verificar que falla**

```bash
cargo build --release -p varn-cli
target/release/vn.exe run tests/102-int-overflow.vn
```

Esperado: **FALLA** en `add overflow raises` — tras el calentamiento el JIT devuelve `-9223372036854775808` en vez de lanzar, así que `overflows()` devuelve `false`.

Confirma además que el instrumento de tiers lo ve:

```bash
target/release/vn.exe run --compare-tiers tests/102-int-overflow.vn
```

Esperado: `2 tier(s) diverge from the interpreter`.

- [ ] **Step 3: Quitar el atajo de `AddInt`/`SubInt`/`MulInt`**

En `crates/varn-jit/src/clif/body/op_dispatch.rs`, la rama `OpCode::AddInt | OpCode::SubInt | OpCode::MulInt`. Sustituye el bloque que empieza en el comentario de la premisa i48:

```rust
            // Two K::Int vars are sign-extended i48 payloads (range [-2^47, 2^47-1]).
            // i48 ± i48 fits in i64 (|result| ≤ 2^48−1 ≪ 2^63−1), so the i64
            // overflow flag is mathematically impossible — skip the guard.
            let both_int = state[r1] == K::Int && state[r2] == K::Int;
            if op == OpCode::AddInt && (arr.loops.is_bounds_safe_arith(ip) || both_int) {
                let v = b.ins().iadd(s1, s2);
                b.def_var(vars[first_reg], v);
            } else if op == OpCode::SubInt && both_int {
                let v = b.ins().isub(s1, s2);
                b.def_var(vars[first_reg], v);
            } else if op == OpCode::MulInt && both_int {
                let v = b.ins().imul(s1, s2);
                b.def_var(vars[first_reg], v);
            } else {
```

por:

```rust
            // `int` is a native i64, so i64 ± i64 CAN overflow and the guard is
            // mandatory. The register kind proves the operands are integers; it
            // proves nothing about their range. (It said otherwise while a value
            // was a NaN-box with a 48-bit payload — that premise died with the
            // two-word VmValue, and the interpreter, which raises on overflow,
            // never shared it.)
            //
            // `is_bounds_safe_arith` is a different claim: the loop analysis
            // proved this arithmetic stays within an array's bounds, so the
            // range IS known. That one stays.
            if op == OpCode::AddInt && arr.loops.is_bounds_safe_arith(ip) {
                let v = b.ins().iadd(s1, s2);
                b.def_var(vars[first_reg], v);
            } else {
```

El bloque `else` que sigue —el que hace `sadd_overflow`/`ssub_overflow`/`smul_overflow` y llama a `guard_overflow`— no se toca: ahora recibe todos los casos que antes tomaban el atajo.

- [ ] **Step 4: Quitar el atajo de `AddImm`/`SubImm`**

En la rama `OpCode::AddImm | OpCode::SubImm` del mismo archivo, sustituye:

```rust
            // K::Int ± i8 cannot overflow i64 (i48 + 128 ≪ 2^63). Skip the guard.
            if arr.loops.is_induction_increment(ip) || state[src] == K::Int {
```

por:

```rust
            // Same correction as AddInt above: being a K::Int says the value is
            // an integer, not that it is small. Only the induction-variable
            // proof, which knows the loop's range, licenses skipping the guard.
            if arr.loops.is_induction_increment(ip) {
```

- [ ] **Step 5: Verificar que el test pasa**

```bash
cargo build --release -p varn-cli
target/release/vn.exe run tests/102-int-overflow.vn
```

Esperado: `[PASSED] 102. Int overflow across tiers`.

```bash
target/release/vn.exe run --compare-tiers tests/102-int-overflow.vn
```

Esperado: ningún tier diverge.

- [ ] **Step 6: Verificar que el corpus sigue verde**

Añade el import a `tests/main.vn`, en orden numérico junto a los demás:

```
import "./102-int-overflow.vn"
```

Y ejecuta la suite completa:

```bash
target/release/vn.exe run tests/main.vn
```

Esperado: todos los `[PASSED]`, sin errores.

- [ ] **Step 7: Medir que el guard no cuesta lo que no debe**

`guard_overflow` añade una comprobación de flag por operación aritmética. Es correcta y obligatoria, pero hay que saber qué cuesta:

```bash
cargo xtask compare
```

Anota el resultado en el commit. Si alguna cifra empeora de forma clara, **no revientes la corrección**: la salida correcta es un análisis de rango que pruebe el caso, no volver a envolver en silencio.

- [ ] **Step 8: Commit**

```bash
git add crates/varn-jit/src/clif/body/op_dispatch.rs tests/102-int-overflow.vn tests/main.vn
git commit -m "fix(jit): int overflows at i64, so the guard is mandatory

AddInt, SubInt, MulInt, AddImm and SubImm skipped the overflow guard
whenever both operands were K::Int, on the grounds that two
sign-extended i48 payloads cannot overflow an i64. That was true while a
value was a NaN-box. The two-word VmValue made int a native i64 and the
premise died with it; the interpreter never shared it, so the same
function raised 'integer overflow' cold and returned a wrapped value
after 500k iterations.

The register kind proves the operand is an integer. It proves nothing
about its range. Only is_bounds_safe_arith and is_induction_increment
prove a range, and those two keep their fast path."
```

---

## Task 2: `i64::MIN` es expresable como literal

**Contexto.** `-9223372036854775808` no compila. El lexer parsea la magnitud sin el signo (`parse_num_lexeme` hace `cleaned.parse::<i64>()`), y `9223372036854775808` no cabe en `i64`, así que el literal se rechaza antes de que el parser llegue a la negación. El límite inferior del tipo `int` sólo se alcanza calculándolo.

**Files:**
- Modify: `crates/varn-parser/src/expressions/calls.rs:63` (la rama `TokenKind::Minus`)
- Create: `crates/varn-parser/tests/int_min_literal.rs`
- Modify: `tests/01-arithmetic.vn`

**Interfaces:**
- Consumes: `s.kind()`, `s.peek_kind()`, `s.lexeme()`, `s.expr(range, ExprKind::IntLiteral { value, raw })` — API del parser ya existente.
- Produces: ningún tipo nuevo. El AST de todo programa que hoy compila queda **byte a byte idéntico**: el plegado se aplica sólo al literal que hoy es un error.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-parser/tests/int_min_literal.rs`:

```rust
//! `int` is an i64, so its lower bound must be writable. The sign is not part
//! of the literal token — the lexer sees only the magnitude, and
//! `9223372036854775808` does not fit in an i64 — so folding the minus into
//! the literal is the parser's job, and it is the only way i64::MIN can be
//! spelled.

/// Lex and parse one source string. `varn-lexer` is already a dev-dependency
/// of this crate.
fn parses(src: &str) -> bool {
    let (tokens, buf, lex_diags) = varn_lexer::scan(src, "test.vn");
    if !lex_diags.is_empty() {
        return false;
    }
    varn_parser::parse(tokens, buf, "test.vn").is_ok()
}

/// The lower bound of `int` parses.
#[test]
fn i64_min_parses_as_a_literal() {
    assert!(
        parses("let x: int = -9223372036854775808"),
        "i64::MIN must be writable as a literal"
    );
}

/// One past the lower bound is still an error — the fold must not widen the type.
#[test]
fn one_below_i64_min_is_rejected() {
    assert!(
        !parses("let x: int = -9223372036854775809"),
        "a value below i64::MIN must be rejected"
    );
}

/// The magnitude without a sign stays an error: it is above i64::MAX.
#[test]
fn unsigned_magnitude_is_still_rejected() {
    assert!(
        !parses("let x: int = 9223372036854775808"),
        "9223372036854775808 is above i64::MAX and must be rejected"
    );
}

/// Everything that parses today keeps parsing, and a plain negation is
/// untouched by the fold.
#[test]
fn ordinary_negation_still_parses() {
    assert!(parses("let x: int = -5"));
    assert!(parses("let y: int = -(3 + 4)"));
    assert!(parses("let z: int = 9223372036854775807"));
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-parser --test int_min_literal
```

Esperado: `i64_min_parses_as_a_literal` **FALLA** con un diagnóstico de overflow. Los otros dos pasan ya.

- [ ] **Step 3: Plegar el signo en el parser**

En `crates/varn-parser/src/expressions/calls.rs`, la rama que hoy es:

```rust
        TokenKind::Minus => prefix_unary!(UnaryOp::Minus),
```

pasa a comprobar primero el único caso que la negación puede expresar y el literal por sí solo no:

```rust
        TokenKind::Minus => {
            // The lexer sees a magnitude, never a sign, so the magnitude of
            // i64::MIN (2^63) does not fit the i64 it parses into and the
            // literal is rejected before the minus is ever considered. Folding
            // the sign here is what makes the lower bound of `int` writable.
            //
            // Deliberately narrow: only the one magnitude that is unspellable
            // otherwise is folded, so every program that parses today keeps the
            // exact same AST.
            const I64_MIN_MAGNITUDE: &str = "9223372036854775808";
            s.advance(); // consume the `-`
            if s.kind() == TokenKind::IntegerLiteral
                && s.lexeme().replace('_', "") == I64_MIN_MAGNITUDE
            {
                let raw = s.consume_lexeme();
                let full_range = s.span_from(start_range);
                return Ok(s.expr(
                    full_range,
                    ExprKind::IntLiteral {
                        value: i64::MIN,
                        raw: std::rc::Rc::from(format!("-{}", raw).as_str()),
                    },
                ));
            }
            // Not the special case: the same body `prefix_unary!` has, minus
            // the `advance` already done above.
            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Unary {
                    op: UnaryOp::Minus,
                    prefix: true,
                    operand: Box::new(o),
                },
            ))
        }
```

`TokenStream` expone `kind()`, `lexeme() -> &str`, `consume_lexeme() -> Rc<str>` y `advance()` (`crates/varn-parser/src/stream.rs:68,164,256`). No hay `peek_lexeme`, y por eso el `advance` va primero: consumir el `-` antes de mirar es exactamente lo que `prefix_unary!` ya hacía.

- [ ] **Step 4: Verificar que los tres tests pasan**

```bash
cargo test -p varn-parser --test int_min_literal
```

Esperado: los tres PASS.

- [ ] **Step 5: Añadir la aserción al corpus**

En `tests/01-arithmetic.vn`, junto a las demás aserciones de enteros:

```
assert("i64 min literal",  -9223372036854775808 === 0 - 9223372036854775807 - 1)
assert("i64 max literal",  9223372036854775807 === 0 + 9223372036854775807)
```

- [ ] **Step 6: Verificar el corpus completo**

```bash
cargo build --release -p varn-cli
target/release/vn.exe run tests/main.vn
```

Esperado: todos los `[PASSED]`.

- [ ] **Step 7: Commit**

```bash
git add crates/varn-parser/src/expressions/calls.rs crates/varn-parser/tests/int_min_literal.rs tests/01-arithmetic.vn
git commit -m "fix(parser): fold the sign so i64::MIN can be written

An integer literal token carries a magnitude — the lexer never sees the
sign — and the magnitude of i64::MIN does not fit the i64 it parses
into, so -9223372036854775808 was rejected before the minus was
considered. The lower bound of int was reachable only by computing it.

The fold is deliberately narrow: it fires only on the one magnitude that
is otherwise unspellable, so every program that parses today keeps
exactly the AST it had."
```

---

# ETAPA 1 — `varn-tir`, sin consumidores

A partir de aquí nada del lenguaje cambia. El crate se construye, se prueba con `cargo test`, y nadie lo importa. El corpus debe seguir verde en cada commit por la vía trivial de que no se toca nada que lo afecte.

## Task 3: El crate y `BackendTy`

**Files:**
- Create: `crates/varn-tir/Cargo.toml`
- Create: `crates/varn-tir/src/lib.rs`
- Create: `crates/varn-tir/src/ty.rs`
- Create: `crates/varn-tir/tests/backend_ty.rs`
- Modify: `Cargo.toml` (miembro del workspace)

**Interfaces:**
- Consumes: `varn_core::TypeTag`.
- Produces:
  - `BackendTy` (`Copy`), con las variantes de la spec §5.
  - `DynReason` (`Copy`): `HostBoundary`, `Union`, `IndexSignature`, `Unannotated`, `NotYetSupported`.
  - Handles `Copy`: `TyId(u32)`, `TyListId(u32)`, `ClassId(u32)`, `EnumId(u32)`, `SigId(u32)`, `FnId(u32)`, `ModuleId(u32)`, `LocalId(u32)`.
  - `TyTable` con `intern(&mut self, BackendTy) -> TyId`, `get(&self, TyId) -> BackendTy`, `intern_list(&mut self, &[BackendTy]) -> TyListId`, `get_list(&self, TyListId) -> &[BackendTy]`.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-tir/tests/backend_ty.rs`:

```rust
//! `BackendTy` is the single type the backend speaks. Two properties matter
//! more than its shape: it is `Copy`, because it is a field of every node; and
//! it has no `Default`, because "the type you get when you wrote none" is
//! exactly the hole this IR exists to close.

use varn_tir::{BackendTy, DynReason, TyTable};

/// Interning is stable: the same type interns to the same id, and reading it
/// back gives the same type.
#[test]
fn interning_round_trips_and_dedups() {
    let mut t = TyTable::default();
    let a = t.intern(BackendTy::Int);
    let b = t.intern(BackendTy::Int);
    assert_eq!(a, b, "the same type must intern to the same id");
    assert_eq!(t.get(a), BackendTy::Int);

    let arr_int = BackendTy::Array(a);
    let x = t.intern(arr_int);
    assert_eq!(t.get(x), arr_int);
    assert_ne!(x, a, "int and int[] are different types");
}

/// A list of types round-trips, for tuples and signatures.
#[test]
fn type_lists_round_trip() {
    let mut t = TyTable::default();
    let l = t.intern_list(&[BackendTy::Int, BackendTy::Str, BackendTy::Bool]);
    assert_eq!(
        t.get_list(l),
        &[BackendTy::Int, BackendTy::Str, BackendTy::Bool]
    );
}

/// Nullable keeps its payload instead of collapsing. `int?` used to reach the
/// backend as Dynamic, which is why a nullable scalar could never be a
/// (value, bit) pair.
#[test]
fn nullable_keeps_its_payload() {
    let mut t = TyTable::default();
    let int_id = t.intern(BackendTy::Int);
    let n = BackendTy::Nullable(int_id);
    match n {
        BackendTy::Nullable(inner) => assert_eq!(t.get(inner), BackendTy::Int),
        other => panic!("expected Nullable, got {:?}", other),
    }
}

/// Dynamic always says why. A count of dynamics is not actionable; a count per
/// reason is.
#[test]
fn dynamic_carries_its_reason() {
    let d = BackendTy::Dynamic(DynReason::HostBoundary);
    assert_ne!(d, BackendTy::Dynamic(DynReason::Unannotated));
}

/// `BackendTy` is Copy, so it can be a field of every node without cloning.
#[test]
fn backend_ty_is_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<BackendTy>();
    assert_copy::<DynReason>();
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-tir --test backend_ty
```

Esperado: FALLA — el crate `varn-tir` no existe.

- [ ] **Step 3: Crear el crate**

`crates/varn-tir/Cargo.toml`:

```toml
[package]
name = "varn-tir"
version.workspace = true
edition.workspace = true

[dependencies]
varn-core  = { path = "../varn-core" }
rustc-hash = { workspace = true }

[lints]
workspace = true
```

En el `Cargo.toml` raíz, añade `"crates/varn-tir",` a `members`, después de `"crates/varn-types",` para mantener el orden en que están los demás.

- [ ] **Step 4: Escribir `ty.rs`**

`crates/varn-tir/src/ty.rs`:

```rust
//! The one type the backend speaks.
//!
//! It replaces the CgTy → HirType → SlotKind cascade, where each narrowing
//! silently dropped constructors: `char`, `decimal` and `bigint` died at HIR
//! despite having a TypeTag and a runtime representation, and `T?` collapsed
//! to Dynamic, which is why a nullable scalar could never be a (value, bit)
//! pair.
//!
//! Deliberately NOT `Default`: "the type you get when you wrote none" is the
//! hole this IR exists to close. `varn_checker::types::Type` has one, and it
//! is `Dynamic`.

/// Handle into a [`TyTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);

/// Handle to a sequence of types — tuple elements, signature parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyListId(pub u32);

/// Handle into the module's class table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

/// Handle into the module's enum table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumId(pub u32);

/// Handle into the module's signature table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigId(pub u32);

/// Handle to a function the module can call directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnId(pub u32);

/// Handle to an imported module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u32);

/// A local binding within one function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

/// Why a value is dynamic. A total count is not actionable; a count per
/// reason is — it separates the host boundary, which is honest, from an
/// inference hole, which is a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynReason {
    /// Crosses the host boundary: a native return, `JSON.parse`.
    HostBoundary,
    /// A non-discriminated union. Representing these is deliberately deferred.
    Union,
    /// Read off an index signature — `{ [key: str]: T }` has no named members.
    IndexSignature,
    /// The author wrote no annotation and inference reached no answer.
    Unannotated,
    /// The TIR cannot express this type yet. This is the redesign's backlog.
    NotYetSupported,
}

/// The type of a value, as the backend sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendTy {
    // Scalars — travel in a register, untagged.
    Int,
    Float,
    Bool,
    Char,
    // References with a known type.
    Str,
    Decimal,
    BigInt,
    Array(TyId),
    Map(TyId, TyId),
    Set(TyId),
    Tuple(TyListId),
    Class(ClassId),
    Enum(EnumId),
    Fn(SigId),
    /// `T?` — the payload plus null. Keeps the payload: over a reference this
    /// is the null pattern, over a scalar a (value, bit) pair.
    Nullable(TyId),
    Void,
    Never,
    Dynamic(DynReason),
}

impl BackendTy {
    /// The type with nullability stripped, for consumers that guard null
    /// separately. Needs the table because the payload is behind a handle.
    pub fn non_nullable(self, t: &TyTable) -> BackendTy {
        match self {
            BackendTy::Nullable(inner) => t.get(inner).non_nullable(t),
            other => other,
        }
    }

    /// Whether a value of this type fits in one machine register with no tag.
    pub fn is_unboxed_scalar(self) -> bool {
        matches!(
            self,
            BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Char
        )
    }
}

/// Per-module interning table for the structured types.
#[derive(Debug, Default)]
pub struct TyTable {
    entries: Vec<BackendTy>,
    dedup: rustc_hash::FxHashMap<BackendTy, u32>,
    lists: Vec<Vec<BackendTy>>,
}

impl TyTable {
    pub fn intern(&mut self, ty: BackendTy) -> TyId {
        if let Some(&i) = self.dedup.get(&ty) {
            return TyId(i);
        }
        let i = self.entries.len() as u32;
        self.entries.push(ty);
        self.dedup.insert(ty, i);
        TyId(i)
    }

    pub fn get(&self, id: TyId) -> BackendTy {
        self.entries[id.0 as usize]
    }

    pub fn intern_list(&mut self, tys: &[BackendTy]) -> TyListId {
        if let Some(i) = self.lists.iter().position(|l| l.as_slice() == tys) {
            return TyListId(i as u32);
        }
        let i = self.lists.len() as u32;
        self.lists.push(tys.to_vec());
        TyListId(i)
    }

    pub fn get_list(&self, id: TyListId) -> &[BackendTy] {
        &self.lists[id.0 as usize]
    }

    /// Whether `id` names an entry this table holds. The verifier's
    /// well-formedness check calls this on every handle it meets.
    pub fn contains(&self, id: TyId) -> bool {
        (id.0 as usize) < self.entries.len()
    }

    pub fn contains_list(&self, id: TyListId) -> bool {
        (id.0 as usize) < self.lists.len()
    }
}
```

`crates/varn-tir/src/lib.rs`:

```rust
//! The typed IR: the contract between the checker and the backend.
//!
//! Every node carries its type and its resolution as fields of the node, not
//! as optional entries in a side map. See `docs/TIR_CONTRATO_TIPADO.md`.

mod ty;

pub use ty::{
    BackendTy, ClassId, DynReason, EnumId, FnId, LocalId, ModuleId, SigId, TyId, TyListId, TyTable,
};
```

- [ ] **Step 5: Verificar que los tests pasan**

```bash
cargo test -p varn-tir --test backend_ty
```

Esperado: los cinco PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/varn-tir/
git commit -m "feat(tir): the one type the backend speaks

BackendTy replaces the CgTy → HirType → SlotKind cascade. Each narrowing
in that chain dropped constructors silently: char, decimal and bigint
died at HIR despite having a TypeTag and a runtime representation, and
T? collapsed to Dynamic, which is why a nullable scalar could never be
the (value, bit) pair the reference design calls for.

Two properties carry the design. It is Copy, so it can be a field of
every node rather than an entry in a map. And it has no Default: 'the
type you get when you wrote none' is the hole this IR exists to close."
```

---

## Task 4: `Resolution`

**Files:**
- Create: `crates/varn-tir/src/resolution.rs`
- Create: `crates/varn-tir/tests/resolution.rs`
- Modify: `crates/varn-tir/src/lib.rs`

**Interfaces:**
- Consumes: los handles de la Task 3.
- Produces: `Resolution` (`Clone`, no `Copy` — `ByName` lleva un `Rc<str>`), con `is_static_dispatch()` y `dyn_reason()`.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-tir/tests/resolution.rs`:

```rust
//! What the checker proved about *which* entity an expression refers to.
//! Today the backend re-derives this at runtime: InvokeVirtual resolves a
//! method by name, globals are patched from name-keyed to index-keyed before
//! execution, and the method dispatcher strcmps against `push` and `pop`.
//! None of that is information the runtime has and the checker lacks.

use std::rc::Rc;
use varn_tir::{DynReason, Resolution};

/// A resolution either dispatches statically or it does not, and the
/// distinction is what the coverage report counts.
#[test]
fn static_dispatch_is_distinguishable() {
    assert!(Resolution::FieldSlot(3).is_static_dispatch());
    assert!(Resolution::VtableSlot(7).is_static_dispatch());
    assert!(Resolution::GlobalSlot(12).is_static_dispatch());
    assert!(!Resolution::ByName {
        name: Rc::from("x"),
        why: DynReason::IndexSignature,
    }
    .is_static_dispatch());
}

/// A by-name resolution always says why, so 1037 name-keyed reads can be
/// split into the ones that are honest and the ones that are bugs.
#[test]
fn by_name_carries_its_reason() {
    let r = Resolution::ByName {
        name: Rc::from("length"),
        why: DynReason::HostBoundary,
    };
    assert_eq!(r.dyn_reason(), Some(DynReason::HostBoundary));
    assert_eq!(Resolution::FieldSlot(0).dyn_reason(), None);
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-tir --test resolution
```

Esperado: FALLA — `Resolution` no existe.

- [ ] **Step 3: Escribir `resolution.rs`**

```rust
//! What the checker proved about which entity an expression names.

use crate::ty::{DynReason, EnumId, FnId, LocalId, ModuleId};
use std::rc::Rc;

/// The entity an expression resolves to.
///
/// Every static variant here is something the checker already proves and the
/// runtime currently re-derives — by name lookup, by bytecode rewriting, or by
/// string comparison against a hard-coded list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Literals and pure arithmetic resolve to nothing.
    None,

    Local(LocalId),
    Param(u32),
    Upvalue(u32),

    /// Numbered at compile time, so `LoadGlobalIdx` is emitted directly and
    /// the runtime rewriting pass disappears.
    GlobalSlot(u32),
    ModuleSlot { module: ModuleId, slot: u32 },

    /// Instance field at a known slot.
    FieldSlot(u16),
    StaticField(u16),

    /// Method at a known vtable index — an integer, not a name.
    VtableSlot(u16),

    /// A call whose target is known: a free function, or a method that cannot
    /// be overridden.
    DirectFn(FnId),

    /// A builtin the compiler can emit directly, instead of the runtime
    /// comparing the method name against a list.
    Intrinsic(u16),

    /// A native operation, by its registered id.
    NativeOp(u64),

    EnumVariant { enum_id: EnumId, tag: u16 },

    /// Honestly dynamic. Carries why, so the remaining name-keyed accesses can
    /// be separated into the ones that are correct and the ones that are holes.
    ByName { name: Rc<str>, why: DynReason },
}

impl Resolution {
    /// Whether this resolves to a known entity at compile time.
    pub fn is_static_dispatch(&self) -> bool {
        !matches!(self, Resolution::ByName { .. })
    }

    /// The reason, when this resolution is dynamic.
    pub fn dyn_reason(&self) -> Option<DynReason> {
        match self {
            Resolution::ByName { why, .. } => Some(*why),
            _ => None,
        }
    }

    /// Whether this resolution only makes sense against a class receiver. The
    /// verifier uses it to demand that a FieldSlot's object actually be one.
    pub fn requires_class_receiver(&self) -> bool {
        matches!(
            self,
            Resolution::FieldSlot(_) | Resolution::StaticField(_) | Resolution::VtableSlot(_)
        )
    }
}
```

En `lib.rs`, añade:

```rust
mod resolution;
pub use resolution::Resolution;
```

- [ ] **Step 4: Verificar que los tests pasan**

```bash
cargo test -p varn-tir
```

Esperado: todos PASS (los de `backend_ty` incluidos).

- [ ] **Step 5: Commit**

```bash
git add crates/varn-tir/
git commit -m "feat(tir): what the checker proved about which entity is named

Every static variant here is something the checker already knows and the
runtime currently re-derives: InvokeVirtual resolves a method by name
even though it is only emitted where the class IS known, globals are
compiled name-keyed and patched to index-keyed before execution, and the
method dispatcher strcmps against push and pop before reaching its
inline cache.

ByName carries a reason for the same purpose Dynamic does: it turns
'1037 name-keyed reads remain' into a list that can be prioritised."
```

---

## Task 5: Los nodos

**Files:**
- Create: `crates/varn-tir/src/node.rs`
- Create: `crates/varn-tir/tests/node.rs`
- Modify: `crates/varn-tir/src/lib.rs`

**Interfaces:**
- Consumes: `BackendTy`, `Resolution`, los handles.
- Produces: `TirExpr { kind, ty, res, span }`, `TirExprKind`, `TirStmt`, `TirFunction`, `TirModule`, `Span`, `TirBinOp`, `TirUnOp`.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-tir/tests/node.rs`:

```rust
//! The type is a field of the NODE, not of some variants of an enum.
//!
//! In HirExpr, `ty` sits on Binary and Member but not on Array, Object,
//! Assign, OptionalChain or TryOp — for those there is nowhere to write a
//! type, so the consumer assumes one. A struct with a mandatory field cannot
//! have that shape.

use varn_tir::{BackendTy, Resolution, Span, TirExpr, TirExprKind};

/// Every node has a type, whatever its kind. This is a compile-time property
/// — the test exists to pin it, because the moment `ty` becomes an Option the
/// old failure mode is back.
#[test]
fn every_node_kind_carries_a_type() {
    let lit = TirExpr {
        kind: TirExprKind::IntLit(42),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    };
    assert_eq!(lit.ty, BackendTy::Int);

    // An array literal — one of the kinds HIR had no slot for.
    let arr = TirExpr {
        kind: TirExprKind::ArrayLit(vec![lit]),
        ty: BackendTy::Int, // stands in for Array(TyId) built from a real table
        res: Resolution::None,
        span: Span::EMPTY,
    };
    assert_eq!(arr.ty, BackendTy::Int);
}

/// A field access is one kind with a resolution, not two separate variants.
/// The Member / GetFixedField split in HIR is what propagates into the
/// GetProperty / GetFixedField opcode pair and every consumer below it.
#[test]
fn field_access_is_one_kind_with_a_resolution() {
    let recv = TirExpr {
        kind: TirExprKind::IntLit(0),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    };
    let fast = TirExpr {
        kind: TirExprKind::Field {
            object: Box::new(recv.clone()),
            name: "x".into(),
        },
        ty: BackendTy::Int,
        res: Resolution::FieldSlot(0),
        span: Span::EMPTY,
    };
    let slow = TirExpr {
        kind: TirExprKind::Field {
            object: Box::new(recv),
            name: "x".into(),
        },
        ty: BackendTy::Int,
        res: Resolution::ByName {
            name: "x".into(),
            why: varn_tir::DynReason::IndexSignature,
        },
        span: Span::EMPTY,
    };
    assert!(fast.res.is_static_dispatch());
    assert!(!slow.res.is_static_dispatch());
    // Same kind. Only the resolution differs.
    assert!(matches!(fast.kind, TirExprKind::Field { .. }));
    assert!(matches!(slow.kind, TirExprKind::Field { .. }));
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-tir --test node
```

Esperado: FALLA — los nodos no existen.

- [ ] **Step 3: Escribir `node.rs`**

```rust
//! The nodes.
//!
//! `ty` and `res` are fields of `TirExpr`, so a constructor cannot omit them.
//! That is the whole point: in `HirExpr` the type was a field of *some*
//! variants, and the ones without it — Array, Object, Assign, OptionalChain,
//! TryOp — left the consumer to assume.

use crate::resolution::Resolution;
use crate::ty::{BackendTy, ClassId, EnumId, FnId, SigId, TyTable};
use std::rc::Rc;

/// Byte range in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const EMPTY: Span = Span { start: 0, end: 0 };
}

/// An expression: what it does, what it produces, what it resolves against.
#[derive(Debug, Clone)]
pub struct TirExpr {
    pub kind: TirExprKind,
    pub ty: BackendTy,
    pub res: Resolution,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirBinOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, Ne, Lt, Le, Gt, Ge,
    BitAnd, BitOr, BitXor, Shl, Shr, Ushr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirUnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone)]
pub enum TirExprKind {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StrLit(Rc<str>),
    CharLit(char),
    NullLit,

    /// A binding reference. Which binding is in `res`.
    Var,

    Binary { op: TirBinOp, lhs: Box<TirExpr>, rhs: Box<TirExpr> },
    Unary { op: TirUnOp, operand: Box<TirExpr> },

    /// Field access. Slot or by-name lives in `res`, not in the kind.
    Field { object: Box<TirExpr>, name: Rc<str> },
    Index { object: Box<TirExpr>, index: Box<TirExpr> },

    Call { callee: Box<TirExpr>, args: Vec<TirExpr> },
    /// Method call. Vtable slot, intrinsic or by-name lives in `res`.
    MethodCall { recv: Box<TirExpr>, name: Rc<str>, args: Vec<TirExpr> },

    Assign { target: Box<TirExpr>, value: Box<TirExpr> },

    ArrayLit(Vec<TirExpr>),
    TupleLit(Vec<TirExpr>),
    ObjectLit { fields: Vec<(Rc<str>, TirExpr)> },

    /// Explicit representation change. The verifier requires one wherever an
    /// operation would otherwise mix representations.
    Cast { operand: Box<TirExpr> },

    /// Construction of a class instance.
    New { class: ClassId, args: Vec<TirExpr> },
    /// Construction of an enum variant. Which variant lives in `res`.
    MakeVariant { args: Vec<TirExpr> },

    /// `cond ? a : b`
    Select { cond: Box<TirExpr>, then_val: Box<TirExpr>, else_val: Box<TirExpr> },
}

#[derive(Debug, Clone)]
pub enum TirStmt {
    Expr(TirExpr),
    /// A local binding. Its type is the declared type; the initializer's type
    /// must be assignable to it, which the verifier checks.
    Let { local: crate::ty::LocalId, ty: BackendTy, init: Option<TirExpr> },
    Return(Option<TirExpr>),
    If { cond: TirExpr, then_body: Vec<TirStmt>, else_body: Vec<TirStmt> },
    /// The only loop form. `for…of` and `for` are desugared into it by the
    /// emitter — if either survives as its own node, the TIR is not desugared
    /// and `hir/` cannot be deleted.
    Loop { cond: TirExpr, body: Vec<TirStmt> },
    Break,
    Continue,
    Throw(TirExpr),
    Try { body: Vec<TirStmt>, catch_local: crate::ty::LocalId, catch_body: Vec<TirStmt> },
}

#[derive(Debug, Clone)]
pub struct TirFunction {
    pub name: Rc<str>,
    pub sig: SigId,
    pub params: Vec<BackendTy>,
    pub return_ty: BackendTy,
    pub locals: Vec<BackendTy>,
    pub body: Vec<TirStmt>,
    pub has_this: bool,
    pub this_class: Option<ClassId>,
}

#[derive(Debug)]
pub struct TirModule {
    pub source_file: Rc<str>,
    pub types: TyTable,
    pub classes: Vec<crate::tables::ClassInfo>,
    pub enums: Vec<crate::tables::EnumInfo>,
    pub signatures: Vec<crate::tables::Signature>,
    pub functions: Vec<TirFunction>,
    pub globals: Vec<BackendTy>,
    pub top_level: TirFunction,
}

impl TirModule {
    pub fn class(&self, id: ClassId) -> Option<&crate::tables::ClassInfo> {
        self.classes.get(id.0 as usize)
    }
    pub fn enum_info(&self, id: EnumId) -> Option<&crate::tables::EnumInfo> {
        self.enums.get(id.0 as usize)
    }
    pub fn signature(&self, id: SigId) -> Option<&crate::tables::Signature> {
        self.signatures.get(id.0 as usize)
    }
    pub fn function(&self, id: FnId) -> Option<&TirFunction> {
        self.functions.get(id.0 as usize)
    }
}
```

En `lib.rs`, añade `mod node;` y `mod tables;`, y reexporta `Span`, `TirExpr`, `TirExprKind`, `TirStmt`, `TirFunction`, `TirModule`, `TirBinOp`, `TirUnOp`.

- [ ] **Step 4: Verificar que compila y los tests pasan**

`node.rs` referencia `crate::tables`, que llega en la Task 6. Para que esta tarea cierre por sí sola, crea `crates/varn-tir/src/tables.rs` con los tres tipos vacíos que la Task 6 rellenará:

```rust
//! Per-module tables. Populated in the next task.

use crate::ty::BackendTy;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: Rc<str>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Rc<str>,
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<BackendTy>,
    pub return_ty: BackendTy,
}
```

```bash
cargo test -p varn-tir
```

Esperado: todos PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/varn-tir/
git commit -m "feat(tir): the type is a field of the node, not of the variant

HirExpr put `ty` on Binary and Member but not on Array, Object, Assign,
OptionalChain or TryOp. For those there was nowhere to write a type at
all, so the consumer assumed one — a failure no amount of annotating can
reach. TirExpr is a struct whose ty and res are mandatory fields.

Field access is one kind with a resolution rather than the Member /
GetFixedField pair, and that split is what propagated all the way down
into the GetProperty / GetFixedField opcode pair."
```

---

## Task 6: Las tablas del módulo

**Files:**
- Modify: `crates/varn-tir/src/tables.rs`
- Create: `crates/varn-tir/tests/tables.rs`

**Interfaces:**
- Produces: `ClassInfo { name, parent, fields, vtable, payload_size }`, `FieldInfo { name, ty, slot, offset }`, `VtableEntry { name, sig, defined_in }`, `EnumInfo { name, variants }`, `VariantInfo { name, tag, payload }`, `Signature { params, return_ty }`, y `ClassInfo::layout_from(fields, parent) -> Vec<FieldInfo>` como **única** autoridad de offsets.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-tir/tests/tables.rs`:

```rust
//! One authority for how a typed field is laid out.
//!
//! Today there are four sites computing this and two answers:
//! ClassLayout::from_fields discards field_repr() and uses 16 bytes for
//! everything, the checker's annotator packs by real alignment, the JIT
//! recomputes slot*16, and set_property looks the field up by name. The
//! checker's packed offset has no consumer, which is the only reason the
//! divergence is currently harmless.

use varn_tir::{BackendTy, ClassId, ClassInfo};
use std::rc::Rc;

/// A subclass's fields come after its parent's, so a pointer to the derived
/// class is a valid pointer to the base without adjustment.
#[test]
fn inheritance_lays_out_by_prefix() {
    let base = ClassInfo::new(Rc::from("Base"), None, vec![
        ("a".into(), BackendTy::Int),
        ("b".into(), BackendTy::Bool),
    ]);
    let derived = ClassInfo::new(Rc::from("Derived"), Some((ClassId(0), &base)), vec![
        ("c".into(), BackendTy::Float),
    ]);

    assert_eq!(derived.field("a").map(|f| f.slot), Some(0));
    assert_eq!(derived.field("b").map(|f| f.slot), Some(1));
    assert_eq!(derived.field("c").map(|f| f.slot), Some(2));
    assert_eq!(base.field("c").map(|f| f.slot), None);
    assert_eq!(derived.parent, Some(ClassId(0)), "the parent id is recorded");
    assert_eq!(base.parent, None);
}

/// Slots are dense and in declaration order within a class.
#[test]
fn slots_are_dense_and_ordered() {
    let c = ClassInfo::new(Rc::from("P"), None, vec![
        ("x".into(), BackendTy::Int),
        ("y".into(), BackendTy::Int),
        ("z".into(), BackendTy::Str),
    ]);
    let slots: Vec<u16> = c.fields.iter().map(|f| f.slot).collect();
    assert_eq!(slots, vec![0, 1, 2]);
}

/// A field's declared type reaches the layout. A layout reporting Dynamic for
/// a declared int means the type was dropped on the way in.
#[test]
fn declared_types_reach_the_layout() {
    let c = ClassInfo::new(Rc::from("P"), None, vec![
        ("n".into(), BackendTy::Int),
        ("s".into(), BackendTy::Str),
    ]);
    assert_eq!(c.field("n").map(|f| f.ty), Some(BackendTy::Int));
    assert_eq!(c.field("s").map(|f| f.ty), Some(BackendTy::Str));
}

/// Overriding a method reuses the parent's vtable index, which is what makes
/// the index a valid dispatch target for a base-typed receiver.
#[test]
fn override_reuses_the_parent_slot() {
    let base = ClassInfo::new_with_methods(
        Rc::from("Animal"), None, vec![],
        vec!["speak".into(), "name".into()],
    );
    let derived = ClassInfo::new_with_methods(
        Rc::from("Dog"), Some((ClassId(0), &base)), vec![],
        vec!["speak".into(), "fetch".into()],
    );

    assert_eq!(base.method_slot("speak"), Some(0));
    assert_eq!(derived.method_slot("speak"), Some(0), "override reuses the slot");
    assert_eq!(derived.method_slot("name"), Some(1), "inherited keeps its slot");
    assert_eq!(derived.method_slot("fetch"), Some(2), "new method appends");
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-tir --test tables
```

Esperado: FALLA — `ClassInfo::new` no existe.

- [ ] **Step 3: Implementar las tablas**

Sustituye `crates/varn-tir/src/tables.rs` por la implementación completa. Puntos que el test fija y que no puedes cambiar: los campos del padre ocupan el prefijo, los slots son densos en orden de declaración, `override` reutiliza el índice del padre, y un método nuevo se añade al final.

```rust
//! Per-module tables: classes with their layout and vtable, enums, signatures.
//!
//! This is the ONLY authority on where a field lives. Four sites compute that
//! today and two of them disagree; the packed offset the checker computes has
//! no consumer, which is the only reason the divergence has not yet produced a
//! misaligned read.

use crate::ty::{BackendTy, ClassId};
use std::rc::Rc;

/// One field of a class instance.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub name: Rc<str>,
    pub ty: BackendTy,
    /// Dense index, counting the parent's fields first.
    pub slot: u16,
    /// Byte offset within the instance payload.
    pub offset: u32,
}

/// One entry of a class vtable. Its index IS the dispatch target, so the
/// position in this vector is the whole payload — the name is kept for
/// diagnostics and for the emitter to match overrides against.
#[derive(Debug, Clone, PartialEq)]
pub struct VtableEntry {
    pub name: Rc<str>,
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: Rc<str>,
    pub parent: Option<ClassId>,
    pub fields: Vec<FieldInfo>,
    pub vtable: Vec<VtableEntry>,
    pub payload_size: u32,
}

/// Bytes one field slot occupies today. Every read and write path still moves
/// a whole VmValue, so this is 16 and the offsets are `slot * 16`. It is a
/// single constant in a single file precisely so that packing later is one
/// change here, not four changes that have to agree.
const SLOT_SIZE: u32 = 16;

impl ClassInfo {
    /// A class with fields and no methods.
    ///
    /// `parent` is the id AND the info: the id goes into the record, the info
    /// supplies the prefix. Taking only one of the two is what leaves a
    /// `parent` field that never gets filled.
    pub fn new(
        name: Rc<str>,
        parent: Option<(ClassId, &ClassInfo)>,
        fields: Vec<(Rc<str>, BackendTy)>,
    ) -> Self {
        Self::new_with_methods(name, parent, fields, Vec::new())
    }

    /// A class with fields and methods, laid out against its parent.
    pub fn new_with_methods(
        name: Rc<str>,
        parent: Option<(ClassId, &ClassInfo)>,
        fields: Vec<(Rc<str>, BackendTy)>,
        methods: Vec<Rc<str>>,
    ) -> Self {
        let parent_info = parent.map(|(_, info)| info);

        // Fields: the parent's prefix, then this class's own, dense.
        let mut out: Vec<FieldInfo> = parent_info.map(|p| p.fields.clone()).unwrap_or_default();
        let mut slot = out.len() as u16;
        for (fname, fty) in fields {
            out.push(FieldInfo {
                name: fname,
                ty: fty,
                slot,
                offset: slot as u32 * SLOT_SIZE,
            });
            slot += 1;
        }
        let payload_size = out.len() as u32 * SLOT_SIZE;

        // Vtable: the parent's entries, then the new ones. A method the parent
        // already has keeps its index — that is what makes the index a valid
        // dispatch target for a base-typed receiver.
        let mut vtable: Vec<VtableEntry> =
            parent_info.map(|p| p.vtable.clone()).unwrap_or_default();
        for m in methods {
            if !vtable.iter().any(|e| e.name == m) {
                vtable.push(VtableEntry { name: m });
            }
        }

        ClassInfo {
            name,
            parent: parent.map(|(id, _)| id),
            fields: out,
            vtable,
            payload_size,
        }
    }

    pub fn field(&self, name: &str) -> Option<&FieldInfo> {
        self.fields.iter().find(|f| f.name.as_ref() == name)
    }

    pub fn field_at(&self, slot: u16) -> Option<&FieldInfo> {
        self.fields.get(slot as usize)
    }

    pub fn method_slot(&self, name: &str) -> Option<u16> {
        self.vtable.iter().position(|e| e.name.as_ref() == name).map(|i| i as u16)
    }

    pub fn method_at(&self, slot: u16) -> Option<&VtableEntry> {
        self.vtable.get(slot as usize)
    }
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: Rc<str>,
    pub tag: u16,
    pub payload: Vec<BackendTy>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Rc<str>,
    pub variants: Vec<VariantInfo>,
}

impl EnumInfo {
    pub fn variant_at(&self, tag: u16) -> Option<&VariantInfo> {
        self.variants.iter().find(|v| v.tag == tag)
    }
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<BackendTy>,
    pub return_ty: BackendTy,
}

impl Signature {
    pub fn arity(&self) -> usize {
        self.params.len()
    }
}
```

`SigId` no se usa dentro de este archivo: quita el import si el compilador lo señala. `ClassId` sí, en `ClassInfo::parent`.

Reexporta desde `lib.rs`: `ClassInfo`, `FieldInfo`, `VtableEntry`, `EnumInfo`, `VariantInfo`, `Signature`.

- [ ] **Step 4: Verificar que los tests pasan**

```bash
cargo test -p varn-tir
```

Esperado: todos PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/varn-tir/
git commit -m "feat(tir): one authority for class layout and vtable

Four sites compute a class's field offsets today and they give two
answers: ClassLayout::from_fields discards field_repr() and uses a
16-byte slot for everything, the checker's annotator packs by real
alignment, the JIT recomputes slot*16, and set_property finds the field
by scanning names. The packed offset has no consumer, which is the only
reason the disagreement has not produced a misaligned read.

Here it is computed once. Fields lay out by prefix so a pointer to a
derived class is a valid pointer to its base, and an override reuses its
parent's vtable index, which is what makes that index dispatchable from
a base-typed receiver.

SLOT_SIZE is one constant in one file so that packing later is one
change rather than four that have to agree."
```

---

## Task 7: Verificador — bien formado

**Files:**
- Create: `crates/varn-tir/src/verify/mod.rs`
- Create: `crates/varn-tir/src/verify/wellformed.rs`
- Create: `crates/varn-tir/tests/verify_wellformed.rs`

**Interfaces:**
- Produces: `VerifyError { message, span }`, `verify_module(&TirModule) -> Result<(), Vec<VerifyError>>`.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-tir/tests/verify_wellformed.rs`. Construye un `TirModule` mínimo con un helper local, porque todavía no hay emisor:

```rust
//! Well-formedness: every handle points at something that exists, and every
//! slot is in range of the table it claims to index. These are the checks
//! that make a dangling ClassId or an out-of-range vtable slot impossible
//! rather than improbable.

use std::rc::Rc;
use varn_tir::*;

fn empty_module() -> TirModule {
    TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![ClassInfo::new(Rc::from("P"), None, vec![("x".into(), BackendTy::Int)])],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    }
}

fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr { kind, ty, res, span: Span::EMPTY }
}

/// A module with nothing wrong passes.
#[test]
fn an_empty_module_verifies() {
    assert!(verify_module(&empty_module()).is_ok());
}

/// A ClassId with no entry is rejected. Today nothing checks this.
#[test]
fn a_dangling_class_id_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::New { class: ClassId(99), args: vec![] },
        BackendTy::Class(ClassId(99)),
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("ClassId")),
        "expected a dangling-class error, got: {:?}",
        errs
    );
}

/// A field slot past the end of the class's layout is rejected.
#[test]
fn an_out_of_range_field_slot_is_rejected() {
    let mut m = empty_module();
    let recv = expr(TirExprKind::Var, BackendTy::Class(ClassId(0)), Resolution::Local(LocalId(0)));
    m.top_level.locals.push(BackendTy::Class(ClassId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "nope".into() },
        BackendTy::Int,
        Resolution::FieldSlot(7), // the class has one field
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("slot")),
        "expected an out-of-range slot error, got: {:?}",
        errs
    );
}

/// A vtable slot past the end of the class's vtable is rejected.
#[test]
fn an_out_of_range_vtable_slot_is_rejected() {
    let mut m = empty_module();
    let recv = expr(TirExprKind::Var, BackendTy::Class(ClassId(0)), Resolution::Local(LocalId(0)));
    m.top_level.locals.push(BackendTy::Class(ClassId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::MethodCall { recv: Box::new(recv), name: "m".into(), args: vec![] },
        BackendTy::Void,
        Resolution::VtableSlot(3), // the class has no methods
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("vtable")),
        "expected an out-of-range vtable error, got: {:?}",
        errs
    );
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-tir --test verify_wellformed
```

Esperado: FALLA — `verify_module` no existe.

- [ ] **Step 3: Implementar la comprobación de buena formación**

`crates/varn-tir/src/verify/mod.rs`:

```rust
//! The verifier.
//!
//! Runs on every compilation, like `ssa/verify.rs` does today — not behind
//! cfg(debug_assertions). It is the only instrument that works while the
//! corpus does not run: `vn debug -p bytecode` compiles without executing, so
//! verifying the 191 modules of the corpus catches incoherence over real code
//! with nothing running.

mod wellformed;

use crate::node::{Span, TirModule};

#[derive(Debug, Clone, PartialEq)]
pub struct VerifyError {
    pub message: String,
    pub span: Span,
}

impl VerifyError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        VerifyError { message: message.into(), span }
    }
}

/// Verify a module. Returns every error found, not just the first: a single
/// missing case in the emitter usually produces many, and seeing them together
/// is what identifies the case.
pub fn verify_module(m: &TirModule) -> Result<(), Vec<VerifyError>> {
    let mut errors = Vec::new();
    wellformed::check(m, &mut errors);
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
```

`crates/varn-tir/src/verify/wellformed.rs`:

```rust
//! Every handle points at something that exists; every slot is in range.

use super::VerifyError;
use crate::node::{TirExpr, TirExprKind, TirFunction, TirModule, TirStmt};
use crate::resolution::Resolution;
use crate::ty::BackendTy;

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    check_function(m, &m.top_level, errors);
    for f in &m.functions {
        check_function(m, f, errors);
    }
}

fn check_function(m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
    if m.signature(f.sig).is_none() {
        errors.push(VerifyError::new(
            format!("function `{}` names SigId({}), which has no entry", f.name, f.sig.0),
            crate::node::Span::EMPTY,
        ));
    }
    for s in &f.body {
        check_stmt(m, s, errors);
    }
}

fn check_stmt(m: &TirModule, s: &TirStmt, errors: &mut Vec<VerifyError>) {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => check_expr(m, e, errors),
        TirStmt::Let { init, .. } => {
            if let Some(e) = init {
                check_expr(m, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                check_expr(m, e, errors);
            }
        }
        TirStmt::If { cond, then_body, else_body } => {
            check_expr(m, cond, errors);
            for s in then_body.iter().chain(else_body) {
                check_stmt(m, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            check_expr(m, cond, errors);
            for s in body {
                check_stmt(m, s, errors);
            }
        }
        TirStmt::Try { body, catch_body, .. } => {
            for s in body.iter().chain(catch_body) {
                check_stmt(m, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue => {}
    }
}

fn check_expr(m: &TirModule, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    check_ty(m, e.ty, e, errors);
    check_res(m, e, errors);

    match &e.kind {
        TirExprKind::Binary { lhs, rhs, .. } => {
            check_expr(m, lhs, errors);
            check_expr(m, rhs, errors);
        }
        TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
            check_expr(m, operand, errors)
        }
        TirExprKind::Field { object, .. } => check_expr(m, object, errors),
        TirExprKind::Index { object, index } => {
            check_expr(m, object, errors);
            check_expr(m, index, errors);
        }
        TirExprKind::Call { callee, args } => {
            check_expr(m, callee, errors);
            for a in args {
                check_expr(m, a, errors);
            }
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            check_expr(m, recv, errors);
            for a in args {
                check_expr(m, a, errors);
            }
        }
        TirExprKind::Assign { target, value } => {
            check_expr(m, target, errors);
            check_expr(m, value, errors);
        }
        TirExprKind::ArrayLit(xs) | TirExprKind::TupleLit(xs) => {
            for x in xs {
                check_expr(m, x, errors);
            }
        }
        TirExprKind::ObjectLit { fields } => {
            for (_, v) in fields {
                check_expr(m, v, errors);
            }
        }
        TirExprKind::New { class, args } => {
            if m.class(*class).is_none() {
                errors.push(VerifyError::new(
                    format!("New names ClassId({}), which has no entry", class.0),
                    e.span,
                ));
            }
            for a in args {
                check_expr(m, a, errors);
            }
        }
        TirExprKind::MakeVariant { args } => {
            for a in args {
                check_expr(m, a, errors);
            }
        }
        TirExprKind::Select { cond, then_val, else_val } => {
            check_expr(m, cond, errors);
            check_expr(m, then_val, errors);
            check_expr(m, else_val, errors);
        }
        TirExprKind::IntLit(_)
        | TirExprKind::FloatLit(_)
        | TirExprKind::BoolLit(_)
        | TirExprKind::StrLit(_)
        | TirExprKind::CharLit(_)
        | TirExprKind::NullLit
        | TirExprKind::Var => {}
    }
}

/// Every handle inside a type points at an entry that exists.
fn check_ty(m: &TirModule, ty: BackendTy, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    let bad = |what: &str, errors: &mut Vec<VerifyError>| {
        errors.push(VerifyError::new(
            format!("type names {what}, which has no entry"),
            e.span,
        ));
    };
    match ty {
        BackendTy::Class(c) if m.class(c).is_none() => bad(&format!("ClassId({})", c.0), errors),
        BackendTy::Enum(en) if m.enum_info(en).is_none() => {
            bad(&format!("EnumId({})", en.0), errors)
        }
        BackendTy::Fn(s) if m.signature(s).is_none() => bad(&format!("SigId({})", s.0), errors),
        BackendTy::Array(t) | BackendTy::Set(t) | BackendTy::Nullable(t) => {
            if !m.types.contains(t) {
                bad(&format!("TyId({})", t.0), errors);
            }
        }
        BackendTy::Map(k, v) => {
            if !m.types.contains(k) {
                bad(&format!("TyId({})", k.0), errors);
            }
            if !m.types.contains(v) {
                bad(&format!("TyId({})", v.0), errors);
            }
        }
        BackendTy::Tuple(l) => {
            if !m.types.contains_list(l) {
                bad(&format!("TyListId({})", l.0), errors);
            }
        }
        _ => {}
    }
}

/// Every slot is in range of the table it claims to index.
fn check_res(m: &TirModule, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    let receiver_class = |recv: &TirExpr| match recv.ty.non_nullable(&m.types) {
        BackendTy::Class(c) => Some(c),
        _ => None,
    };

    match (&e.res, &e.kind) {
        (Resolution::FieldSlot(slot), TirExprKind::Field { object, .. }) => {
            match receiver_class(object) {
                Some(c) => {
                    let ok = m.class(c).and_then(|ci| ci.field_at(*slot)).is_some();
                    if !ok {
                        errors.push(VerifyError::new(
                            format!(
                                "field slot {slot} is out of range for class ClassId({})",
                                c.0
                            ),
                            e.span,
                        ));
                    }
                }
                None => errors.push(VerifyError::new(
                    format!("field slot {slot} on a receiver that is not a class"),
                    e.span,
                )),
            }
        }
        (Resolution::VtableSlot(slot), TirExprKind::MethodCall { recv, .. }) => {
            match receiver_class(recv) {
                Some(c) => {
                    let ok = m.class(c).and_then(|ci| ci.method_at(*slot)).is_some();
                    if !ok {
                        errors.push(VerifyError::new(
                            format!(
                                "vtable slot {slot} is out of range for class ClassId({})",
                                c.0
                            ),
                            e.span,
                        ));
                    }
                }
                None => errors.push(VerifyError::new(
                    format!("vtable slot {slot} on a receiver that is not a class"),
                    e.span,
                )),
            }
        }
        (Resolution::GlobalSlot(slot), _) => {
            if *slot as usize >= m.globals.len() {
                errors.push(VerifyError::new(
                    format!("global slot {slot} is out of range"),
                    e.span,
                ));
            }
        }
        (Resolution::DirectFn(f), _) => {
            if m.function(*f).is_none() {
                errors.push(VerifyError::new(
                    format!("DirectFn names FnId({}), which has no entry", f.0),
                    e.span,
                ));
            }
        }
        (Resolution::EnumVariant { enum_id, tag }, _) => {
            let ok = m
                .enum_info(*enum_id)
                .and_then(|ei| ei.variant_at(*tag))
                .is_some();
            if !ok {
                errors.push(VerifyError::new(
                    format!("EnumId({}) has no variant with tag {tag}", enum_id.0),
                    e.span,
                ));
            }
        }
        _ => {}
    }
}
```

Añade `mod verify;` y `pub use verify::{verify_module, VerifyError};` a `lib.rs`.

- [ ] **Step 4: Verificar que los tests pasan**

```bash
cargo test -p varn-tir
```

Esperado: todos PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/varn-tir/
git commit -m "feat(tir): reject a module whose handles or slots point nowhere

Nothing checks this today. ssa/verify.rs proves SSA form — single
definition, dominance — and does not touch types, so a field slot past
the end of a layout or a vtable index into a class with no methods
reaches the backend unremarked.

Errors are collected rather than returned on the first one: a single
missing case in the emitter usually produces many, and seeing them
together is what identifies the case."
```

---

## Task 8: Verificador — coherencia tipo ↔ operación

**Files:**
- Create: `crates/varn-tir/src/verify/coherence.rs`
- Modify: `crates/varn-tir/src/verify/mod.rs`
- Create: `crates/varn-tir/tests/verify_coherence.rs`

**Interfaces:**
- Consumes: `VerifyError`, `TirModule`, y las tablas.
- Produces: nada público nuevo; `verify_module` gana comprobaciones.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-tir/tests/verify_coherence.rs`:

```rust
//! A missing type costs performance. A WRONG type is a miscompile, and
//! nothing in the pipeline looks for one today. These checks are the trip
//! wire.

use std::rc::Rc;
use varn_tir::*;

fn module_with_point() -> TirModule {
    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Int);
    TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![ClassInfo::new(
            Rc::from("Point"),
            None,
            vec![("x".into(), BackendTy::Int), ("label".into(), BackendTy::Str)],
        )],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    }
}

fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr { kind, ty, res, span: Span::EMPTY }
}

fn int(v: i64) -> TirExpr {
    expr(TirExprKind::IntLit(v), BackendTy::Int, Resolution::None)
}

/// int + int is int.
#[test]
fn int_addition_is_int() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Add, lhs: Box::new(int(1)), rhs: Box::new(int(2)) },
        BackendTy::Int,
        Resolution::None,
    )));
    assert!(verify_module(&m).is_ok());
}

/// int + int claiming to produce Str is a miscompile, and is rejected.
#[test]
fn a_lying_result_type_is_rejected() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Add, lhs: Box::new(int(1)), rhs: Box::new(int(2)) },
        BackendTy::Str,
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Add")), "got: {:?}", errs);
}

/// Mixing representations without an explicit Cast is rejected: it is exactly
/// where a float silently travels in an integer register.
#[test]
fn mixed_operands_without_a_cast_are_rejected() {
    let mut m = module_with_point();
    let f = expr(TirExprKind::FloatLit(1.5), BackendTy::Float, Resolution::None);
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Add, lhs: Box::new(int(1)), rhs: Box::new(f) },
        BackendTy::Float,
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Cast")), "got: {:?}", errs);
}

/// A comparison produces Bool whatever its operands are.
#[test]
fn comparison_produces_bool() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Lt, lhs: Box::new(int(1)), rhs: Box::new(int(2)) },
        BackendTy::Int, // wrong on purpose
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Bool")), "got: {:?}", errs);
}

/// A field read's type must be the field's DECLARED type. This is the check
/// that makes a slot and a type disagreeing impossible.
#[test]
fn a_field_read_must_have_the_declared_type() {
    let mut m = module_with_point();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    // slot 0 is `x: int`, but the node claims Str.
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "x".into() },
        BackendTy::Str,
        Resolution::FieldSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("declared")), "got: {:?}", errs);
}

/// The same read with the right type passes.
#[test]
fn a_correct_field_read_verifies() {
    let mut m = module_with_point();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "x".into() },
        BackendTy::Int,
        Resolution::FieldSlot(0),
    )));
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-tir --test verify_coherence
```

Esperado: los cuatro tests de rechazo FALLAN — nada comprueba coherencia todavía.

- [ ] **Step 3: Implementar la coherencia**

`crates/varn-tir/src/verify/coherence.rs`:

```rust
//! Type ↔ operation coherence.
//!
//! A missing type costs performance; a wrong one is a miscompile. Nothing in
//! the pipeline looks for the second today.

use super::VerifyError;
use crate::node::{TirBinOp, TirExpr, TirExprKind, TirFunction, TirModule, TirStmt};
use crate::resolution::Resolution;
use crate::ty::BackendTy;

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    check_function(m, &m.top_level, errors);
    for f in &m.functions {
        check_function(m, f, errors);
    }
}

fn check_function(m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
    for s in &f.body {
        walk_stmt(m, s, errors);
    }
}

fn walk_stmt(m: &TirModule, s: &TirStmt, errors: &mut Vec<VerifyError>) {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => walk_expr(m, e, errors),
        TirStmt::Let { init, .. } => {
            if let Some(e) = init {
                walk_expr(m, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                walk_expr(m, e, errors);
            }
        }
        TirStmt::If { cond, then_body, else_body } => {
            walk_expr(m, cond, errors);
            for s in then_body.iter().chain(else_body) {
                walk_stmt(m, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            walk_expr(m, cond, errors);
            for s in body {
                walk_stmt(m, s, errors);
            }
        }
        TirStmt::Try { body, catch_body, .. } => {
            for s in body.iter().chain(catch_body) {
                walk_stmt(m, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue => {}
    }
}

fn walk_expr(m: &TirModule, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    match &e.kind {
        TirExprKind::Binary { op, lhs, rhs } => {
            walk_expr(m, lhs, errors);
            walk_expr(m, rhs, errors);
            check_binary(m, e, *op, lhs, rhs, errors);
        }
        TirExprKind::Field { object, .. } => {
            walk_expr(m, object, errors);
            check_field(m, e, object, errors);
        }
        TirExprKind::Index { object, index } => {
            walk_expr(m, object, errors);
            walk_expr(m, index, errors);
            check_index(m, e, object, errors);
        }
        TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
            walk_expr(m, operand, errors)
        }
        TirExprKind::Call { callee, args } => {
            walk_expr(m, callee, errors);
            for a in args {
                walk_expr(m, a, errors);
            }
            check_direct_call(m, e, args, errors);
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            walk_expr(m, recv, errors);
            for a in args {
                walk_expr(m, a, errors);
            }
        }
        TirExprKind::Assign { target, value } => {
            walk_expr(m, target, errors);
            walk_expr(m, value, errors);
        }
        TirExprKind::ArrayLit(xs) | TirExprKind::TupleLit(xs) => {
            for x in xs {
                walk_expr(m, x, errors);
            }
        }
        TirExprKind::ObjectLit { fields } => {
            for (_, v) in fields {
                walk_expr(m, v, errors);
            }
        }
        TirExprKind::New { args, .. } | TirExprKind::MakeVariant { args } => {
            for a in args {
                walk_expr(m, a, errors);
            }
        }
        TirExprKind::Select { cond, then_val, else_val } => {
            walk_expr(m, cond, errors);
            walk_expr(m, then_val, errors);
            walk_expr(m, else_val, errors);
            if then_val.ty != else_val.ty && e.ty != BackendTy::Dynamic(crate::ty::DynReason::Union)
            {
                errors.push(VerifyError::new(
                    "Select arms have different types and the result is not a union",
                    e.span,
                ));
            }
        }
        _ => {}
    }
}

fn is_comparison(op: TirBinOp) -> bool {
    matches!(
        op,
        TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
    )
}

fn check_binary(
    m: &TirModule,
    e: &TirExpr,
    op: TirBinOp,
    lhs: &TirExpr,
    rhs: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    if is_comparison(op) {
        if e.ty != BackendTy::Bool {
            errors.push(VerifyError::new(
                format!("comparison {op:?} must produce Bool, node says {:?}", e.ty),
                e.span,
            ));
        }
        return;
    }

    let l = lhs.ty.non_nullable(&m.types);
    let r = rhs.ty.non_nullable(&m.types);

    // Dynamic operands make the operation generic; nothing to prove.
    if matches!(l, BackendTy::Dynamic(_)) || matches!(r, BackendTy::Dynamic(_)) {
        return;
    }

    if l != r {
        errors.push(VerifyError::new(
            format!(
                "{op:?} mixes {:?} and {:?}; an explicit Cast is required",
                l, r
            ),
            e.span,
        ));
        return;
    }

    // int / int is the one arithmetic case whose result leaves the operand
    // class, and `varn_core::numeric` is where that rule lives.
    let expected = if op == TirBinOp::Div && l == BackendTy::Int {
        BackendTy::Float
    } else {
        l
    };

    if e.ty != expected {
        errors.push(VerifyError::new(
            format!(
                "{op:?} on {:?} produces {:?}, node says {:?}",
                l, expected, e.ty
            ),
            e.span,
        ));
    }
}

fn check_field(m: &TirModule, e: &TirExpr, object: &TirExpr, errors: &mut Vec<VerifyError>) {
    let Resolution::FieldSlot(slot) = e.res else {
        return;
    };
    let BackendTy::Class(c) = object.ty.non_nullable(&m.types) else {
        return; // well-formedness already reported this
    };
    let Some(field) = m.class(c).and_then(|ci| ci.field_at(slot)) else {
        return; // ditto
    };
    if e.ty != field.ty {
        errors.push(VerifyError::new(
            format!(
                "field `{}` is declared {:?}, node says {:?}",
                field.name, field.ty, e.ty
            ),
            e.span,
        ));
    }
}

fn check_index(m: &TirModule, e: &TirExpr, object: &TirExpr, errors: &mut Vec<VerifyError>) {
    if let BackendTy::Array(el) = object.ty.non_nullable(&m.types) {
        let elem = m.types.get(el);
        if e.ty != elem {
            errors.push(VerifyError::new(
                format!("indexing an array of {:?} produces {:?}, node says {:?}", elem, elem, e.ty),
                e.span,
            ));
        }
    }
}

fn check_direct_call(
    m: &TirModule,
    e: &TirExpr,
    args: &[TirExpr],
    errors: &mut Vec<VerifyError>,
) {
    let Resolution::DirectFn(f) = e.res else {
        return;
    };
    let Some(func) = m.function(f) else {
        return;
    };
    let Some(sig) = m.signature(func.sig) else {
        return;
    };
    if args.len() != sig.arity() {
        errors.push(VerifyError::new(
            format!(
                "call to `{}` passes {} arguments, signature takes {}",
                func.name,
                args.len(),
                sig.arity()
            ),
            e.span,
        ));
        return;
    }
    for (i, (a, p)) in args.iter().zip(&sig.params).enumerate() {
        if matches!(a.ty, BackendTy::Dynamic(_)) {
            continue;
        }
        if a.ty != *p {
            errors.push(VerifyError::new(
                format!(
                    "call to `{}`: argument {i} is {:?}, parameter is {:?}",
                    func.name, a.ty, p
                ),
                e.span,
            ));
        }
    }
    if e.ty != sig.return_ty && !matches!(e.ty, BackendTy::Dynamic(_)) {
        errors.push(VerifyError::new(
            format!(
                "call to `{}` returns {:?}, node says {:?}",
                func.name, sig.return_ty, e.ty
            ),
            e.span,
        ));
    }
}
```

En `verify/mod.rs`, añade `mod coherence;` y llama a `coherence::check(m, &mut errors);` justo después de `wellformed::check(...)`.

- [ ] **Step 4: Verificar que los tests pasan**

```bash
cargo test -p varn-tir
```

Esperado: todos PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/varn-tir/
git commit -m "feat(tir): a wrong type stops compiling

ssa/verify.rs proves SSA form and does not touch types, so nothing looks
for a type that is wrong rather than missing — a miscompile with no trip
wire, the same shape of debt AUDITORIA_DISENO flags around decode.

The checks pin the rules that already exist but were never enforceable:
a comparison produces Bool, mixing representations needs an explicit
Cast, int/int is the one arithmetic case that leaves its operand class,
and a field read has the field's declared type. That last one is what
makes a slot and a type disagreeing impossible."
```

---

## Task 9: El informe de cobertura

**Files:**
- Create: `crates/varn-tir/src/coverage.rs`
- Create: `crates/varn-tir/tests/coverage.rs`
- Modify: `crates/varn-tir/src/lib.rs`

**Interfaces:**
- Produces: `Coverage { nodes, dynamic_by_reason, by_name_by_reason, static_dispatch, name_dispatch }`, `Coverage::of(&TirModule) -> Coverage`, `Coverage::static_ratio() -> f64`, `Coverage::report() -> String`.

- [ ] **Step 1: Escribir el test que falla**

Crea `crates/varn-tir/tests/coverage.rs`:

```rust
//! The counter that says whether the work is advancing, as opposed to the
//! verifier, which says whether it is broken. A receiver whose class IS known
//! but which resolves by name is legal — it is an opportunity lost, not a
//! miscompile — so it is counted, not rejected.

use std::rc::Rc;
use varn_tir::*;

fn module() -> TirModule {
    TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![ClassInfo::new(Rc::from("P"), None, vec![("x".into(), BackendTy::Int)])],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    }
}

fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr { kind, ty, res, span: Span::EMPTY }
}

/// Dynamics are counted per reason, not as one number. An honest host
/// boundary and an inference hole read identically in a total.
#[test]
fn dynamics_are_counted_by_reason() {
    let mut m = module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Dynamic(DynReason::HostBoundary),
        Resolution::Local(LocalId(0)),
    )));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Dynamic(DynReason::Unannotated),
        Resolution::Local(LocalId(0)),
    )));

    let c = Coverage::of(&m);
    assert_eq!(c.dynamic_by_reason(DynReason::HostBoundary), 1);
    assert_eq!(c.dynamic_by_reason(DynReason::Unannotated), 1);
    assert_eq!(c.dynamic_by_reason(DynReason::Union), 0);
}

/// A known class resolved by name is legal and counted — that is the
/// difference between the verifier and this report.
#[test]
fn name_dispatch_on_a_known_class_is_counted_not_rejected() {
    let mut m = module();
    let recv = expr(TirExprKind::Var, BackendTy::Class(ClassId(0)), Resolution::Local(LocalId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "x".into() },
        BackendTy::Int,
        Resolution::ByName { name: "x".into(), why: DynReason::Unannotated },
    )));

    assert!(verify_module(&m).is_ok(), "a lost opportunity is not an error");
    let c = Coverage::of(&m);
    assert_eq!(c.name_dispatch, 1);
}

/// The ratio is what a regression gate compares across commits.
#[test]
fn the_static_ratio_is_reported() {
    let mut m = module();
    let recv = expr(TirExprKind::Var, BackendTy::Class(ClassId(0)), Resolution::Local(LocalId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "x".into() },
        BackendTy::Int,
        Resolution::FieldSlot(0),
    )));

    let c = Coverage::of(&m);
    assert!(c.static_ratio() > 0.0);
    assert!(c.report().contains("static"));
}
```

- [ ] **Step 2: Ejecutar y verificar que falla**

```bash
cargo test -p varn-tir --test coverage
```

Esperado: FALLA — `Coverage` no existe.

- [ ] **Step 3: Implementar `coverage.rs`**

```rust
//! How static the module actually is.
//!
//! The verifier says whether the module is broken. This says whether the work
//! is advancing — and with the corpus red, those are two different questions
//! that need two different instruments.

use crate::node::{TirExpr, TirExprKind, TirFunction, TirModule, TirStmt};
use crate::resolution::Resolution;
use crate::ty::{BackendTy, DynReason};

#[derive(Debug, Default, Clone)]
pub struct Coverage {
    pub nodes: u32,
    /// Indexed in the declaration order of `DynReason`.
    dynamics: [u32; 5],
    by_name: [u32; 5],
    pub static_dispatch: u32,
    pub name_dispatch: u32,
}

fn reason_index(r: DynReason) -> usize {
    match r {
        DynReason::HostBoundary => 0,
        DynReason::Union => 1,
        DynReason::IndexSignature => 2,
        DynReason::Unannotated => 3,
        DynReason::NotYetSupported => 4,
    }
}

const REASONS: [(DynReason, &str); 5] = [
    (DynReason::HostBoundary, "host boundary"),
    (DynReason::Union, "union"),
    (DynReason::IndexSignature, "index signature"),
    (DynReason::Unannotated, "unannotated"),
    (DynReason::NotYetSupported, "not yet supported"),
];

impl Coverage {
    pub fn of(m: &TirModule) -> Coverage {
        let mut c = Coverage::default();
        c.walk_function(&m.top_level);
        for f in &m.functions {
            c.walk_function(f);
        }
        c
    }

    pub fn dynamic_by_reason(&self, r: DynReason) -> u32 {
        self.dynamics[reason_index(r)]
    }

    pub fn by_name_by_reason(&self, r: DynReason) -> u32 {
        self.by_name[reason_index(r)]
    }

    /// Share of resolutions that dispatch statically. This is the number a
    /// regression gate compares between commits.
    pub fn static_ratio(&self) -> f64 {
        let total = self.static_dispatch + self.name_dispatch;
        if total == 0 {
            return 1.0;
        }
        self.static_dispatch as f64 / total as f64
    }

    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "nodes {}   static {}   by name {}   static {:.0}%\n",
            self.nodes,
            self.static_dispatch,
            self.name_dispatch,
            self.static_ratio() * 100.0
        ));
        for (r, label) in REASONS {
            let d = self.dynamic_by_reason(r);
            let n = self.by_name_by_reason(r);
            if d > 0 || n > 0 {
                s.push_str(&format!("  {label:<18} dynamic {d:>5}   by name {n:>5}\n"));
            }
        }
        s
    }

    fn walk_function(&mut self, f: &TirFunction) {
        for s in &f.body {
            self.walk_stmt(s);
        }
    }

    fn walk_stmt(&mut self, s: &TirStmt) {
        match s {
            TirStmt::Expr(e) | TirStmt::Throw(e) => self.walk_expr(e),
            TirStmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.walk_expr(e);
                }
            }
            TirStmt::Return(v) => {
                if let Some(e) = v {
                    self.walk_expr(e);
                }
            }
            TirStmt::If { cond, then_body, else_body } => {
                self.walk_expr(cond);
                for s in then_body.iter().chain(else_body) {
                    self.walk_stmt(s);
                }
            }
            TirStmt::Loop { cond, body } => {
                self.walk_expr(cond);
                for s in body {
                    self.walk_stmt(s);
                }
            }
            TirStmt::Try { body, catch_body, .. } => {
                for s in body.iter().chain(catch_body) {
                    self.walk_stmt(s);
                }
            }
            TirStmt::Break | TirStmt::Continue => {}
        }
    }

    fn walk_expr(&mut self, e: &TirExpr) {
        self.nodes += 1;

        if let BackendTy::Dynamic(r) = e.ty {
            self.dynamics[reason_index(r)] += 1;
        }

        match &e.res {
            Resolution::None => {}
            Resolution::ByName { why, .. } => {
                self.name_dispatch += 1;
                self.by_name[reason_index(*why)] += 1;
            }
            _ => self.static_dispatch += 1,
        }

        match &e.kind {
            TirExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs);
                self.walk_expr(rhs);
            }
            TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
                self.walk_expr(operand)
            }
            TirExprKind::Field { object, .. } => self.walk_expr(object),
            TirExprKind::Index { object, index } => {
                self.walk_expr(object);
                self.walk_expr(index);
            }
            TirExprKind::Call { callee, args } => {
                self.walk_expr(callee);
                for a in args {
                    self.walk_expr(a);
                }
            }
            TirExprKind::MethodCall { recv, args, .. } => {
                self.walk_expr(recv);
                for a in args {
                    self.walk_expr(a);
                }
            }
            TirExprKind::Assign { target, value } => {
                self.walk_expr(target);
                self.walk_expr(value);
            }
            TirExprKind::ArrayLit(xs) | TirExprKind::TupleLit(xs) => {
                for x in xs {
                    self.walk_expr(x);
                }
            }
            TirExprKind::ObjectLit { fields } => {
                for (_, v) in fields {
                    self.walk_expr(v);
                }
            }
            TirExprKind::New { args, .. } | TirExprKind::MakeVariant { args } => {
                for a in args {
                    self.walk_expr(a);
                }
            }
            TirExprKind::Select { cond, then_val, else_val } => {
                self.walk_expr(cond);
                self.walk_expr(then_val);
                self.walk_expr(else_val);
            }
            _ => {}
        }
    }
}
```

Añade `mod coverage;` y `pub use coverage::Coverage;` a `lib.rs`.

- [ ] **Step 4: Verificar que los tests pasan**

```bash
cargo test -p varn-tir
```

Esperado: todos PASS.

- [ ] **Step 5: Verificar que nada más se rompió**

El crate no lo importa nadie, así que el resto del workspace debe compilar y el corpus seguir verde:

```bash
cargo build --release --workspace
target/release/vn.exe run tests/main.vn
```

Esperado: build limpia y todos los `[PASSED]`.

- [ ] **Step 6: Commit**

```bash
git add crates/varn-tir/
git commit -m "feat(tir): count how static the module is, and why it is not

The verifier answers whether the module is broken. This answers whether
the work is advancing, and with the corpus red those need separate
instruments.

Dynamics and name-keyed resolutions are counted per reason rather than
summed, because a host boundary and an inference hole read identically
in a total — which is why '1037 name-keyed property reads' has never
been an actionable number."
```

---

## Cierre de la etapa 1

Al terminar la Task 9:

- `cargo test -p varn-tir` — todos los tests pasan.
- `cargo build --release --workspace` — limpio.
- `target/release/vn.exe run tests/main.vn` — verde.
- `crates/varn-tir/` existe, nadie lo importa, y `Cargo.lock` lo registra.

Lo que queda listo para la etapa 2: `BackendTy`, `Resolution`, los nodos, las tablas con **una** autoridad de layout y vtable, el verificador en dos niveles y el contador de cobertura. La etapa 2 —el checker emite TIR en paralelo al camino viejo— se planifica cuando esta API esté estable, porque la forma de la emisión depende de ella.

**Lo que la spec pide y este plan deliberadamente no incluye:** los comandos `vn debug -p tir` y `-p tir:check` de §7.4. Volcar y verificar necesitan un TIR real que volcar, y no lo hay hasta que el checker emita. Van con la etapa 2, en el mismo commit que produzca el primer módulo emitido — que es cuando el informe de cobertura deja de ser un test unitario y pasa a ser el instrumento de la rama.

**Antes de empezar la etapa 2**, lee `docs/TIR_CONTRATO_TIPADO.md` §11: la condición que decide si el enfoque funciona es que el TIR salga desazucarado. `TirStmt::Loop` es la única forma de bucle a propósito. Si al emitir aparece la necesidad de un nodo `ForOf` o `Match`, el TIR no está desazucarado, `hir/` no se podrá borrar, y hay que parar y replantear en vez de añadir el nodo.
