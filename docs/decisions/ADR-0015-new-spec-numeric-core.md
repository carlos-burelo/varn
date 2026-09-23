# ADR 0015: Núcleo semántico de NEW_SPEC — números, conversiones y errores

## Estado
Aceptada (2026-09-23). Reemplaza la sección 2 de ADR-0004.

## Contexto
`docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md` fija cuatro tipos numéricos públicos
y prohíbe conversiones implícitas con pérdida. El código aceptaba `int → float`
implícito, `int / int → float`, ocho anchos angostos públicos, y tenía
crashes y errores silenciosos (ver `docs/plans/2026-09-23-new-spec-roadmap.md` §2.1).

## Decisiones
- D1 `for (const x of e)` se mantiene; `for x in e` del spec es ilustrativo.
- D2 `Symbol` sale del núcleo (Fase C).
- D3 `float as int` trunca hacia cero; NaN/±Inf/fuera de rango → `IntegerOverflow`.
- D4 Implícitas: solo `int → bigint`, `int → decimal`.
- D5 `int → float` y `float ↔ decimal` requieren `as`.
- D6 Un literal entero adopta `float`/`decimal`/`bigint` del contexto si es
  exactamente representable (`|v| ≤ 2^53` para `float`).
- D7 `dynamic` conserva `int ⊕ float → float`; `int / int` es `int` en todo tier.
- D8 `int / int` trunca; `/ 0` y `% 0` → `DivisionByZero`; `MIN / -1` →
  `IntegerOverflow`; `MIN % -1 == 0`.
- D9 `float / 0.0` y `float % 0.0` siguen IEEE 754.
- D10 `IntegerOverflow` y `DivisionByZero` son clases `extends Error`.
- D11 `ArrayRepr` angosto se borra; vuelve solo como optimización probada (Fase F).
- D12 `bigint`/`decimal` de precisión arbitraria (Fase B, ADR-0016).
- D13 API de enteros: `wrapping*`, `saturating*`, `checked*` (→ `int?`), `div`,
  `floorDiv`, `ceilDiv`, `rem`, `mod`.

## Consecuencias
- Se borran `i8 i16 i32 u8 u16 u32 u64 f32` de lexer, checker, TIR, SSA, VM y JIT.
- Programas con `int / int` que esperaban `float` cambian de resultado; se
  migran en el mismo commit que cambia la regla.
- Ganancia (Ley 10): corrección (crashes y silencios eliminados), una sola
  representación por tipo y menos código de backend.
