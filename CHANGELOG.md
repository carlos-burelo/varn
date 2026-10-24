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

- The manifest is `varn.toml` only, in one flat shape. `vn.toml`, `[package]` and `[project]` are gone and an unknown key is an error. `vn init` writes the flat form.
- `vn build --native` is gone: use `--target native` (`--target` is now `bytecode`, `exe` or `native`).
- `vn` is always built with the language server, the debug adapter and the package manager; the `lsp`, `dap` and `pm` features no longer exist.
- `VARN_DEBUG_OPS` is replaced by `vn debug --natives`.
- A path given to `vn fmt` that does not exist is an error, and a standalone executable whose trailer cannot be read is an error instead of starting the normal CLI.
- `vn pkg` installs dependencies in alphabetical order and reports an unreadable dependency manifest instead of skipping its dependencies.
- `vn fmt` and `vn test` fail on a path that does not exist, and `vn fmt` fails on a directory it cannot read.
- `varn.toml` accepts `description`, `license`, `repository`, `homepage`, `authors` and `keywords` besides the fields listed in the CLI reference; every other unknown key is an error.
- `vn debug`, `vn eval`, `vn repl` and `vn test` accept the same `--allow-*` and `--sandbox` flags as `vn run`, and a bad `--phase` spec exits with the usage code.

### Added

- `[fmt]` and `[test]` sections in `varn.toml` with `include`/`exclude` patterns for the files `vn fmt` and `vn test` visit; `vn bench` accepts the permission flags.
- `vn task`: run the command lines declared under `[tasks]` in `varn.toml`.
- `vn build --target exe`: a standalone executable made of the running `vn` and the compiled module graph.
- `.vn/.env`, the file `vn init` creates, is now read with the other `.env` files.

### Fixed

- `implements` of a generic class is checked structurally: `class C implements Box<int>` with `get(): str` is `VN4117`.
- Hovering a member of a generic class declared in another module no longer panics the language server.
- `using a = x, b = y` binds both declarators.
- A `match` guard such as `n if ok =>` no longer reads `ok =>` as an arrow function.
- `x is T` is typed `bool` and `(a, b)` takes the type of `b`; both were `dynamic`.

Desde `v0.1.0` (2026-09-12) hay más de mil commits sin release. Las decisiones de
diseño que cambiaron el lenguaje o el runtime están en [`docs/decisions/`](docs/decisions/).
Antes de publicar `0.2.0`, esta sección se reescribe agrupada en `Added`, `Changed`,
`Removed` y `Fixed` con los cambios visibles para el usuario.

## [0.1.0] - 2026-09-12

Primera versión etiquetada.
