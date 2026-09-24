# Fase B — `bigint` y `decimal` de precisión arbitraria

> Ejecutar tarea a tarea; cada tarea termina con GATE-SUITE (main.vn en JIT y
> `VARN_NO_JIT=1`, caché limpio) y un commit. Decisiones: ADR-0016.

**Goal:** `bigint` = `num_bigint::BigInt`, `decimal` = `bigdecimal::BigDecimal`,
con aritmética, comparación, hash, conversión, serialización y literales
completos; `rust_decimal` fuera del workspace.

**Hallazgo previo (bug):** hoy `10n + 5n` imprime `0` y `10n / 5n` imprime
`NaN`: `varn-vm/src/exec/arith.rs` no tiene rama `bigint` y cae a
`to_f64_val`, que devuelve `0.0` para un heap no numérico.

## Global Constraints
- Spec §6–§10; ADR-0015 (D4: `int → bigint`/`int → decimal` implícitos) y ADR-0016.
- Leyes de `AGENTS.md`; archivos ≤ 400 líneas para lo nuevo; nunca
  `git add <directorio>`.
- `varn-tir` no gana dependencias: los literales viajan como texto canónico
  base 10 (`Arc<str>`) hasta el pool de constantes.

## Tareas

### B.1 `bigint` arbitrario
- `varn-core/src/numeric_big.rs` (nuevo): `parse_bigint_literal(&str) -> Option<BigInt>`
  (prefijos `0x`/`0o`/`0b`, `_`), `div_big`, `rem_big` con `IntDivFault`.
- `Value::BigInt(Box<BigInt>)`, `SendValue::BigInt(BigInt)`,
  `Literal::BigInt(BigInt)` (serde), `HeapObj::BigInt(Box<BigInt>)`,
  `bigint_interner: FxHashMap<BigInt, u32>`.
- `TirExprKind::BigIntLit(Arc<str>)`, `InstKind::ConstBigInt(Arc<str>)`, clave CSE.
- Checker: se borra el límite de 128 bits (VN3021 para `n`); emit parsea con
  `parse_bigint_literal` y emite texto canónico.
- VM `arith.rs`: `bigint_pair` (bigint⊕bigint, bigint⊕int) en `+ - * / % **`
  y `-x`; `compare.rs` igualdad/orden con `int`.
- Conversiones `BigIntToInt`/`IntToBigInt` sobre `BigInt`.
- `builtins/primitives/bigint`: métodos portados.
- Tests: `tests/124-bigint-arbitrary.vn` (aritmética, > 2^127, división,
  errores, comparación con `int`, mapas con claves `bigint`); fixture
  `tests/errors/invalid-bigint-overflow.vn` se borra (ya no es error).

### B.2 `decimal` arbitrario
- `Value::Decimal(Box<BigDecimal>)` etc.; `rust_decimal` fuera de
  `Cargo.toml` del workspace y de cada crate.
- Lexer/token: literal `d` como texto; `TirExprKind`/`InstKind` con `Arc<str>`.
- `arith.rs`: `/` = 34 dígitos significativos half-even
  (`varn_core::numeric_big::div_decimal`), `%` exacto, `/ 0` → `DivisionByZero`.
- Igualdad/hash numéricos (`1.0d == 1.00d`), formato `toString` sin
  exponente.
- `builtins/primitives/decimal`: `toFixed`, `floor`, `ceil`, `round`, `trunc`,
  `abs`, `negate`, `isZero`, `isPositive`, `isNegative`, `parse` portados.
- Tests: ampliar `tests/35-decimal-bigint.vn` con > 28 dígitos y `1d/3d`.

### B.3 Conversiones entre dominios
- `NumConv`: `BigIntToFloat`, `FloatToBigInt`, `DecimalToFloat`,
  `FloatToDecimal`, `BigIntToDecimal`, `DecimalToBigInt`; VM + const-fold.
- Tests en `tests/122-numeric-conversions.vn`.

### B.4 Docs
- `docs/lang/types.md` §1, §9; `standard_library.md` (`bigint`, `decimal`);
  roadmap §0.
