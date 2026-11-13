# Changelog

Formato basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/).
Versionado: ver [`docs/STABILITY.md`](docs/STABILITY.md).

## [Unreleased]

### Changed (incompatible)

- Crates, environment variables, the manifest and the lockfile are named for the `vn` program: the crates are `vn-*` (`crates/vn-core`, `vn_core`), the variables are `VN_*` (`VN_NO_JIT`, `VN_STD`, `VN_CACHE_DIR`), the manifest is `vn.toml` and the lockfile `vn.lock`, and the language server's commands are `vn.*` with `vn` as its language id, configuration section and diagnostic source.
- `+` needs two strings or two numbers: `"n=" + n` is `VN3010`, use a template literal (`` `n=${n}` ``).
- `x as T` between unrelated types is `VN3014`. Unions, arrays, numbers and int enums keep working; a conversion out of a generic `T` goes through `dynamic`.
- `this` outside a class, enum or extension is `VN4115`.
- Interface properties need a type annotation.
- `get`/`set` accessors in object literals are a syntax error: declare a class (RFC 0001).
- Calling a value whose interface declares no call signature is `VN3003`.
- The right operand of `in` cannot be a scalar.
- Reading a property of an `int`, `float` or `bool` at runtime is an error instead of `null`.

- The manifest is `vn.toml` only, in one flat shape. `[package]` and `[project]` are gone and an unknown key is an error. `vn init` writes the flat form.
- `vn build --native` is gone: use `--target native` (`--target` is now `bytecode`, `exe` or `native`).
- `vn` is always built with the language server, the debug adapter and the package manager; the `lsp`, `dap` and `pm` features no longer exist.
- `VN_DEBUG_OPS` is replaced by `vn debug --natives`.
- A path given to `vn fmt` that does not exist is an error, and a standalone executable whose trailer cannot be read is an error instead of starting the normal CLI.
- The std image has a `templates` section per module; images built by an earlier binary are rebuilt.
- `vn pkg` installs dependencies in alphabetical order and reports an unreadable dependency manifest instead of skipping its dependencies.
- `vn fmt` and `vn test` fail on a path that does not exist, and `vn fmt` fails on a directory it cannot read.
- `vn.toml` accepts `description`, `license`, `repository`, `homepage`, `authors` and `keywords` besides the fields listed in the CLI reference; every other unknown key is an error.
- `vn debug`, `vn eval`, `vn repl` and `vn test` accept the same `--allow-*` and `--sandbox` flags as `vn run`, and a bad `--phase` spec exits with the usage code.

### Added

- Imported functions are linked at compile time. A module is compiled before the modules that import it and publishes inline packages: small self-contained exported functions (no classes, closures, `this` or mutable module state), the helpers they call, and exported constants with a literal value. An importer calls such a function directly, so the compiler can inline it, and reads such a constant as its literal. A module whose top level only assigns literals and whose imports are all linked this way is not loaded. `std` modules publish their packages in `std.vnb`.
- `@decodes("<op>")` in a native contract declares the typed variant of a native function (`runtime:json`, `parse$`); a `std` function that only forwards to a native, as a `return` or as a bare call statement, is resolved to that native at the call site.
- `vn debug` renders the machine code and the Cranelift IR of a module with the source line of each instruction, and reports when the VM would compile each function; the language server adds the `vn.showMachineCode` and `vn.showClif` commands and a View Machine Code lens.
- `[fmt]` and `[test]` sections in `vn.toml` with `include`/`exclude` patterns for the files `vn fmt` and `vn test` visit; `vn bench` accepts the permission flags.
- `vn task`: run the command lines declared under `[tasks]` in `vn.toml`.
- `vn build --target exe`: a self-contained executable made of the running `vn`, the std image it uses and the compiled module graph.
- `.env` files and the `std` key of `vn.toml` come from the project of the file or directory the command receives, not from the working directory; `vn pkg` works on the project that owns the installed packages, and an invalid `vn.toml` in an ancestor directory is an error that names the file.
- `vn check` takes any number of files and directories (none: the `main` of `vn.toml`) and type-checks each one with all its dependencies without generating code; emission errors only show up in `vn run` and `vn build`.
- `.vn/.env`, the file `vn init` creates, is now read with the other `.env` files.

### Performance

- Typed `Json.parse<T>` parses straight into the declared type; JSON objects are parsed by predicting the cached shape of the previous one; floats with up to four decimals are written without `ryu`.
- `new Map`, `new Set`, `new Array` and map literals are built through native constructor operations or in place; functions imported from native modules are called by operation id.
- Frameless self-calls keep depth and stack floor in registers; call arguments are computed straight into the call window; comparisons against an immediate have their own opcodes.
- A native callback into a compiled closure goes through a host thunk, and `instanceof` answers an instance of exactly the class without walking the registry.
- `Array.sort` checks the comparator contract against each element once before sorting, not on every comparison; a comparator that rejects an element now fails before the first comparison.

### Fixed

- A `/=` where a value is expected is a regex, every letter after a regex literal is a flag, and a repeated flag is an error.
- Reserved words used as keys or values, and `null`, get the semantic tokens of an identifier and a number in the language server.
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
