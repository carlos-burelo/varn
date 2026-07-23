# `vn debug -p clif` — Cranelift Introspection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `vn debug -p clif` phase that shows, per function and without running the program, the Cranelift backend's ROUTE/BAIL decision + reason, the kind lattice, the textual CLIF IR, and the x86-64 disassembly.

**Architecture:** `varn-jit` gains a `clif::debug::inspect()` that reuses `try_compile` (threading an optional capture sink — production passes `None`, zero drift). `varn-vm` exposes a DRY `build_jit_helpers()` (extracted from the inline literal in `compile_jit`) so inspection uses production-faithful helper addresses without a live VM. `varn-debug` renders the four views (owning an `iced-x86` disassembler), and `varn-pipeline` wires the phase next to the existing `bytecode` dispatch.

**Tech Stack:** Rust, cranelift-codegen 0.125.4 (`Function::display()`), iced-x86 1.x (machine disasm), the existing `varn-debug` phase framework.

## Global Constraints

- File size governance: no file may exceed **1000 lines**; measure with `(Get-Content <file>).Count` (PowerShell `Measure-Object -Line` **undercounts by ~44** — do not use it).
- Validation gold (run after every behavior-affecting change): `./target/release/vn.exe run ./tests/main.vn` = **701**; `VARN_NO_CLIF=1 … run` = **701**; `VARN_NO_JIT=1 … run` = **763**. Outputs byte-identical across tiers.
- Purge the `.vnc` cache before validating compiler/JIT changes (it is keyed by source hash only): `Remove-Item -Recurse -Force $env:LOCALAPPDATA\varn\cache -ErrorAction SilentlyContinue` (or the platform cache dir; `vn cache clear` if available).
- Rebuild the std bundle after every `vn` rebuild, or `vn` aborts: `cargo run --release --bin xtask -- build-std`.
- Work on `main` only. No feature branches. Frequent commits.
- `int` = i48 wrapping; object identity = `Rc` address; instruction shapes come only from `varn_types::bytecode::decode`. Do not touch these.
- The dev machine is thermally throttled — release builds take **5–22 min**. Do not interpret a slow build as a hang.

---

### Task 1: Extract `build_jit_helpers()` in varn-vm (DRY)

Pull the `JitHelpers { … }` struct literal out of `Closure::compile_jit` into a free function so both production and the new debug path build identical, real helpers. Pure move — no logic change.

**Files:**
- Modify: `crates/varn-vm/src/frame.rs` (the `JitHelpers { … }` literal inside `compile_jit`, ~lines 164–236)

**Interfaces:**
- Produces: `pub fn build_jit_helpers() -> varn_jit::JitHelpers` (module path `varn_vm::frame::build_jit_helpers`)

- [ ] **Step 1: Add the failing test**

Add to the bottom of `crates/varn-vm/src/frame.rs`:

```rust
#[cfg(test)]
mod build_jit_helpers_tests {
    #[test]
    fn build_jit_helpers_has_real_addresses() {
        let h = super::build_jit_helpers();
        // Function-address fields must be non-zero (real code), and the
        // probed nursery threshold must be the real one, proving this is the
        // production construction and not a zeroed stub.
        assert_ne!(h.add, 0);
        assert_ne!(h.gc_safepoint, 0);
        assert_ne!(h.clif_call_fallback, 0);
        assert!(h.nursery_threshold > 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p varn-vm build_jit_helpers_has_real_addresses`
Expected: FAIL — `build_jit_helpers` not found (`cannot find function`).

- [ ] **Step 3: Extract the builder**

In `crates/varn-vm/src/frame.rs`, locate `compile_jit`. It currently contains:

```rust
        let array_layout = crate::heap::Heap::jit_array_layout();
        let stack_data_offset =
            std::mem::offset_of!(ctx::ExecCtx, stack) + array_layout.elems_ptr_off;
        let helpers = varn_jit::JitHelpers {
            load_const: ctx::jit_load_const as usize,
            // … ~110 fields …
            clif_call_fallback: ctx::clif_call_fallback as usize,
        };
```

Cut everything from `let array_layout = …` through the end of the `let helpers = JitHelpers { … };` literal, and move it into a new free function placed just above `impl` or below `compile_jit` in the same file:

```rust
/// Build the production `JitHelpers` table. All fields are static (function
/// addresses + host-struct offsets + probed layouts) — no live `ExecCtx`
/// needed — so both `compile_jit` and the `vn debug -p clif` inspection path
/// share this single source of truth.
pub fn build_jit_helpers() -> varn_jit::JitHelpers {
    let array_layout = crate::heap::Heap::jit_array_layout();
    let stack_data_offset =
        std::mem::offset_of!(ctx::ExecCtx, stack) + array_layout.elems_ptr_off;
    varn_jit::JitHelpers {
        load_const: ctx::jit_load_const as usize,
        // … paste the ENTIRE field list verbatim from the old literal …
        clif_call_fallback: ctx::clif_call_fallback as usize,
    }
}
```

Then in `compile_jit`, replace the removed block with:

```rust
        let helpers = build_jit_helpers();
```

Leave the rest of `compile_jit` (the `varn_jit::compile(&self.proto, &self.constants, helpers, &linker)` call and everything after) untouched.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p varn-vm build_jit_helpers_has_real_addresses`
Expected: PASS.

- [ ] **Step 5: Non-regression (build + suite)**

```bash
cargo build --release --bin vn
cargo run --release --bin xtask -- build-std
./target/release/vn.exe run ./tests/main.vn          # 701
VARN_NO_CLIF=1 ./target/release/vn.exe run ./tests/main.vn   # 701
```
Expected: both `ALL TESTS PASSED` (701). This proves the extracted helpers are byte-identical to the old inline ones.

- [ ] **Step 6: Commit**

```bash
git add crates/varn-vm/src/frame.rs
git commit -m "refactor(vm): extract build_jit_helpers() from compile_jit (DRY)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: CLIF debug sink + threading through `try_compile`

Add the capture types and thread an optional `&mut ClifDebugSink` through `try_compile` → `lower_raw`. Production callers pass `None` (zero cost, no drift).

**Files:**
- Create: `crates/varn-jit/src/clif/debug.rs`
- Modify: `crates/varn-jit/src/clif/mod.rs` (add `pub mod debug;`)
- Modify: `crates/varn-jit/src/clif/lower.rs` (`try_compile` + `lower_raw` signatures + 3 capture points; update the one production caller)
- Modify: `crates/varn-jit/src/lib.rs` (the `varn_jit::compile` call site of `try_compile`)

**Interfaces:**
- Produces:
  - `pub struct ClifDebugSink { pub kinds: Option<KindReport>, pub clif_ir: Option<String>, pub code: Option<CodeBytes> }` (Default-able: all `Option`, `#[derive(Default)]`)
  - `pub struct KindReport { pub nregs: usize, pub blocks: Vec<(usize, Vec<String>)> }`
  - `pub struct CodeBytes { pub bytes: Vec<u8>, pub raw_off: usize, pub entry_off: usize }`
  - `pub fn try_compile(proto, constants, helpers, isa, linker, debug: Option<&mut ClifDebugSink>) -> Result<ClifArtifact, String>` (added trailing param)

- [ ] **Step 1: Create the capture types**

Create `crates/varn-jit/src/clif/debug.rs`:

```rust
//! Static introspection of the Cranelift lowering for `vn debug -p clif`.
//!
//! Reuses `try_compile` as the single source of truth: a `ClifDebugSink`
//! is threaded in and the lowering records the kind lattice, the textual
//! CLIF IR, and the finalized code bytes as it goes. The production path
//! passes `None`, so this adds nothing to normal compilation.

/// Kind lattice at each block entry (from `kind_flow`). `blocks[i] =
/// (block_start_ip, [K per register as text])`.
#[derive(Debug, Default, Clone)]
pub struct KindReport {
    pub nregs: usize,
    pub blocks: Vec<(usize, Vec<String>)>,
}

/// Finalized machine code for one function's buffer (raw fn at `raw_off`,
/// wrapper at `entry_off`).
#[derive(Debug, Default, Clone)]
pub struct CodeBytes {
    pub bytes: Vec<u8>,
    pub raw_off: usize,
    pub entry_off: usize,
}

/// Capture slots populated by `try_compile` when inspection is active.
#[derive(Debug, Default)]
pub struct ClifDebugSink {
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
}
```

- [ ] **Step 2: Register the module**

In `crates/varn-jit/src/clif/mod.rs`, add after the other `pub(crate) mod` / `pub mod` lines (near line 19):

```rust
pub mod debug;
```

- [ ] **Step 3: Thread the sink through `try_compile` and `lower_raw`**

In `crates/varn-jit/src/clif/lower.rs`:

3a. Add `use super::debug::{ClifDebugSink, CodeBytes, KindReport};` to the `use super::…` block.

3b. Change `try_compile`'s signature (line 98) to add the trailing param:

```rust
pub fn try_compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<ClifArtifact, String> {
```

3c. Pass it to `lower_raw` (line 121). Change:

```rust
    let raw = lower_raw(proto, constants, helpers, isa, linker, has_alloc)?;
```
to:
```rust
    let raw = lower_raw(proto, constants, helpers, isa, linker, has_alloc, debug.as_deref_mut())?;
```

3d. Capture the bytes. After the `buf` build block closes and BEFORE `buf.make_executable()?;` (line 141), insert:

```rust
    if let Some(sink) = debug.as_deref_mut() {
        sink.code = Some(CodeBytes {
            bytes: buf.as_mut_slice().to_vec(),
            raw_off: 0,
            entry_off: wrapper_off,
        });
    }
```

3e. Change `lower_raw`'s signature (line 210) to add the trailing param:

```rust
fn lower_raw(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    has_alloc: bool,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<CompiledPiece, String> {
```

3f. Capture the kind lattice. Right after `let entries = kind_flow(…);` completes (line 436, after the `?`), insert:

```rust
    if let Some(sink) = debug.as_deref_mut() {
        let mut blocks: Vec<(usize, Vec<String>)> = entries
            .iter()
            .map(|(start, ks)| (*start, ks.iter().map(|k| format!("{k:?}")).collect()))
            .collect();
        blocks.sort_by_key(|(s, _)| *s);
        sink.kinds = Some(KindReport { nregs, blocks });
    }
```

3g. Capture the CLIF IR. Immediately before the final `compile_piece(func, isa)` at line 900, insert:

```rust
    if let Some(sink) = debug.as_deref_mut() {
        sink.clif_ir = Some(func.display().to_string());
    }
```

(Leave `build_wrapper`'s `compile_piece` at line 993 alone — we only surface the raw function's IR.)

- [ ] **Step 4: Update the production caller**

In `crates/varn-jit/src/lib.rs`, the `compile` function calls `try_compile` (line ~331):

```rust
            match clif::lower::try_compile(proto, constants, &helpers, isa, linker) {
```
Change to:
```rust
            match clif::lower::try_compile(proto, constants, &helpers, isa, linker, None) {
```

- [ ] **Step 5: Build to verify it compiles**

Run: `cargo build --release -p varn-jit`
Expected: compiles, 0 warnings. (`K` derives `Debug`, so `format!("{k:?}")` works.)

- [ ] **Step 6: Non-regression suite (production path is `None`)**

```bash
cargo build --release --bin vn && cargo run --release --bin xtask -- build-std
./target/release/vn.exe run ./tests/main.vn          # 701
```
Expected: 701 — production compilation unchanged.

- [ ] **Step 7: Commit**

```bash
git add crates/varn-jit/src/clif/debug.rs crates/varn-jit/src/clif/mod.rs crates/varn-jit/src/clif/lower.rs crates/varn-jit/src/lib.rs
git commit -m "feat(jit): thread optional debug sink through clif try_compile

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `clif::debug::inspect()`

Assemble the four data pieces into one `ClifInspection` by running `try_compile` with an active sink.

**Files:**
- Modify: `crates/varn-jit/src/clif/debug.rs`

**Interfaces:**
- Consumes: `try_compile(..., Option<&mut ClifDebugSink>)` (Task 2), `NoLinker` (`clif::lower::NoLinker`), `ClifLinker`.
- Produces:
  - `pub struct ClifInspection { pub name: String, pub route: Result<(), String>, pub kinds: Option<KindReport>, pub clif_ir: Option<String>, pub code: Option<CodeBytes>, pub frame_aware: bool }`
  - `pub fn inspect(proto: &FunctionProto, constants: &[VmValue], helpers: &JitHelpers, isa: &OwnedTargetIsa, linker: &dyn ClifLinker) -> ClifInspection`

- [ ] **Step 1: Add `inspect` and its output type**

Append to `crates/varn-jit/src/clif/debug.rs`:

```rust
use varn_types::{FunctionProto, VmValue};
use cranelift_codegen::isa::OwnedTargetIsa;
use crate::JitHelpers;
use super::lower::{try_compile, ClifLinker};

/// Everything `vn debug -p clif` shows for one function.
pub struct ClifInspection {
    pub name: String,
    /// `Ok(())` = ROUTE; `Err(reason)` = BAIL.
    pub route: Result<(), String>,
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
    pub frame_aware: bool,
}

/// Run the clif lowering for `proto` with capture active, without executing.
pub fn inspect(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> ClifInspection {
    let mut sink = ClifDebugSink::default();
    let result = try_compile(proto, constants, helpers, isa, linker, Some(&mut sink));
    let (route, frame_aware) = match &result {
        Ok(art) => (Ok(()), art.frame_aware),
        Err(e) => (Err(e.clone()), false),
    };
    ClifInspection {
        name: proto.name.as_deref().unwrap_or("<top-level>").to_string(),
        route,
        kinds: sink.kinds,
        clif_ir: sink.clif_ir,
        code: sink.code,
        frame_aware,
    }
}
```

> Note: `try_compile` may early-return `Err` before `lower_raw` runs (e.g. generators, upvalues), leaving `kinds`/`clif_ir` `None`. On an op-level BAIL inside `lower_raw`, `kinds` is populated (captured right after `kind_flow`, before the bailing op) but `clif_ir`/`code` are `None`. The renderer handles all combinations.

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build --release -p varn-jit`
Expected: compiles, 0 warnings. (`ClifLinker` and `NoLinker` are `pub` in `lower.rs`; `try_compile` is `pub`.)

- [ ] **Step 3: Commit**

```bash
git add crates/varn-jit/src/clif/debug.rs
git commit -m "feat(jit): clif::debug::inspect() assembles a ClifInspection

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `clif` debug flags + sub-phases

Add the `clif` phase and its `clif:route|kinds|ir|asm|all` sub-phases to `DebugFlags`, mirroring the existing `lsp:` idiom.

**Files:**
- Modify: `crates/varn-debug/src/flags.rs`

**Interfaces:**
- Produces: `DebugFlags { … clif, clif_route, clif_kinds, clif_ir, clif_asm: bool }` and parse support for `"clif"` and `"clif:<sub>[+<sub>…]"`.

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `crates/varn-debug/src/flags.rs`:

```rust
#[cfg(test)]
mod clif_flag_tests {
    use super::DebugFlags;

    #[test]
    fn bare_clif_enables_all_four_views() {
        let f = DebugFlags::parse("clif").unwrap();
        assert!(f.clif && f.clif_route && f.clif_kinds && f.clif_ir && f.clif_asm);
        assert!(f.any());
    }

    #[test]
    fn clif_ir_enables_only_ir() {
        let f = DebugFlags::parse("clif:ir").unwrap();
        assert!(f.clif && f.clif_ir);
        assert!(!f.clif_route && !f.clif_kinds && !f.clif_asm);
    }

    #[test]
    fn clif_multi_sub_via_comma() {
        let f = DebugFlags::parse("clif:kinds,clif:asm").unwrap();
        assert!(f.clif && f.clif_kinds && f.clif_asm);
        assert!(!f.clif_ir && !f.clif_route);
    }

    #[test]
    fn unknown_clif_sub_errors() {
        assert!(DebugFlags::parse("clif:bogus").is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p varn-debug clif_flag_tests`
Expected: FAIL — `no field clif on DebugFlags`.

- [ ] **Step 3: Add the fields**

In the `pub struct DebugFlags` (after `pub ssa: bool,`, line 36):

```rust
    pub clif: bool,
    pub clif_route: bool,
    pub clif_kinds: bool,
    pub clif_ir: bool,
    pub clif_asm: bool,
```

- [ ] **Step 4: Add parsing**

4a. In `parse`, add a `clif:` prefix branch alongside the `lsp:` branch (after the `lsp:` `else if`, before the final `else`):

```rust
            } else if let Some(sub) = phase.strip_prefix("clif:") {
                flags.clif = true;
                for sub_part in sub.split('+') {
                    match sub_part {
                        "route" => flags.clif_route = true,
                        "kinds" => flags.clif_kinds = true,
                        "ir" => flags.clif_ir = true,
                        "asm" => flags.clif_asm = true,
                        "all" => flags.clif_all(),
                        unknown => {
                            return Err(CliError::usage(format!(
                                "unknown clif debug sub-phase: '{unknown}'\n\
                                 Valid sub-phases: route, kinds, ir, asm, all"
                            )));
                        }
                    }
                }
```

4b. In the bare-phase `match phase { … }`, add (after `"ssa" => flags.ssa = true,`):

```rust
                    "clif" => flags.clif_all_on(),
```

4c. Add the two helper methods in `impl DebugFlags` (next to `lsp_all`):

```rust
    pub fn clif_all(&mut self) {
        self.clif_route = true;
        self.clif_kinds = true;
        self.clif_ir = true;
        self.clif_asm = true;
    }

    /// Bare `clif` = the phase plus all four views.
    pub fn clif_all_on(&mut self) {
        self.clif = true;
        self.clif_all();
    }
```

4d. In `any()`, add `|| self.clif` to the chain.

4e. In the bare `"all"` arm, add `flags.clif_all_on();` so `-p all` includes clif.

4f. Update the unknown-phase help text to mention `clif` and `clif:route/kinds/ir/asm/all`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p varn-debug clif_flag_tests`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/varn-debug/src/flags.rs
git commit -m "feat(debug): clif phase + clif:route/kinds/ir/asm sub-phases

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Renderer + x86-64 disassembler

New `varn-debug/src/clif.rs`: walk protos recursively, call `inspect`, print the enabled views. Owns the `iced-x86` machine disassembler.

**Files:**
- Create: `crates/varn-debug/src/clif.rs`
- Modify: `crates/varn-jit/src/lib.rs` (add `pub use cranelift_codegen::isa::OwnedTargetIsa;` re-export — Step 0)
- Modify: `crates/varn-debug/src/lib.rs` (add `pub mod clif;`)
- Modify: `crates/varn-debug/Cargo.toml` (add `iced-x86`)

**Interfaces:**
- Consumes: `varn_jit::clif::debug::{inspect, ClifInspection}`, `varn_jit::clif::{shared_isa, lower::NoLinker}`, `varn_jit::JitHelpers`, `DebugFlags` (Task 4), `FunctionProto`.
- Produces: `pub fn debug_clif(proto: &FunctionProto, flags: &DebugFlags, helpers: &JitHelpers)`

- [ ] **Step 1: Add the dependency**

In `crates/varn-debug/Cargo.toml` under `[dependencies]`:

```toml
iced-x86 = "1"
```

- [ ] **Step 2: Write the renderer**

Create `crates/varn-debug/src/clif.rs`:

```rust
//! `vn debug -p clif` — Cranelift backend introspection, per function.

use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use varn_jit::clif::debug::{inspect, ClifInspection};
use varn_jit::clif::lower::NoLinker;
use varn_jit::JitHelpers;
use varn_types::{FunctionProto, PoolEntry};

use crate::flags::DebugFlags;

const BOLD: &str = "\x1b[1m";
const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const R: &str = "\x1b[0m";

/// Entry point: render the clif views for `proto` and every nested proto.
pub fn debug_clif(proto: &FunctionProto, flags: &DebugFlags, helpers: &JitHelpers) {
    eprintln!(
        "\n{BOLD}{BLUE}CLIF{R}{DIM} ─────────────────────────────── {}{R}",
        proto.name.as_deref().unwrap_or("<top-level>")
    );
    let isa = match varn_jit::clif::shared_isa() {
        Ok(isa) => isa,
        Err(e) => {
            eprintln!("  {RED}error{R} host ISA unavailable: {e}");
            return;
        }
    };
    render_recursive(proto, flags, helpers, isa);
    eprintln!("{DIM}── end: CLIF ──{R}");
}

fn render_recursive(
    proto: &FunctionProto,
    flags: &DebugFlags,
    helpers: &JitHelpers,
    isa: &varn_jit::OwnedTargetIsa,
) {
    let constants = constants_for_inspect(proto);
    let insp = inspect(proto, &constants, helpers, isa, &NoLinker);
    render_one(&insp, flags);
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(f) = entry {
            render_recursive(f, flags, helpers, isa);
        }
    }
}

/// Heap-free constant resolution for inspection. Only `is_int()` fidelity
/// matters to the lowering's kind classification, so scalar literals map
/// exactly (mirroring `varn_vm::exec::calls::resolve_constants`) and heap
/// literals (strings, bigints, symbols, chars) plus function/shape entries
/// become `null` placeholders — they are non-int, which is the correct kind,
/// and their real heap bits are irrelevant to a static, non-executing view.
/// (Consequence: a string constant shows as `null` in the IR/disasm — see
/// the phase limitations.)
fn constants_for_inspect(proto: &FunctionProto) -> Vec<VmValue> {
    const I48_MIN: i64 = -(1_i64 << 47);
    const I48_MAX: i64 = (1_i64 << 47) - 1;
    proto
        .chunk
        .constants
        .iter()
        .map(|entry| match entry {
            PoolEntry::Literal(Literal::Null) => VmValue::null(),
            PoolEntry::Literal(Literal::Bool(b)) => VmValue::from_bool(*b),
            PoolEntry::Literal(Literal::Int(n)) if *n >= I48_MIN && *n <= I48_MAX => {
                VmValue::from_int(*n)
            }
            PoolEntry::Literal(Literal::Int(n)) => VmValue::from_f64(*n as f64),
            PoolEntry::Literal(Literal::Float(f)) => VmValue::from_f64(*f),
            // Heap literals + function/shape entries: non-int placeholder.
            _ => VmValue::null(),
        })
        .collect()
}

fn render_one(insp: &ClifInspection, flags: &DebugFlags) {
    let fa = if insp.frame_aware { " (frame-aware)" } else { "" };
    eprintln!("\n  {BOLD}{}{R}{DIM}{fa}{R}", insp.name);

    if flags.clif_route {
        match &insp.route {
            Ok(()) => eprintln!("    {GREEN}ROUTE{R}"),
            Err(reason) => eprintln!("    {RED}BAIL{R}  {reason}"),
        }
    }

    if flags.clif_kinds {
        if let Some(k) = &insp.kinds {
            eprintln!("    {DIM}kinds ({} regs):{R}", k.nregs);
            for (start, ks) in &k.blocks {
                eprintln!("      block@{start}: [{}]", ks.join(", "));
            }
        }
    }

    if flags.clif_ir {
        if let Some(ir) = &insp.clif_ir {
            eprintln!("    {DIM}clif ir:{R}");
            for line in ir.lines() {
                eprintln!("      {line}");
            }
        }
    }

    if flags.clif_asm {
        if let Some(code) = &insp.code {
            eprintln!("    {DIM}x86-64 (raw@{} wrapper@{}):{R}", code.raw_off, code.entry_off);
            eprint!("{}", disasm(&code.bytes, code.raw_off as u64));
        }
    }
}

/// Decode `bytes` (x86-64) into Intel-syntax text, one instruction per line.
fn disasm(bytes: &[u8], rip: u64) -> String {
    let mut decoder = Decoder::with_ip(64, bytes, rip, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut out = String::new();
    let mut line = String::new();
    let mut inst = Instruction::default();
    while decoder.can_decode() {
        decoder.decode_out(&mut inst);
        line.clear();
        formatter.format(&inst, &mut line);
        out.push_str(&format!("      {:016x}  {line}\n", inst.ip()));
    }
    out
}
```

The imports block at the top of `clif.rs` must include the constant types:
`use varn_types::{FunctionProto, PoolEntry, VmValue, chunk::Literal};` (verify the
`Literal` path against `crates/varn-types/src/chunk.rs` — the enum is defined there;
adjust the `use` path if it is re-exported at the crate root as `varn_types::Literal`).

The ISA type is named via a re-export added to `varn-jit` in Step 0 below
(`varn_jit::OwnedTargetIsa`), so `varn-debug` needs no `cranelift-codegen` dependency.

- [ ] **Step 0: Re-export the ISA type from varn-jit** (so `varn-debug` can name it without a new dep)

In `crates/varn-jit/src/lib.rs`, add near the top-level re-exports:

```rust
pub use cranelift_codegen::isa::OwnedTargetIsa;
```

Verify with `cargo build -p varn-jit` (fast). Commit this tiny change with the Task 5 commit (Step 5).

- [ ] **Step 3: Register the module**

In `crates/varn-debug/src/lib.rs`, add (alphabetically near `pub mod cap_trace;`):

```rust
pub mod clif;
```

- [ ] **Step 4: Build to verify it compiles**

Run: `cargo build --release -p varn-debug`
Expected: compiles, 0 warnings. (If `Literal` is not at `varn_types::chunk::Literal`, fix the `use` path per Step 2's note — that is the only expected snag.)

- [ ] **Step 5: Commit**

```bash
git add crates/varn-jit/src/lib.rs crates/varn-debug/src/clif.rs crates/varn-debug/src/lib.rs crates/varn-debug/Cargo.toml Cargo.lock
git commit -m "feat(debug): clif renderer + iced-x86 disassembler

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Wire the pipeline + behavioral tests + docs

Dispatch `debug.clif` in `compile.rs` next to `debug.bytecode`, verify the four views end-to-end via the CLI, and document the phase.

**Files:**
- Modify: `crates/varn-pipeline/src/compile.rs` (two dispatch sites: ~line 54 and the module loop ~line 105)
- Modify: `docs/CLI_INSPECT.md`

**Interfaces:**
- Consumes: `varn_vm::frame::build_jit_helpers` (Task 1), `varn_debug::clif::debug_clif` (Task 5).

- [ ] **Step 1: Wire the main proto dispatch**

In `crates/varn-pipeline/src/compile.rs`, immediately after the `if debug.bytecode { varn_debug::bytecode::debug_bytecode(&proto, debug); }` block (line 54-56), add:

```rust
    if debug.clif {
        let helpers = varn_vm::frame::build_jit_helpers();
        varn_debug::clif::debug_clif(&proto, debug, &helpers);
    }
```

- [ ] **Step 2: Wire the module-loop dispatch**

In the module loop that renders `debug.bytecode` for each module (line ~105), after that inner `debug_bytecode` call, add a sibling:

```rust
    if debug.clif {
        let helpers = varn_vm::frame::build_jit_helpers();
        for (path, module_proto) in graph_build.modules.iter() {
            if path != &graph_build.entry_path {
                println!("\n=== MODULE CLIF: {} ===", path);
                varn_debug::clif::debug_clif(module_proto, debug, &helpers);
            }
        }
    }
```

- [ ] **Step 3: Rebuild the binary**

```bash
cargo build --release --bin vn && cargo run --release --bin xtask -- build-std
```
Expected: builds clean.

- [ ] **Step 4: Behavioral test — ROUTE case (all four views)**

Run:
```bash
./target/release/vn.exe debug -p clif -e "function f(a: int, b: int): int = a * b + 1" 2>&1
```
Expected output contains, for `f`: a `ROUTE` line; a `kinds` block showing `Int` registers; a `clif ir:` section containing `imul` and `iadd`; and an `x86-64` section with disassembled instructions (e.g. `imul`, `ret`).

Assert:
```bash
out=$(./target/release/vn.exe debug -p clif -e "function f(a: int, b: int): int = a * b + 1" 2>&1)
echo "$out" | grep -q "ROUTE" && echo "$out" | grep -qi "imul" && echo "$out" | grep -q "Int" && echo "PASS route" || echo "FAIL route"
```
Expected: `PASS route`.

- [ ] **Step 5: Behavioral test — BAIL case (route + kinds only)**

`DivInt` is not routed (int/int→float; see the clif-alloc memory). A function that divides two ints bails:
```bash
out=$(./target/release/vn.exe debug -p clif:route -e "function g(a: int, b: int): float = a / b" 2>&1)
echo "$out" | grep -qiE "BAIL|DivInt|unsupported" && echo "PASS bail" || echo "FAIL bail"
```
Expected: `PASS bail`.

- [ ] **Step 6: Behavioral test — sub-phase isolation**

```bash
out=$(./target/release/vn.exe debug -p clif:asm -e "function f(a: int): int = a + 2" 2>&1)
echo "$out" | grep -q "x86-64" && ! echo "$out" | grep -q "clif ir:" && echo "PASS iso" || echo "FAIL iso"
```
Expected: `PASS iso` (asm shown, IR suppressed).

- [ ] **Step 7: Non-regression — full suite over the phase + all three tiers**

```bash
Remove-Item -Recurse -Force $env:LOCALAPPDATA\varn\cache -ErrorAction SilentlyContinue
./target/release/vn.exe debug -p clif ./tests/main.vn > $null 2>&1; echo "clif-phase-exit:$LASTEXITCODE"   # no panic
./target/release/vn.exe run ./tests/main.vn                    # 701
VARN_NO_CLIF=1 ./target/release/vn.exe run ./tests/main.vn     # 701
VARN_NO_JIT=1 ./target/release/vn.exe run ./tests/main.vn      # 763
```
Expected: `clif-phase-exit:0`, then 701 / 701 / 763.

- [ ] **Step 8: Document the phase**

In `docs/CLI_INSPECT.md`, add `clif` to the `-p` table and a section:

```markdown
### `clif` — Backend Cranelift
Por función: decisión ROUTE/BAIL + razón, lattice de kinds, CLIF IR textual y
disasm x86-64 del código generado. Estático (no ejecuta).

Sub-fases: `clif:route`, `clif:kinds`, `clif:ir`, `clif:asm`, `clif:all`.
`clif` a secas = las cuatro.

    vn debug -p clif      -e "function f(a:int):int = a*2"
    vn debug -p clif:asm  src/hot.vn

**Cuándo usarlo:** verificar por qué una función rutea o bailea, revisar el
lowering a CLIF, cazar bugs de codegen/regalloc.
```

Also update the "Otros valores disponibles" line and the comparison table.

- [ ] **Step 9: Governance check**

```bash
for f in crates/varn-jit/src/clif/debug.rs crates/varn-jit/src/clif/lower.rs crates/varn-debug/src/clif.rs crates/varn-debug/src/flags.rs; do echo "$f: $((Get-Content $f).Count)"; done
```
Expected: every file < 1000. `lower.rs` was ~994 before the 🟢 batch shrank it; the ~9 inserted lines keep it under 1000 — if it crosses, extract the three capture blocks into a `debug_capture` helper in `clif/debug.rs` called from `lower.rs`.

- [ ] **Step 10: Commit**

```bash
git add crates/varn-pipeline/src/compile.rs docs/CLI_INSPECT.md
git commit -m "feat(debug): wire vn debug -p clif phase end-to-end

Cranelift introspection per function: ROUTE/BAIL, kind lattice, CLIF IR,
x86-64 disasm. Validated 701/701/763 across tiers.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:** UX/sub-phases → Task 4; four views (route/kinds/ir/asm) → Task 2 (capture) + Task 3 (inspect) + Task 5 (render); real helpers via builder → Task 1; pipeline wire → Task 6; docs → Task 6; iced-x86 dep → Task 5; recursion over protos → Task 5. All spec sections mapped.

**Placeholder scan:** the two accessor uncertainties in Task 5 (`constants_values()` name; ISA re-export path) are flagged with concrete fallbacks, not left as TODO — the implementer picks the branch the compiler dictates.

**Type consistency:** `ClifDebugSink`/`KindReport`/`CodeBytes` defined in Task 2 and consumed in Task 3/5 with matching field names (`kinds`, `clif_ir`, `code`, `nregs`, `blocks`, `bytes`, `raw_off`, `entry_off`); `inspect` signature identical in Task 3 (def) and Task 5 (call); `build_jit_helpers` identical in Task 1 (def) and Task 6 (call); flag field names (`clif`, `clif_route/kinds/ir/asm`) identical in Task 4 (def) and Task 5 (read).
