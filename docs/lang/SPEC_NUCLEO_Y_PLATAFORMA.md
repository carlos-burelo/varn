> Precisión parcial. Verificado contra tests/... Código manda.

# Varn — Núcleo y plataforma (índice)

Documento histórico de 3000 líneas reducido a índice. Era propuesta arquitectónica,
no spec vigente. La mayoría de sus §§ (StringView, Span, intersecciones, literales
como tipos, Unit, Type\<T\>, unsafe, SIMD, WASM, FFI, capas std:*) no aparece en
`tests/*.vn` (cero matches de `StringView`, `Span<`, `struct`, `unsafe` en tests/).
Lo no citado abajo es NO-VERIFICADO.

Verificado (contrato real, detalle en `types.md` / `specification.md`):

- 4 tipos numéricos, sin anchos físicos públicos: `int, float, bigint, decimal`.
- `int` = entero 64 bits con signo; `+ - * / % **` lanzan `IntegerOverflow`
  fuera de rango (`tests/120-int-division.vn:25`, `crates/varn-core/src/numeric_conv.rs:115`).
- `int / int` = `int` truncado hacia cero; `x / 0` lanza `DivisionByZero`
  (`tests/120-int-division.vn:16-24`).
- Conversiones explícitas con `as` (`tests/122-numeric-conversions.vn:3-4,44-61`).
- `T?` = `T | null` (`tests/30-nullable-types.vn:3`); uniones `A | B`
  (`tests/15-unions.vn:2`); enums simples, con payload, genéricos
  (`tests/16-enums.vn:2`, `tests/41-advanced-enums.vn`).
- `Array<T>` ≡ `T[]`; tuplas `#[…]` y records `#{…}` inmutables con `==`
  estructural (`tests/69-tuples-records.vn:4,22`).
- `dynamic` como frontera explícita (`tests/79-control-flow-narrowing.vn:32-34`,
  `tests/122-numeric-conversions.vn:47-48`).

Contratos entre fases y runtime: `../COMPILER_ARCHITECTURE.md`,
`../RUNTIME_ARCHITECTURE.md`, `../VM_ARCHITECTURE.md`, `../MODULES_SYSTEM.md`.
