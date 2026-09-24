# ADR 0016: `bigint` y `decimal` de precisión arbitraria

## Estado
Aceptada (2026-09-23). Implementa la decisión D12 de ADR-0015.

## Contexto
`docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md` §6 y §8 definen `bigint` y `decimal`
como dominios de precisión arbitraria. Hoy `bigint` es `i128` (un literal de
40 dígitos es error VN3021) y `decimal` es `rust_decimal` (mantisa de 96 bits,
28 dígitos).

## Decisiones
- `bigint` = `num_bigint::BigInt` (0.4). Aritmética exacta; `/` trunca hacia
  cero y `%` toma el signo del dividendo, igual que `int`; `/ 0` y `% 0`
  lanzan `DivisionByZero`; `**` con exponente negativo lanza.
- `decimal` = `bigdecimal::BigDecimal` (0.4). `+ - *` exactos. `/` redondea a
  **34 dígitos significativos, half-even** (precisión de IEEE decimal128) —
  una división exacta (`1/4`) no pierde nada; una periódica (`1/3`) queda
  determinista. `%` es exacto, con el signo del dividendo.
- Igualdad y hash de `decimal` son numéricos: `1.0d == 1.00d`.
- Conversiones `as` nuevas (tabla `NumConv`): `bigint ↔ float`,
  `decimal ↔ float`, `bigint ↔ decimal`. De `float`, `NaN`/`±Inf` lanzan
  `IntegerOverflow` hacia `bigint` (no hay entero que los represente) y
  `Error` hacia `decimal`; `decimal`/`bigint → float` redondea al más cercano
  (`±Infinity` si excede).
- Literales: el texto del literal viaja sin truncar (TIR, SSA, pool); se
  borra el límite de 128 bits (VN3021 para `n`).
- `rust_decimal` sale del workspace (Ley 8: una sola representación).

## Consecuencias
- Ganancia: corrección (dominios completos según spec), una sola
  representación por dominio.
- Coste: `bigint`/`decimal` siempre en heap (ya lo eran). `int` no se ve
  afectado: sigue siendo i64 en registro.
