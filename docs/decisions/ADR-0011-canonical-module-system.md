# ADR-0011: One Canonical Module System (single loader, single resolution, single source of truth)

## Status
Accepted. Supersedes the parallel per-crate loading paths it removes.

## Context

"Find a module from a specifier and read it" is implemented **four times**, with
different types, different resolution rules, and different representation choices:

1. **VM**: `trait ModuleLoader { resolve, load -> FunctionProto, native }` plus
   `CompositeLoader` (`crates/varn-vm/src/loader.rs`), with `FileLoader` and
   `StdlibLoader` in `crates/varn-pipeline/src/stdlib_loader.rs`.
2. **Checker**: `trait ImportResolver { module_bind, stdlib_bind, module_exports,
   resolve_specifier }` (`crates/varn-checker/src/module_resolver/resolver.rs`), with
   `Carrier::{Blob, Embedded, File}` (`.../module_resolver/stdlib.rs`).
3. **Assets/provider**: `varn_modules::provider` (`interface_blob`, `embedded_source`,
   `source_path`, `bundled_source`) plus `std_root`.
4. **CLI/LSP/build**: `read_source_file`, `ImportSpecifier::parse`,
   `varn_modules::resolver::ModuleResolver`, `register_provider`.

Every one of them can pick a different representation for the *same* `ModuleId`. That
is not theoretical: the checker once served `std:` modules from a precompiled **blob**
while the VM compiled the **source**, so the same module had two different type sets.
The earlier F1 work removed the symptom (prefer source) but left the duplication in
place.

The cost is structural:
- Two modules with one identity can be *different modules* depending on who asks.
- A fix in one path (e.g. portable types) must be repeated in the others or diverges.
- Over-engineering: `Carrier`, two `ModuleLoader`s, a provider trait, and ad-hoc
  resolution, all doing the same job in different vocabulary.

## Decision

**`varn-modules` is the module system.** It already depends only on `varn-core`, and
`varn-checker`, `varn-vm`, `varn-pipeline`, `varn-cli`, and `varn-lsp` all already
depend on it, so the contract lives there with no cycles.

### 1. One identity
`varn_core::ModuleId` is the only module identity. There is no second key (path,
`virtual_id`, specifier string, blob index). Resolution returns a `ModuleId`;
every cache, every artifact, every request is keyed by `ModuleId` (+ source
fingerprint when the file may change).

### 2. One resolution
`resolve(specifier: &str, from: &ModuleId) -> Result<ModuleId, LoadError>` is owned by
`varn-modules::resolver`. Relative paths, packages, `std:`/`core:`/`runtime:`, and
native ids all go through the same function. No phase computes paths on its own.

### 3. One load
```rust
pub struct ModuleSource {
    pub id: ModuleId,
    pub text: Rc<str>,
    pub provenance: Provenance,          // File(path) | Embedded | Bundle | Memory | Native
    pub interface: Option<Rc<[u8]>>,     // precompiled checker interface, if any
    pub bytecode: Option<Rc<[u8]>>,      // precompiled FunctionProto, if any
}

pub trait ModuleLoader {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, LoadError>;
    fn source(&self, id: &ModuleId) -> Result<ModuleSource, LoadError>;
}
```
`source` returns the module's **text** and, optionally, precomputed artifacts. It never
compiles. Compilation is a separate service (`ModuleCompiler`) that keys on
`(ModuleId, fingerprint)`; the checker binds/checks, the compiler lowers, the VM runs —
each consumes the loader and produces its own artifact.

### 4. One registry of backends
Representations are **backends behind one registry**, tried in a fixed, explicit order:
1. `MemoryLoader` (LSP unsaved buffers) — highest priority.
2. `PackageLoader` / `FilesystemLoader` (`ModuleId::Local`).
3. `BundleLoader` (embedded std bundle: text + interface + bytecode).
4. `BuiltinsProviderLoader` (`core:`/`runtime:` sources and native ids).

The order and the fallbacks exist in exactly one place. A backend cannot be reached
"around" the registry.

### 5. Invariant (enforced, not hoped)
> For a given `ModuleId`, the checker and the VM are served by the same loader and see
> the same `text` (or the same precompiled artifact). A phase never re-resolves or
> re-reads on its own.

`Provenance` is carried so diagnostics can say *where* a module came from; it is not a
branching key.

## Consequences

### Deleted (the whole point)
- `varn-checker::module_resolver::Carrier` and `stdlib_carrier`.
- Direct `varn_modules::provider::{interface_blob, embedded_source, bundled_source,
  source_path}` calls outside `varn-modules` backends.
- `varn-vm::loader::trait ModuleLoader` and `CompositeLoader`; the VM takes the
  registry plus a compile callback.
- `varn-pipeline::stdlib_loader::{FileLoader, StdlibLoader}` as independent resolvers;
  they become thin adapters (or disappear into the registry).
- `varn-lsp`/`varn-cli` resolution helpers that duplicate `varn_modules::resolver`.
- Per-phase source caches; one cache per `(ModuleId, fingerprint)`.

### Kept / strengthened
- `ModuleId` in `varn-core` (unchanged).
- The `ImportSpecifier` parser and `ModuleResolver` in `varn-modules` (become the only
  resolution path).
- Precompiled interfaces/bytecode as **optional fields of `ModuleSource`**, so a bundle
  can still skip parsing and compiling — but through the same door as source.

## Migration steps (each one part of the final design, none discarded)

1. **Contract + backends** in `varn-modules`: `LoadError`, `ModuleSource`,
   `ModuleLoader`, `ModuleRegistry`, `FilesystemLoader`, `MemoryLoader`, unit-tested.
   (Touches no consumer; pure addition of the single door.)
2. **Checker adopts the registry**: `ImportResolver` is implemented by an adapter over
   `ModuleLoader`; `Carrier` and direct provider calls go away.
3. **VM/pipeline adopt the registry**: the VM's loader trait and
   `FileLoader`/`StdlibLoader` are removed; the VM takes the registry + compiler.
4. **CLI/LSP/build construct one registry** at one place; duplicate resolution deleted.
5. **One artifact cache** keyed by `(ModuleId, fingerprint)` shared by checker and
   compiler.

## Alternatives rejected
- **Keep the four paths, add adapters between them**: explicitly forbidden by `AGENTS.md`
  Ley 8. An adapter that lets the old design survive is the bug.
- **Move the contract to `varn-core`**: works, but `varn-core` is the AST/span leaf;
  `varn-modules` already owns ids/providers/artifacts and everyone depends on it.
- **Have the VM keep returning compiled protos from `load`**: couples loading to
  compiling and forces a second loader for the checker. Separation is the point.
