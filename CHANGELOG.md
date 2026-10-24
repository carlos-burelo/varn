# Changelog

Formato basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/).
Versionado: ver [`docs/STABILITY.md`](docs/STABILITY.md).

## [Unreleased]

### Changed (incompatible)

- `+` needs two strings or two numbers: `"n=" + n` is `VN3010`, use a template literal (`` `n=${n}` ``).
- `x as T` between unrelated types is `VN3014`. Unions, arrays, numbers and int enums keep working; a conversion out of a generic `T` goes through `dynamic`.
- `this` outside a class, enum or extension is `VN4115`.
- Interface properties need a type annotation.
- `get`/`set` accessors in object literals are a syntax error: declare a class (RFC 0001).
- Calling a value whose interface declares no call signature is `VN3003`.
- The right operand of `in` cannot be a scalar.
- Reading a property of an `int`, `float` or `bool` at runtime is an error instead of `null`.

### Fixed

- `using a = x, b = y` binds both declarators.
- A `match` guard such as `n if ok =>` no longer reads `ok =>` as an arrow function.
- `x is T` is typed `bool` and `(a, b)` takes the type of `b`; both were `dynamic`.

Desde `v0.1.0` (2026-09-12) hay más de mil commits sin release. Las decisiones de
diseño que cambiaron el lenguaje o el runtime están en [`docs/decisions/`](docs/decisions/).
Antes de publicar `0.2.0`, esta sección se reescribe agrupada en `Added`, `Changed`,
`Removed` y `Fixed` con los cambios visibles para el usuario.

## [0.1.0] - 2026-09-12

Primera versión etiquetada.
