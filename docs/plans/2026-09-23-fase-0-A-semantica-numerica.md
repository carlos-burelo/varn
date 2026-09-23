# Fase 0 + Fase A — Protección y semántica numérica del núcleo

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** dejar la semántica numérica observable de Varn exactamente como la
fija `NEW_SPEC.md` §2–§12 y §61 — cuatro tipos numéricos, `int` checked con
errores tipados, `int / int → int`, IEEE en `float`, conversiones explícitas y
reales — y borrar los anchos angostos públicos.

**Architecture:** una sola fuente de verdad por hecho (Ley 6):
`varn_core::numeric` decide reglas de operandos, resultados, división y
conversión; `varn_core::errors::RuntimeErrorKind` decide la clase del error;
el checker, el const-folder, la VM y el JIT consumen esas funciones. Las
conversiones pasan de `Cast` (un `Move`) a un nodo propio `Convert` con un
opcode único guiado por datos (`NumConv`).

**Tech Stack:** Rust workspace, Cranelift, corpus `.vn`.

**Spec:** `NEW_SPEC.md` (→ `docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md` tras la
Tarea 0.1). Roadmap y decisiones D1–D13:
`docs/plans/2026-09-23-new-spec-roadmap.md` §1.

## Global Constraints

- Tipos numéricos públicos: **exactamente** `int`, `float`, `bigint`, `decimal`.
- `int`: signed 64-bit two's complement, `-2^63 .. 2^63-1`, **checked**; overflow
  lanza `IntegerOverflow`; sin wrapping silencioso, sin promoción a `bigint`,
  sin conversión a `float`.
- `float`: IEEE 754 binary64 (`+0`, `-0`, `NaN`, `±Infinity`, subnormales).
- Implícitas permitidas: solo `int → bigint`, `int → decimal` (D4) y literales
  enteros exactamente representables en contexto numérico (D6).
- `int / int → int` truncado hacia cero; `int / 0`, `int % 0` →
  `DivisionByZero`; `i64::MIN / -1` → `IntegerOverflow`; `i64::MIN % -1 == 0` (D8).
- `float / 0.0`, `float % 0.0` → IEEE, no lanzan (D9).
- `float as int`: trunca hacia cero; `NaN`, `±Inf`, fuera de rango →
  `IntegerOverflow` (D3).
- Aritmética `dynamic`: `int ⊕ float → float` se conserva; `int / int` da `int` (D7).
- `AGENTS.md`: Leyes 1–10; archivos ≤ 400 líneas (si un archivo tocado ya lo
  excede, **no** crecerlo: extraer el dominio nuevo a un archivo propio); sin
  `_ =>` que oculte variantes (Ley 7); un commit por cambio (Ley 9); **no
  ejecutar `git` sin autorización explícita del usuario** — los pasos "Commit"
  asumen esa autorización.
- Comentarios mínimos: nombres autoexplicativos; comentario solo para el
  *porqué* no obvio, breve.

## Comandos de validación (referenciados como `GATE`)

```powershell
# GATE-CHECK: compila todo lo relevante sin warnings
cargo check --workspace --exclude varn-lsp --all-targets

# GATE-SUITE: suite canónica en los dos tiers con caché limpio
cargo build --release -p varn-cli
$env:VARN_CACHE_DIR = "$env:TEMP\varn-cache-$(Get-Random)"; .\target\release\vn.exe run tests/main.vn
$env:VARN_NO_JIT = "1"; .\target\release\vn.exe run tests/main.vn; Remove-Item Env:VARN_NO_JIT
Remove-Item Env:VARN_CACHE_DIR
# Criterio: "PASSED: N", "FAILED: 0" en ambas corridas.

# GATE-ERRORS (existe desde la Tarea 0.2)
cargo test -p varn-cli --test error_corpus

# GATE-PHASE (al cerrar la fase): 4 cuadrantes + fmt + clippy
.\scripts\verify.ps1 -Fast
```

`varn-cli/build.rs` compila `std/`: un error de tipos en `std/` rompe
`cargo build -p varn-cli` y el compilador lista cada sitio.

## Mapa de archivos

| Archivo | Responsabilidad | Tareas |
|---|---|---|
| `docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md` | spec (movido) | 0.1 |
| `docs/decisions/ADR-0015-new-spec-numeric-core.md` | decisiones D1–D13 | 0.1 |
| `crates/varn-cli/tests/error_corpus.rs` | runner del corpus negativo | 0.2 |
| `tests/117-tuples.vn`, `tests/118-intersections.vn` | regresión de crashes | 0.3, 0.4 |
| `crates/varn-core/src/errors.rs` (nuevo) | `RuntimeErrorKind` | A.1 |
| `crates/varn-core/src/numeric.rs` | reglas numéricas | A.2, A.4, A.6, A.7 |
| `crates/varn-core/src/numeric_conv.rs` (nuevo) | `NumConv` + funciones de conversión | A.6 |
| `crates/varn-vm/src/error.rs` | `RuntimeError.kind` | A.1 |
| `crates/varn-vm/src/exec/exceptions.rs` | clase del valor capturado | A.1 |
| `crates/varn-vm/src/exec/arith.rs` | aritmética genérica | A.1, A.2, A.4, A.5 |
| `crates/varn-vm/src/exec/dispatch/ops_math_cmp.rs` | opcodes tipados | A.1, A.2, A.4 |
| `crates/varn-vm/src/exec/convert.rs` (nuevo) | opcode `Convert` | A.6 |
| `crates/varn-builtins/src/modules/globals/{globals.vn,globals.rs}` | clases de error | A.1 |
| `crates/varn-compiler/src/passes/dce.rs` | pureza | A.3 |
| `crates/varn-compiler/src/passes/const_fold.rs` | plegado | A.4, A.6 |
| `crates/varn-compiler/src/ssa/{ir.rs,emit/values.rs,portable.rs,dump.rs,uses.rs,verify.rs}` | `InstKind::Convert` | A.6 |
| `crates/varn-compiler/src/from_tir/build.rs` | `Cast` → `Convert` | A.6 |
| `crates/varn-types/src/ssa.rs` | `SsaOp::Convert` | A.6 |
| `crates/varn-jit/src/clif/from_ssa/{scalar.rs,boxed.rs}` | JIT de `Convert`/`IntDiv` | A.4, A.6 |
| `crates/varn-checker/src/checker/compat/mod.rs` | asignabilidad numérica | A.7, A.8 |
| `crates/varn-checker/src/checker_expressions/check/mod.rs` | validez de operadores | A.7 |
| `crates/varn-checker/src/binder/type_inference.rs`, `checker/refine.rs` | tipo resultado | A.7, A.8 |
| `crates/varn-core/src/{type_tag.rs,intrinsics.rs}` | borrar anchos | A.8 |
| `crates/varn-types/src/native_ctx.rs` | `NativeError` | A.9 |
| `crates/varn-op-macros/src/varn_contract.rs` | métodos `@fallible` | A.9 |
| `crates/varn-builtins/src/modules/primitives/int/{int.vn,int.rs}` | API de enteros | A.9 |

---

# FASE 0

### Tarea 0.1: Spec a `docs/lang/` y ADR-0015

**Files:**
- Move: `NEW_SPEC.md` → `docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md`
- Create: `docs/decisions/ADR-0015-new-spec-numeric-core.md`
- Modify: `docs/decisions/ADR-0004-type-system-reforms.md` (sección Estado)
- Modify: `docs/lang/README.md` (índice)

**Interfaces:** Produces: la ruta canónica del spec que citan todas las tareas.

- [ ] **Step 1: Mover el spec**

```powershell
Move-Item NEW_SPEC.md docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md
```

- [ ] **Step 2: Escribir ADR-0015**

Crear `docs/decisions/ADR-0015-new-spec-numeric-core.md`:

```markdown
# ADR 0015: Núcleo semántico de NEW_SPEC — números, conversiones y errores

## Estado
Aceptada (2026-09-23). Reemplaza la sección 2 de ADR-0004.

## Contexto
`docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md` fija cuatro tipos numéricos públicos
y prohíbe conversiones implícitas con pérdida. El código aceptaba `int → float`
implícito, `int / int → float`, ocho anchos angostos públicos, y tenía
crashes/errores silenciosos (ver roadmap §2.1).

## Decisiones
- D1 `for (const x of e)` se mantiene; `for x in e` del spec es ilustrativo.
- D2 `Symbol` sale del núcleo (Fase C).
- D3 `float as int` trunca hacia cero; NaN/±Inf/fuera de rango → `IntegerOverflow`.
- D4 Implícitas: solo `int → bigint`, `int → decimal`.
- D5 `int → float`, `float ↔ decimal` requieren `as`.
- D6 Un literal entero adopta `float`/`decimal`/`bigint` del contexto si es
  exactamente representable (`|v| ≤ 2^53` para `float`).
- D7 `dynamic` conserva `int ⊕ float → float`; `int / int` es `int` en todo tier.
- D8 `int / int` trunca; `/ 0` y `% 0` → `DivisionByZero`; `MIN / -1` →
  `IntegerOverflow`; `MIN % -1 == 0`.
- D9 `float / 0.0` y `float % 0.0` siguen IEEE.
- D10 `IntegerOverflow` y `DivisionByZero` son clases `extends Error`.
- D11 `ArrayRepr` angosto se borra; vuelve solo como optimización probada (Fase F).
- D12 `bigint`/`decimal` de precisión arbitraria (Fase B, ADR-0016).
- D13 API de enteros: `wrapping*`, `saturating*`, `checked*` (→ `int?`), `div`,
  `floorDiv`, `ceilDiv`, `rem`, `mod`.

## Consecuencias
- Se borran `i8 i16 i32 u8 u16 u32 u64 f32` de lexer/checker/TIR/SSA/VM/JIT.
- Programas con `int / int` esperando `float` cambian de resultado: se migran
  en el mismo commit que cambia la regla.
- Ganancia (Ley 10): corrección (tres crashes/silencios eliminados), una sola
  representación por tipo, ~1500 líneas menos en backend.
```

- [ ] **Step 3: Marcar ADR-0004 §2 como reemplazada**

En `docs/decisions/ADR-0004-type-system-reforms.md`, bajo `## Estado`, añadir
la línea:

```markdown
Sección 2 ("Prohibición de Narrowing Implícito en Numéricos") reemplazada por ADR-0015.
```

- [ ] **Step 4: Indexar el spec**

En `docs/lang/README.md` añadir una entrada:
`- [Spec del núcleo y plataforma](SPEC_NUCLEO_Y_PLATAFORMA.md) — modelo de tipos, representación y plataforma estándar (fuente de verdad del lenguaje).`

- [ ] **Step 5: Commit**

```bash
git add docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md docs/decisions/ADR-0015-new-spec-numeric-core.md docs/decisions/ADR-0004-type-system-reforms.md docs/lang/README.md
git commit -m "docs: adopt NEW_SPEC as language spec and record ADR-0015"
```

---

### Tarea 0.2: Runner del corpus negativo `tests/errors/`

Hoy nadie ejecuta `tests/errors/*.vn`. Sin esto, las tareas A.7/A.8 no pueden
probar rechazos.

**Files:**
- Create: `crates/varn-cli/tests/error_corpus.rs`
- Modify: `tests/errors/try-operator-invalid-return.vn` (le falta cabecera)
- Modify: `tests/errors/int-overflow-mul.vn` y los que tengan comentario "not catchable" (texto obsoleto)

**Interfaces:**
- Produces: contrato de cabecera de fixture, primera línea:
  - `// expect: error[VNxxxx]` o `// expect: warning[VNxxxx]` → `vn check` debe
    imprimir exactamente ese token (con error ⇒ exit ≠ 0).
  - `// expect: error[<texto>]` sin `VN` → error runtime: `vn check` pasa y
    `vn run` sale con exit ≠ 0 e imprime `<texto>`.

- [ ] **Step 1: Escribir el runner (el test ES el entregable)**

`crates/varn-cli/tests/error_corpus.rs`:

```rust
//! Corpus negativo: cada `tests/errors/*.vn` declara en su primera línea qué
//! debe rechazarlo. Sin este runner los fixtures eran documentación, no tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

enum Expect {
    Diagnostic { token: String, is_error: bool },
    Runtime { text: String },
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn parse_expect(first_line: &str) -> Option<Expect> {
    let rest = first_line.trim().strip_prefix("// expect: ")?;
    let (level, inner) = rest.split_once('[')?;
    let inner = inner.strip_suffix(']')?;
    match (level, inner.starts_with("VN")) {
        ("error", true) | ("warning", true) => Some(Expect::Diagnostic {
            token: format!("{level}[{inner}]"),
            is_error: level == "error",
        }),
        ("error", false) => Some(Expect::Runtime { text: inner.to_owned() }),
        _ => None,
    }
}

fn vn(sub: &str, file: &Path, cache: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vn"))
        .arg(sub)
        .arg(file)
        .env("VARN_CACHE_DIR", cache)
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn vn")
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn check_fixture(path: &Path, cache: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let first = src.lines().next().unwrap_or_default();
    let expect = parse_expect(first)
        .ok_or_else(|| format!("cabecera inválida: {first:?}"))?;
    match expect {
        Expect::Diagnostic { token, is_error } => {
            let out = vn("check", path, cache);
            let text = combined(&out);
            if !text.contains(&token) {
                return Err(format!("esperaba {token}, salida:\n{text}"));
            }
            if is_error && out.status.success() {
                return Err(format!("{token} pero `vn check` salió con 0"));
            }
            Ok(())
        }
        Expect::Runtime { text: wanted } => {
            let checked = vn("check", path, cache);
            if !checked.status.success() {
                return Err(format!("debía compilar, salida:\n{}", combined(&checked)));
            }
            let out = vn("run", path, cache);
            let text = combined(&out);
            if out.status.success() || !text.contains(&wanted) {
                return Err(format!("esperaba fallo runtime {wanted:?}, salida:\n{text}"));
            }
            Ok(())
        }
    }
}

#[test]
fn every_error_fixture_is_rejected_as_declared() {
    let root = repo_root();
    let cache = std::env::temp_dir().join(format!("varn-error-corpus-{}", std::process::id()));
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(root.join("tests/errors"))
        .expect("tests/errors")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "vn"))
        .collect();
    fixtures.sort();
    let failures: Vec<String> = fixtures
        .iter()
        .filter_map(|p| check_fixture(p, &cache).err().map(|e| format!("{}: {e}", p.display())))
        .collect();
    let _ = std::fs::remove_dir_all(&cache);
    assert!(failures.is_empty(), "{} fixture(s):\n{}", failures.len(), failures.join("\n\n"));
}
```

- [ ] **Step 2: Correr y leer fallos**

Run: `cargo test -p varn-cli --test error_corpus -- --nocapture`
Expected: FAIL; como mínimo `try-operator-invalid-return.vn: cabecera inválida`.
Anotar la lista completa de fallos en el mensaje del commit del Step 5.

- [ ] **Step 3: Arreglar cabeceras (no el comportamiento)**

- `tests/errors/try-operator-invalid-return.vn`: correr
  `.\target\release\vn.exe check tests/errors/try-operator-invalid-return.vn`,
  confirmar que el error es el del operador `try` (mensaje
  `operator 'try' on 'Result' requires enclosing function to return 'Result'`)
  y anteponer `// expect: error[VN3001]`.
- En los fixtures `int-overflow-*.vn`, borrar el párrafo que dice
  "not catchable by try/catch" (es falso desde `exceptions.rs::thrown_value_for`).
- Cualquier otro fixture que falle por **comportamiento** (el compilador ya
  no lo rechaza): no se toca aquí; se lista en la sección "Fallos conocidos"
  de este plan (abajo) y se resuelve en la tarea que corresponda. Si no
  corresponde a ninguna tarea de Fase A, añadir un `#[test]` hermano no; en su
  lugar mover el fixture a `tests/errors/pending/` (el runner solo lee el
  directorio raíz) y registrar la deuda en el roadmap §2.

- [ ] **Step 4: Verde**

Run: `cargo test -p varn-cli --test error_corpus`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/varn-cli/tests/error_corpus.rs tests/errors
git commit -m "test: run the negative corpus tests/errors as an integration test"
```

**Fallos conocidos** (rellenar en el Step 3 con la salida real; cada línea
cita la tarea que lo resuelve).

---

### Tarea 0.3: Crash de tuplas

Repro verificado: `let t = #[1, "a", true]; print(t[1])` →
`panicked at crates\varn-vm\src\frame_store.rs:202:42: index out of bounds: the len is 11 but the index is 11`
desde `ExecCtx::new`. El frame del módulo tiene un registro menos del que el
bytecode usa: el conteo de registros no incluye los que `BuildTuple` necesita.

**Files:**
- Create: `tests/117-tuples.vn`
- Modify: `tests/main.vn` (import)
- Modify: el sitio de conteo de registros que el Step 3 localice (candidatos:
  `crates/varn-compiler/src/ssa/emit/values.rs` rama `InstKind::BuildTuple`,
  y el cálculo de `max_regs`/`frame_size` del `FunctionProto` en
  `crates/varn-compiler/src/regalloc/`)

- [ ] **Step 1: Test que falla**

`tests/117-tuples.vn`:

```varn
const t = #[1, "a", true]
assert("tuple index 0", t[0] === 1)
assert("tuple index 1", t[1] === "a")
assert("tuple index 2", t[2] === true)

function pair(): #[int, str] { return #[7, "x"] }
const p = pair()
assert("tuple from fn", p[0] === 7 && p[1] === "x")

const u = #[1, 2]
const v = #[1, 2]
assert("tuple structural eq", u == v)

print("[PASSED] 117. Tuples")
```

Añadir `import "./117-tuples.vn"` al final de `tests/main.vn`.

- [ ] **Step 2: Verificar que falla**

Run: `.\target\release\vn.exe run tests/117-tuples.vn`
Expected: panic `frame_store.rs:202` (index out of bounds).

- [ ] **Step 3: Localizar (superpowers:systematic-debugging)**

```powershell
.\target\release\vn.exe debug -p ssa tests/117-tuples.vn
.\target\release\vn.exe debug -p bytecode tests/117-tuples.vn
```

Comparar el mayor registro que aparece en el bytecode con el tamaño de frame
declarado del proto `<module>`. El registro fuera de rango es el primero de la
ventana de argumentos de `BuildTuple` (`call_base + i`, igual que
`BuildArray` en `ssa/emit/values.rs`). La causa raíz es que la ventana
`call_base..call_base+n` de `BuildTuple` no se suma al tamaño del frame
cuando es el último uso del módulo. Corregir en el cálculo de tamaño de frame
(una sola función), no añadiendo un registro extra en `ExecCtx::new`.

- [ ] **Step 4: Verde**

Run: `.\target\release\vn.exe run tests/117-tuples.vn` → `[PASSED] 117. Tuples`,
luego GATE-SUITE.

- [ ] **Step 5: Commit**

```bash
git add tests/117-tuples.vn tests/main.vn <archivo corregido>
git commit -m "fix(compiler): count BuildTuple argument window in frame size"
```

---

### Tarea 0.4: Crash de intersecciones

Repro verificado: `function f(x: A & B): int { return x.a() + x.b() }` →
`panicked at crates\varn-core\src\atom.rs:46:22: index out of bounds: the len is 237 but the index is 237`,
backtrace `resolve_type_node` → `CheckerTyTable::get_list` →
`BindResult::get_interface_members_local`. Un `Atom` de una sesión se resuelve
con el interner de otra: violación de Ley 2.

**Files:**
- Create: `tests/118-intersections.vn`
- Modify: `tests/main.vn`
- Modify: `crates/varn-checker/src/binder/types.rs` (`get_interface_members_local`)
  y/o `crates/varn-checker/src/binder/type_resolution/mod.rs` según el Step 3

- [ ] **Step 1: Test que falla**

`tests/118-intersections.vn`:

```varn
interface Named { name(): str }
interface Aged { age(): int }

class Person {
    constructor(private n: str, private a: int) {}
    name(): str { return this.n }
    age(): int { return this.a }
}

function describe(x: Named & Aged): str {
    return x.name() + ":" + x.age().toString()
}

assert("intersection param", describe(new Person("ana", 30)) === "ana:30")
print("[PASSED] 118. Intersections")
```

Si la sintaxis `constructor(private n: str, ...)` no existe, declarar campos
`n: str` y `a: int` y asignarlos en el constructor (ver `tests/12-classes.vn`).

- [ ] **Step 2: Verificar que falla**

Run: `.\target\release\vn.exe run tests/118-intersections.vn`
Expected: panic en `atom.rs:46`.

- [ ] **Step 3: Localizar**

Build debug con backtrace completo:

```powershell
cargo build -p varn-cli
$env:RUST_BACKTRACE = "1"; .\target\debug\vn.exe check tests/118-intersections.vn
```

En `get_interface_members_local`, identificar qué `Atom` se resuelve y de qué
`AtomInterner` proviene. El `TyListId` de `Intersection(C)` lleva
`CheckerTyId` de los miembros; resolver cada uno a `Named(atom, origin)` y
buscar la interfaz con `interner.resolve(atom)` usando el interner **del
`BindResult` dueño de ese id**. La corrección es resolver por nombre
(`Arc<str>`) en la frontera, como exige Ley 2, no indexar un interner ajeno.

- [ ] **Step 4: Verde**

`.\target\release\vn.exe run tests/118-intersections.vn` → `[PASSED] 118...`;
GATE-SUITE.

- [ ] **Step 5: Commit**

```bash
git add tests/118-intersections.vn tests/main.vn crates/varn-checker/src/binder
git commit -m "fix(checker): resolve intersection members by name, not foreign atom"
```

---

# FASE A

### Tarea A.1: Errores runtime tipados `IntegerOverflow` / `DivisionByZero`

**Files:**
- Create: `crates/varn-core/src/errors.rs`
- Modify: `crates/varn-core/src/lib.rs` (`pub mod errors; pub use errors::RuntimeErrorKind;`)
- Modify: `crates/varn-vm/src/error.rs:11-26`
- Modify: `crates/varn-vm/src/exec/exceptions.rs:82-113`
- Modify: `crates/varn-vm/src/exec/arith.rs:7-26, 139-184`
- Modify: `crates/varn-vm/src/exec/dispatch/ops_math_cmp.rs:12-21` y cada `RuntimeError::new("division by zero" | "modulo by zero")`
- Modify: `crates/varn-vm/src/exec/ctx.rs:264-278` (registro de clases)
- Modify: `crates/varn-builtins/src/modules/globals/globals.vn:15-21`, `globals.rs:120-152`
- Create: `tests/119-numeric-errors.vn`; Modify: `tests/main.vn`

**Interfaces:**
- Produces:
  - `varn_core::RuntimeErrorKind { Error, IntegerOverflow, DivisionByZero }`,
    `fn class_name(self) -> &'static str`.
  - `varn_vm::error::RuntimeError { message, frames, thrown, kind: RuntimeErrorKind }`.
  - `RuntimeError::integer_overflow(msg: impl Into<String>) -> Self`,
    `RuntimeError::division_by_zero(msg: impl Into<String>) -> Self`.

- [ ] **Step 1: Test `.vn` que falla**

`tests/119-numeric-errors.vn`:

```varn
function mul(a: int, b: int): int { return a * b }
function quo(a: int, b: int): int { return a % b }

function kindOf(f: () => int): str {
    try {
        f()
        return "none"
    } catch (e) {
        if (e instanceof IntegerOverflow) { return "IntegerOverflow" }
        if (e instanceof DivisionByZero) { return "DivisionByZero" }
        return "Error"
    }
}

assert("mul overflow is IntegerOverflow", kindOf(() => mul(4000000000, 4000000000)) === "IntegerOverflow")
assert("mod zero is DivisionByZero", kindOf(() => quo(1, 0)) === "DivisionByZero")

let caught: bool = false
try {
    mul(9223372036854775807, 2)
} catch (e) {
    caught = e instanceof Error && e.name === "IntegerOverflow"
}
assert("IntegerOverflow extends Error and has name", caught)

print("[PASSED] 119. Numeric errors")
```

Añadir `import "./119-numeric-errors.vn"` a `tests/main.vn`.

- [ ] **Step 2: Verificar que falla**

Run: `.\target\release\vn.exe run tests/119-numeric-errors.vn`
Expected: error de checker `undefined variable: IntegerOverflow` (VN3002).

- [ ] **Step 3: `RuntimeErrorKind` en `varn-core`**

`crates/varn-core/src/errors.rs`:

```rust
//! Clase de plataforma de un error nacido en el runtime. Se decide donde nace
//! el error y viaja con él; el `catch` la materializa sin re-derivarla del texto.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeErrorKind {
    #[default]
    Error,
    IntegerOverflow,
    DivisionByZero,
}

impl RuntimeErrorKind {
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::IntegerOverflow => "IntegerOverflow",
            Self::DivisionByZero => "DivisionByZero",
        }
    }

    pub const ALL: [Self; 3] = [Self::Error, Self::IntegerOverflow, Self::DivisionByZero];
}

#[cfg(test)]
mod tests {
    use super::RuntimeErrorKind;

    #[test]
    fn class_names_are_distinct() {
        let names: Vec<_> = RuntimeErrorKind::ALL.iter().map(|k| k.class_name()).collect();
        let mut dedup = names.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(names.len(), dedup.len());
    }
}
```

En `crates/varn-core/src/lib.rs` añadir `pub mod errors;` junto a los demás
`pub mod` y `pub use errors::RuntimeErrorKind;` junto a los `pub use`.

Run: `cargo test -p varn-core errors` → PASS.

- [ ] **Step 4: `RuntimeError.kind`**

`crates/varn-vm/src/error.rs`:

```rust
pub struct RuntimeError {
    pub message: String,
    pub frames: Vec<FrameInfo>,
    pub thrown: Option<crate::value::VmValue>,
    pub kind: varn_core::RuntimeErrorKind,
}

impl RuntimeError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::Error, message)
    }

    pub(crate) fn integer_overflow(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::IntegerOverflow, message)
    }

    pub(crate) fn division_by_zero(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::DivisionByZero, message)
    }

    fn of_kind(kind: varn_core::RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            frames: Vec::new(),
            thrown: None,
            kind,
        }
    }
}
```

En `crates/varn-vm/src/exec/exceptions.rs::build_thrown_error` (literal
`RuntimeError { .. }` en la línea 85) añadir `kind: varn_core::RuntimeErrorKind::Error,`
(un `throw` del usuario ya trae su valor; `kind` no se consulta cuando
`thrown` es `Some`).

- [ ] **Step 5: Los sitios de overflow y división usan su clase**

`crates/varn-vm/src/exec/arith.rs`: en `overflow` y `overflow_neg` cambiar
`RuntimeError::new(` por `RuntimeError::integer_overflow(`. En `div` y
`modulo` cambiar cada `RuntimeError::new("division by zero")` por
`RuntimeError::division_by_zero("division by zero")` y cada
`RuntimeError::new("modulo by zero")` por
`RuntimeError::division_by_zero("modulo by zero")`.

`crates/varn-vm/src/exec/dispatch/ops_math_cmp.rs`: `int_overflow` →
`crate::error::RuntimeError::integer_overflow(format!(...))`; cada
`crate::error::RuntimeError::new("division by zero")`/`("modulo by zero")` →
`::division_by_zero(...)`.

Verificación de que no queda ninguno:

Run: `rg -n 'RuntimeError::new\("(division|modulo) by zero"\)|RuntimeError::new\(format!\(\s*"integer overflow' crates/varn-vm`
Expected: sin resultados.

- [ ] **Step 6: Clases de plataforma**

`crates/varn-builtins/src/modules/globals/globals.vn`, tras `RangeError`:

```varn
export declare class IntegerOverflow extends Error {
    constructor(message?: str);
}

export declare class DivisionByZero extends Error {
    constructor(message?: str);
}
```

`crates/varn-builtins/src/modules/globals/globals.rs`, tras `RangeErrorClass`:

```rust
pub struct IntegerOverflowClass;

varn_contract! {
    module: "globals",
    class: "IntegerOverflow",
    extends: "Error",
    contract: "src/modules/globals/globals.vn",
    impl IntegerOverflowClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "IntegerOverflow");
            this
        }
    }
}

pub struct DivisionByZeroClass;

varn_contract! {
    module: "globals",
    class: "DivisionByZero",
    extends: "Error",
    contract: "src/modules/globals/globals.vn",
    impl DivisionByZeroClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "DivisionByZero");
            this
        }
    }
}
```

- [ ] **Step 7: Registrar y materializar por clase**

`crates/varn-vm/src/exec/ctx.rs` (array `names`, línea ~264): tras
`IntrinsicType::RangeError.as_str(),` añadir:

```rust
            varn_core::RuntimeErrorKind::IntegerOverflow.class_name(),
            varn_core::RuntimeErrorKind::DivisionByZero.class_name(),
```

`crates/varn-vm/src/exec/exceptions.rs::thrown_value_for`:

```rust
pub(crate) fn thrown_value_for(err: &RuntimeError, heap: &mut Heap) -> VmValue {
    if let Some(v) = err.thrown {
        return v;
    }
    let msg = heap.alloc_str_dynamic(&err.message);
    let class_name = err.kind.class_name();
    let Some(cls) = heap.get_intrinsic_class(class_name) else {
        return msg;
    };
    let oref = varn_types::value::ObjRef::instance(&cls);
    oref.set_field_nv(std::sync::Arc::from("message"), msg);
    let name = heap.alloc_str_dynamic(class_name);
    oref.set_field_nv(std::sync::Arc::from("name"), name);
    VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref)))
}
```

Borrar el `use` de `IntrinsicType` en `exceptions.rs` si queda sin uso.

- [ ] **Step 8: Verde en ambos tiers**

```powershell
cargo build --release -p varn-cli
.\target\release\vn.exe run tests/119-numeric-errors.vn
$env:VARN_NO_JIT="1"; .\target\release\vn.exe run tests/119-numeric-errors.vn; Remove-Item Env:VARN_NO_JIT
```

Expected: `[PASSED] 119. Numeric errors` dos veces. Si el tier JIT da
`"Error"`: `jit_propagate_error` (`jit_helpers/construct.rs:12`) guarda
`e.thrown.unwrap_or(null)`; el valor capturado debe salir de
`thrown_value_for(&e, heap)` también ahí — reemplazar esa línea por
`ctx.jit_panic_exception_error = Some(crate::exec::exceptions::thrown_value_for(&e, &mut ctx.heap));`.

Luego GATE-CHECK, GATE-SUITE, GATE-ERRORS.

- [ ] **Step 9: Commit**

```bash
git add crates/varn-core/src/errors.rs crates/varn-core/src/lib.rs crates/varn-vm crates/varn-builtins/src/modules/globals tests/119-numeric-errors.vn tests/main.vn
git commit -m "feat(runtime): raise IntegerOverflow and DivisionByZero as platform error classes"
```

---

### Tarea A.2: Casos límite enteros sin pánico (`MIN % -1`)

Repro verificado: `(-2^63) % -1` tumba el proceso
(`ops_math_cmp.rs:295`, "attempt to calculate the remainder with overflow").
D8: el resultado es `0`.

**Files:**
- Modify: `crates/varn-core/src/numeric.rs` (nuevas `rem_int`, `div_int`)
- Modify: `crates/varn-vm/src/exec/dispatch/ops_math_cmp.rs` (`ModInt`, ambas ramas)
- Modify: `crates/varn-vm/src/exec/arith.rs::modulo`
- Modify: `crates/varn-compiler/src/passes/const_fold.rs:141-147`
- Modify: `tests/119-numeric-errors.vn`

**Interfaces:**
- Produces (en `varn_core`, re-exportadas desde `lib.rs` junto a `add_int`):
  - `pub enum IntDivFault { DivisionByZero, Overflow }`
  - `pub fn div_int(a: i64, b: i64) -> Result<i64, IntDivFault>` — trunca.
  - `pub fn rem_int(a: i64, b: i64) -> Result<i64, IntDivFault>` — signo del dividendo; `MIN % -1 = 0`.

- [ ] **Step 1: Tests unitarios que fallan**

Al final de `crates/varn-core/src/numeric.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div_int_truncates_toward_zero() {
        assert_eq!(div_int(7, 2), Ok(3));
        assert_eq!(div_int(-7, 2), Ok(-3));
        assert_eq!(div_int(7, -2), Ok(-3));
    }

    #[test]
    fn div_int_faults() {
        assert_eq!(div_int(1, 0), Err(IntDivFault::DivisionByZero));
        assert_eq!(div_int(i64::MIN, -1), Err(IntDivFault::Overflow));
    }

    #[test]
    fn rem_int_follows_dividend_sign_and_never_overflows() {
        assert_eq!(rem_int(-7, 2), Ok(-1));
        assert_eq!(rem_int(7, -2), Ok(1));
        assert_eq!(rem_int(i64::MIN, -1), Ok(0));
        assert_eq!(rem_int(1, 0), Err(IntDivFault::DivisionByZero));
    }
}
```

Run: `cargo test -p varn-core numeric` → FAIL (`div_int` no existe).

- [ ] **Step 2: Implementar**

En `crates/varn-core/src/numeric.rs`, junto a `neg_int`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntDivFault {
    DivisionByZero,
    Overflow,
}

/// `a / b` truncado hacia cero.
#[inline(always)]
pub fn div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    a.checked_div(b).ok_or(IntDivFault::Overflow)
}

/// Resto con el signo del dividendo. `MIN % -1` es exactamente 0: el cociente
/// desborda, el resto no.
#[inline(always)]
pub fn rem_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a.wrapping_rem(b))
}
```

En `crates/varn-core/src/lib.rs`, en el `pub use numeric::{...}` añadir
`div_int, rem_int, IntDivFault`.

Run: `cargo test -p varn-core numeric` → PASS.

- [ ] **Step 3: VM usa `rem_int`**

En `crates/varn-vm/src/exec/arith.rs` añadir, junto a `overflow`:

```rust
#[cold]
#[inline(never)]
pub(crate) fn int_div_fault(fault: varn_core::IntDivFault, op: &str, a: i64, b: i64) -> RuntimeError {
    match fault {
        varn_core::IntDivFault::DivisionByZero => {
            RuntimeError::division_by_zero(if op == "%" { "modulo by zero" } else { "division by zero" })
        }
        varn_core::IntDivFault::Overflow => overflow(op, a, b),
    }
}
```

`arith::modulo`, rama int:

```rust
    if heap.is_int(a) && heap.is_int(b) {
        let (x, y) = (heap.as_int(a), heap.as_int(b));
        return varn_core::rem_int(x, y)
            .map(|r| heap.make_int(r))
            .map_err(|f| int_div_fault(f, "%", x, y));
    }
```

`ops_math_cmp.rs`, `OpCode::ModInt` — en **las dos** ramas (registro
`int_pair` y `box_reg`), reemplazar el chequeo de cero + `a_val % b_val` por:

```rust
let r = varn_core::rem_int(a_val, b_val)
    .map_err(|f| crate::exec::arith::int_div_fault(f, "%", a_val, b_val))?;
w!(first_reg, VmValue::from_int(r));
```

- [ ] **Step 4: Const-fold coherente**

`crates/varn-compiler/src/passes/const_fold.rs`, brazo `Mod` de
`(ConstInt, ConstInt)`:

```rust
            Mod => varn_core::rem_int(*x, *y).ok().map(InstKind::ConstInt),
```

- [ ] **Step 5: Test `.vn`**

Añadir a `tests/119-numeric-errors.vn` antes del `print` final:

```varn
function rem(a: int, b: int): int { return a % b }
const MIN: int = 0 - 9223372036854775807 - 1
assert("MIN % -1 is 0", rem(MIN, 0 - 1) === 0)
assert("rem sign follows dividend", rem(0 - 7, 2) === 0 - 1)
```

Run: GATE-SUITE → PASS (antes: panic del proceso).

- [ ] **Step 6: Commit**

```bash
git add crates/varn-core/src/numeric.rs crates/varn-core/src/lib.rs crates/varn-vm/src/exec crates/varn-compiler/src/passes/const_fold.rs tests/119-numeric-errors.vn
git commit -m "fix(vm): i64::MIN % -1 yields 0 instead of aborting the process"
```

---

### Tarea A.3: DCE no borra operaciones que pueden lanzar

Repro verificado: `function m(a: int, b: int): int { return a * b }` y
`try { m(4000000000, 4000000000) } catch (e) { print("caught") }` no imprime
nada: `dce.rs:127-148` clasifica `Add/Sub/Mul` de `int` tipado como puras.
§2.1: el chequeo solo puede eliminarse si el overflow es imposible.

**Files:**
- Modify: `crates/varn-compiler/src/passes/dce.rs:127-155`
- Modify: `tests/119-numeric-errors.vn`

- [ ] **Step 1: Test `.vn` que falla**

Añadir a `tests/119-numeric-errors.vn`:

```varn
function mulDiscard(a: int, b: int): int { return a * b }
let discardedCaught: bool = false
try {
    mulDiscard(4000000000, 4000000000)
} catch (e) {
    discardedCaught = e instanceof IntegerOverflow
}
assert("overflow is observable even when the result is unused", discardedCaught)

function negDiscard(a: int): int { return -a }
let negCaught: bool = false
try {
    negDiscard(0 - 9223372036854775807 - 1)
} catch (e) {
    negCaught = e instanceof IntegerOverflow
}
assert("negation overflow is observable", negCaught)
```

Run: `.\target\release\vn.exe run tests/119-numeric-errors.vn`
Expected: FAIL en `overflow is observable even when the result is unused`.

- [ ] **Step 2: Unit test de pureza**

Al final de `crates/varn-compiler/src/passes/dce.rs` (o en su módulo de tests
si existe; si el archivo supera 400 líneas, crear `passes/dce_tests.rs` con
`#[cfg(test)] #[path = "dce_tests.rs"] mod tests;`):

```rust
#[cfg(test)]
mod purity_tests {
    use super::is_pure;
    use crate::hir::{HirBinOp, HirType, HirUnOp};
    use crate::ssa::ir::{InstKind, Value};

    fn bin(op: HirBinOp, ty: HirType) -> InstKind {
        InstKind::Binary { op, lhs: Value(0), rhs: Value(1), ty }
    }

    #[test]
    fn checked_int_arithmetic_is_not_pure() {
        for op in [HirBinOp::Add, HirBinOp::Sub, HirBinOp::Mul] {
            assert!(!is_pure(&bin(op, HirType::Int)), "{op:?} int can overflow");
        }
        assert!(!is_pure(&InstKind::Unary { op: HirUnOp::Neg, operand: Value(0), ty: HirType::Int }));
    }

    #[test]
    fn float_and_comparison_arithmetic_stay_pure() {
        for op in [HirBinOp::Add, HirBinOp::Sub, HirBinOp::Mul] {
            assert!(is_pure(&bin(op, HirType::Float)));
        }
        assert!(is_pure(&bin(HirBinOp::Lt, HirType::Int)));
        assert!(is_pure(&bin(HirBinOp::BitAnd, HirType::Int)));
    }
}
```

Ajustar los nombres de campos de `InstKind::Binary`/`Unary`/`Value` a los de
`crates/varn-compiler/src/ssa/ir.rs` si difieren (leer la definición primero).

Run: `cargo test -p varn-compiler purity_tests` → FAIL.

- [ ] **Step 3: Implementar**

En `is_pure`, reemplazar el brazo `Binary` y el brazo `Unary`:

```rust
        Binary { op, ty, .. } => {
            let int_can_overflow = *ty == HirType::Int
                && matches!(op, HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul);
            let never_traps = matches!(
                op,
                HirBinOp::Add
                    | HirBinOp::Sub
                    | HirBinOp::Mul
                    | HirBinOp::Eq
                    | HirBinOp::Ne
                    | HirBinOp::Lt
                    | HirBinOp::Le
                    | HirBinOp::Gt
                    | HirBinOp::Ge
                    | HirBinOp::BitAnd
                    | HirBinOp::BitOr
                    | HirBinOp::BitXor
                    | HirBinOp::Shl
                    | HirBinOp::Shr
                    | HirBinOp::Ushr
            );
            let typed = matches!(ty, HirType::Int | HirType::Float | HirType::Bool);
            typed && never_traps && !int_can_overflow
        }
        Unary { op, ty, .. } => match op {
            HirUnOp::Typeof => true,
            HirUnOp::Neg => matches!(ty, HirType::Float),
            HirUnOp::Not | HirUnOp::BitNot => {
                matches!(ty, HirType::Int | HirType::Float | HirType::Bool)
            }
        },
```

Actualizar el doc-comment de `is_pure`: añadir a la frase de trampas medidas
"`+ - *` y `-x` sobre `int` desbordan".

- [ ] **Step 4: Verde**

`cargo test -p varn-compiler purity_tests` → PASS; GATE-SUITE.

- [ ] **Step 5: Medir (Ley 10)**

Run: `cargo xtask compare --only fib,loop,nbody` (o el subconjunto que exista;
`cargo xtask compare --help`). Registrar en el commit si algún bench empeora
> 10 %. Una regresión real aquí es aceptable (corrección) pero debe constar.

- [ ] **Step 6: Commit**

```bash
git add crates/varn-compiler/src/passes/dce.rs tests/119-numeric-errors.vn
git commit -m "fix(compiler): keep overflow-checked int ops alive in DCE"
```

---

### Tarea A.4: `int / int → int`

**Files:**
- Modify: `crates/varn-core/src/numeric.rs:15-24` (doc), `:104-109` (`binary_result_kind`)
- Modify: `crates/varn-vm/src/exec/dispatch/ops_math_cmp.rs:262-285` (`DivInt`)
- Modify: `crates/varn-vm/src/exec/arith.rs:139-156` (`div`)
- Modify: `crates/varn-compiler/src/passes/const_fold.rs:133-140`
- Modify: `crates/varn-jit/src/clif/from_ssa/scalar.rs:20-27, 158-167`, `boxed.rs:25-65`
- Modify: `crates/varn-checker/src/checker/refine.rs:159-162` (doc)
- Modify: `std/collections/priority_queue.vn:62`, `std/time/core.vn:32`, `std/encoding/toml.vn:47-49` y cada sitio que el barrido del Step 2 encuentre
- Modify: `tests/01-arithmetic.vn` y demás tests que dependan de `int / int` flotante
- Create: `tests/120-int-division.vn`

**Interfaces:**
- Consumes: `varn_core::div_int`, `IntDivFault` (A.2), `arith::int_div_fault` (A.2).
- Produces: `binary_result_kind(Div, Int) == Int`.

- [ ] **Step 1: Test `.vn` que falla**

`tests/120-int-division.vn`:

```varn
function d(a: int, b: int): int { return a / b }

assert("5 / 2 is 2", d(5, 2) === 2)
assert("-7 / 2 truncates to -3", d(0 - 7, 2) === 0 - 3)
assert("7 / -2 truncates to -3", d(7, 0 - 2) === 0 - 3)
const literalDiv: int = 9 / 4
assert("literal int division is int", literalDiv === 2)

let dz: str = "none"
try { d(1, 0) } catch (e) { if (e instanceof DivisionByZero) { dz = "zero" } }
assert("int / 0 raises DivisionByZero", dz === "zero")

let ov: str = "none"
try { d(0 - 9223372036854775807 - 1, 0 - 1) } catch (e) { if (e instanceof IntegerOverflow) { ov = "ovf" } }
assert("MIN / -1 raises IntegerOverflow", ov === "ovf")

const dyn: dynamic = 7
assert("dynamic int / int stays int", dyn / 2 === 3)

const f: float = 5.0 / 2.0
assert("float division unchanged", f === 2.5)

print("[PASSED] 120. Int division")
```

Añadir el import a `tests/main.vn`.

Run: `.\target\release\vn.exe check tests/120-int-division.vn`
Expected: FAIL — `declared as 'int' but initialised with 'float'` en la función `d`.

- [ ] **Step 2: Barrido previo (lista de sitios afectados)**

Antes de cambiar la regla, listar cada división cuyos dos operandos son
`int`. Añadir **temporalmente** en
`crates/varn-checker/src/checker_expressions/check/mod.rs`, dentro del brazo
`ExprKind::Binary` después de calcular `l_base`/`r_base`:

```rust
if op == BinaryOp::Div && l_base == Type::Int && r_base == Type::Int {
    eprintln!("INT_DIV_SWEEP {:?}", range);
}
```

```powershell
cargo build --release -p varn-cli 2>&1 | Select-String INT_DIV_SWEEP > $env:TEMP\int_div_sweep.txt
Get-ChildItem tests -Recurse -Filter *.vn | ForEach-Object { .\target\release\vn.exe check $_.FullName 2>&1 | Select-String INT_DIV_SWEEP } >> $env:TEMP\int_div_sweep.txt
```

Revisar cada sitio: si el autor esperaba un `float`, el sitio se reescribe en
el Step 7 (`a as float / b as float`, o literales `.0`). **Borrar el
`eprintln!` antes de seguir** (no se commitea).

- [ ] **Step 3: La regla**

`crates/varn-core/src/numeric.rs`:

```rust
/// Result class of an arithmetic op whose operands share class `operands`.
/// Division keeps the operand domain (spec §10): there is no operator whose
/// result class differs from its operands.
pub fn binary_result_kind(_op: BinaryOp, operands: NumericOperand) -> NumericOperand {
    operands
}
```

Y el doc del módulo: reemplazar la viñeta "`int / int` always produces a
`float`..." por:

```text
//! - `int / int` is an `int`, truncated toward zero (spec §10). A zero divisor
//!   raises `DivisionByZero`; `INT_MIN / -1` raises `IntegerOverflow`.
//! - `int % int` has the sign of the dividend; `INT_MIN % -1 == 0`.
```

Añadir a `mod tests`:

```rust
    #[test]
    fn division_keeps_the_operand_domain() {
        for k in [NumericOperand::Int, NumericOperand::Float, NumericOperand::Decimal] {
            assert_eq!(binary_result_kind(BinaryOp::Div, k), k);
        }
    }
```

Run: `cargo test -p varn-core numeric` → PASS.

- [ ] **Step 4: VM**

`ops_math_cmp.rs`, `OpCode::DivInt`, ambas ramas:

```rust
let r = varn_core::div_int(a_val, b_val)
    .map_err(|f| crate::exec::arith::int_div_fault(f, "/", a_val, b_val))?;
w!(first_reg, VmValue::from_int(r));
```

`arith::div` (camino genérico/dinámico, D7):

```rust
pub(crate) fn div(a: VmValue, b: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if heap.is_int(a) && heap.is_int(b) {
        let (x, y) = (heap.as_int(a), heap.as_int(b));
        return varn_core::div_int(x, y)
            .map(VmValue::from_int)
            .map_err(|f| int_div_fault(f, "/", x, y));
    }
    if a.is_heap() || b.is_heap() {
        if let Some((x, y)) = decimal_pair(a, b, heap) {
            if y.is_zero() {
                return Err(RuntimeError::division_by_zero("division by zero"));
            }
            return Ok(heap.alloc_decimal(x / y));
        }
    }
    let bv = heap.to_f64_val(b);
    if bv == 0.0 {
        return Err(RuntimeError::division_by_zero("division by zero"));
    }
    Ok(VmValue::from_f64(heap.to_f64_val(a) / bv))
}
```

(El chequeo de cero en `float` desaparece en A.5; aquí solo cambia la rama int.)

- [ ] **Step 5: Const-fold**

`const_fold.rs`, brazo `Div` de `(ConstInt, ConstInt)`:

```rust
            Div => varn_core::div_int(*x, *y).ok().map(InstKind::ConstInt),
```

- [ ] **Step 6: JIT**

`crates/varn-jit/src/clif/from_ssa/scalar.rs`:
- Doc de `emit_inst` (líneas 20-27): borrar la frase "`int / int` has the
  `DivInt` opcode but a `float` result." y el parámetro razonado por ella se
  mantiene solo si otro op lo usa.
- Líneas 158-167: `IntDiv` ya no produce float:

```rust
    // Integer division/mod/power and float mod/power keep the VM's exact
    // semantics (faults included) through a runtime helper.
    if matches!(op, IntDiv | IntMod | IntPow | FloatMod | FloatPow) {
        let dest_float = matches!(op, FloatMod | FloatPow);
        return boxed::emit_bin(b, ctx, op, a, c, dest_float);
    }
```

Borrar el parámetro `dest_ty` de la función si queda sin uso (y de su
llamador). `boxed::emit_bin` no cambia: `helpers.div` → `jit_div` →
`arith::div`, que ya devuelve `int`.

- [ ] **Step 7: Migrar `std/` y tests**

Run: `cargo build --release -p varn-cli`
Expected: errores de tipo en `std/` si algún sitio asignaba `int / int` a
`float`. Para cada sitio de la lista del Step 2:

- `std/collections/priority_queue.vn:62`: `((current - 1) / 2) as int` →
  `(current - 1) / 2` (el `as int` era un parche de la regla vieja).
- `std/time/core.vn:32`: `((a - (a % b)) / b).toInt()` → `(a - (a % b)) / b`
  si `a`, `b` son `int`; revisar su firma.
- `std/encoding/toml.vn:47-49`: si `fractionVal`/`divisor` son `int` y se
  espera fracción, escribir `(fractionVal as float) / (divisor as float)`.
  (`as float` funciona hoy vía el hack `+0.0`; A.6 lo reemplaza sin cambiar
  la sintaxis.)
- Tests: actualizar aserciones que esperaban `2.5` de `5 / 2` a `2`, o
  convertir operandos a `float` si el test trata de división flotante.

Run: `cargo build --release -p varn-cli` → OK; GATE-SUITE → PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/varn-core crates/varn-vm crates/varn-compiler crates/varn-jit crates/varn-checker std tests
git commit -m "feat(lang)!: int / int is an int truncated toward zero (spec §10)"
```

---

### Tarea A.5: `float` IEEE en división y resto por cero

Repro verificado: `1.0 / 0.0` lanza `division by zero`. D9: `+Inf`.

**Files:**
- Modify: `crates/varn-vm/src/exec/arith.rs` (`div`, `modulo`, rama float)
- Modify: `crates/varn-vm/src/exec/dispatch/ops_math_cmp.rs` (`DivFloat`, `ModFloat` si chequean cero)
- Create: `tests/121-float-ieee.vn`; Modify: `tests/main.vn`

- [ ] **Step 1: Test que falla**

`tests/121-float-ieee.vn`:

```varn
function fdiv(a: float, b: float): float { return a / b }
function fmod(a: float, b: float): float { return a % b }

const inf: float = fdiv(1.0, 0.0)
assert("1/0 is +Infinity", inf === Infinity)
assert("-1/0 is -Infinity", fdiv(0.0 - 1.0, 0.0) === 0.0 - Infinity)
const nan: float = fdiv(0.0, 0.0)
assert("0/0 is NaN", nan !== nan)
const modNan: float = fmod(1.0, 0.0)
assert("1 % 0 is NaN", modNan !== modNan)

const negZero: float = 0.0 * (0.0 - 1.0)
assert("-0 equals +0", negZero === 0.0)
assert("1/-0 is -Infinity", fdiv(1.0, negZero) === 0.0 - Infinity)

const tiny: float = 5e-324
assert("subnormal survives", tiny > 0.0 && tiny / 2.0 === 0.0)

print("[PASSED] 121. Float IEEE")
```

Run: `.\target\release\vn.exe run tests/121-float-ieee.vn` → FAIL (`division by zero`).

- [ ] **Step 2: Implementar**

`arith::div`: borrar el bloque `let bv = ...; if bv == 0.0 { return Err(...) }`
y dejar `Ok(VmValue::from_f64(heap.to_f64_val(a) / heap.to_f64_val(b)))`.
`arith::modulo`: igual para la rama float final
(`Ok(VmValue::from_f64(heap.to_f64_val(a) % heap.to_f64_val(b)))`).
En `ops_math_cmp.rs`, si `DivFloat`/`ModFloat` tienen chequeo de cero, borrarlo.

Run: `rg -n '== 0\.0' crates/varn-vm/src/exec/arith.rs crates/varn-vm/src/exec/dispatch/ops_math_cmp.rs`
Expected: ningún chequeo de cero en ramas float.

- [ ] **Step 3: Verde** — GATE-SUITE (ambos tiers: el JIT ya emite `fdiv`).

- [ ] **Step 4: Commit**

```bash
git add crates/varn-vm tests/121-float-ieee.vn tests/main.vn
git commit -m "fix(vm): float division and remainder by zero follow IEEE 754"
```

---

### Tarea A.6: Conversiones reales (`NumConv` + `InstKind::Convert` + `OpCode::Convert`)

Hoy `Cast` baja a `Move` (`ssa/emit/values.rs:240-245`): `3.9 as int` y
`bigint as int` fallan con `cannot store 'float' in an int register`, y
`as float` depende del hack `v + 0.0` (`from_tir/build.rs:797-833`). Se
introduce **un** mecanismo de conversión guiado por datos; `Cast` queda solo
para cambios sin representación (clase ↔ interfaz, etc.).

**Files:**
- Create: `crates/varn-core/src/numeric_conv.rs`
- Modify: `crates/varn-core/src/lib.rs`, `crates/varn-core/src/opcode.rs` (nuevo `Convert`)
- Modify: `crates/varn-compiler/src/ssa/ir.rs` (nuevo `InstKind::Convert`)
- Modify: `crates/varn-compiler/src/ssa/{emit/values.rs,dump.rs,uses.rs,verify.rs,portable.rs}`
- Modify: `crates/varn-compiler/src/passes/{dce.rs,const_fold.rs}`
- Modify: `crates/varn-compiler/src/from_tir/build.rs:185-199` (`coerce`), `:797-838` (`Cast`)
- Modify: `crates/varn-types/src/ssa.rs` (nuevo `SsaOp::Convert`), `crates/varn-types/src/bytecode.rs`, `crates/varn-debug/src/bytecode.rs` (desensamblado)
- Create: `crates/varn-vm/src/exec/convert.rs`; Modify: `crates/varn-vm/src/exec/mod.rs`, `exec/dispatch/mod.rs`
- Modify: `crates/varn-vm/src/exec/jit_helpers/arith.rs` (nuevo `jit_convert`), `crates/varn-jit/src/helper_abi.rs`
- Modify: `crates/varn-jit/src/clif/from_ssa/scalar.rs` (brazo `SsaOp::Convert`)
- Modify: `crates/varn-lsp/src/features/compiler_inspect.rs` (dump)
- Create: `tests/122-numeric-conversions.vn`; Modify: `tests/main.vn`

**Interfaces:**
- Produces:
  - `varn_core::NumConv` `#[repr(u8)] { IntToFloat = 0, FloatToInt = 1, IntToBigInt = 2, BigIntToInt = 3, IntToDecimal = 4, DecimalToInt = 5 }`, `fn from_u8(u8) -> Option<NumConv>`, `fn can_fault(self) -> bool`.
  - `varn_core::float_to_int(f: f64) -> Option<i64>` (D3).
  - `varn_core::NumConv::between(from: NumericDomain, to: NumericDomain) -> Option<NumConv>` con `pub enum NumericDomain { Int, Float, BigInt, Decimal }`.
  - `InstKind::Convert { operand: Value, conv: varn_core::NumConv }`.
  - `SsaOp::Convert { operand: u32, conv: varn_core::NumConv }`.
  - Bytecode: `OpCode::Convert`, operandos `pack(dest, src)` + palabra `conv as u16`.
  - `varn_vm::exec::convert::convert(conv, v, heap) -> VmResult<VmValue>`.

- [ ] **Step 1: Test `.vn` que falla**

`tests/122-numeric-conversions.vn`:

```varn
function f2i(x: float): int { return x as int }
function i2f(x: int): float { return x as float }

assert("3.9 as int truncates", f2i(3.9) === 3)
assert("-3.9 as int truncates toward zero", f2i(0.0 - 3.9) === 0 - 3)
assert("int as float", i2f(2) === 2.0)
assert("large int as float rounds", i2f(9007199254740993) === 9007199254740992.0)

function faultOf(f: () => int): str {
    try { f(); return "none" } catch (e) {
        if (e instanceof IntegerOverflow) { return "ovf" }
        return "other"
    }
}
assert("NaN as int raises", faultOf(() => f2i(0.0 / 0.0)) === "ovf")
assert("Inf as int raises", faultOf(() => f2i(1.0 / 0.0)) === "ovf")
assert("1e19 as int raises", faultOf(() => f2i(1e19)) === "ovf")

const big: bigint = 42n
assert("small bigint as int", (big as int) === 42)
const huge: bigint = 99999999999999999999999n
assert("huge bigint as int raises", faultOf(() => huge as int) === "ovf")

const dec: decimal = 7.9d
assert("decimal as int truncates", (dec as int) === 7)

const fromDyn: dynamic = 5
assert("dynamic int as float converts", typeof (fromDyn as float) === "float")

print("[PASSED] 122. Numeric conversions")
```

Run: `.\target\release\vn.exe run tests/122-numeric-conversions.vn`
Expected: FAIL `cannot store 'float' in an int register`.

- [ ] **Step 2: `NumConv` con tests unitarios**

`crates/varn-core/src/numeric_conv.rs`:

```rust
//! Conversiones numéricas explícitas (`as`). Una tabla, consumida igual por
//! const-fold, VM y JIT: el `as` nunca es un `Move`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumericDomain {
    Int,
    Float,
    BigInt,
    Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum NumConv {
    IntToFloat = 0,
    FloatToInt = 1,
    IntToBigInt = 2,
    BigIntToInt = 3,
    IntToDecimal = 4,
    DecimalToInt = 5,
}

impl NumConv {
    pub const fn from_u8(raw: u8) -> Option<Self> {
        Some(match raw {
            0 => Self::IntToFloat,
            1 => Self::FloatToInt,
            2 => Self::IntToBigInt,
            3 => Self::BigIntToInt,
            4 => Self::IntToDecimal,
            5 => Self::DecimalToInt,
            _ => return None,
        })
    }

    pub const fn between(from: NumericDomain, to: NumericDomain) -> Option<Self> {
        use NumericDomain::*;
        Some(match (from, to) {
            (Int, Float) => Self::IntToFloat,
            (Float, Int) => Self::FloatToInt,
            (Int, BigInt) => Self::IntToBigInt,
            (BigInt, Int) => Self::BigIntToInt,
            (Int, Decimal) => Self::IntToDecimal,
            (Decimal, Int) => Self::DecimalToInt,
            _ => return None,
        })
    }

    /// Whether the conversion can raise, i.e. whether DCE must keep it.
    pub const fn can_fault(self) -> bool {
        matches!(self, Self::FloatToInt | Self::BigIntToInt | Self::DecimalToInt)
    }
}

/// `f as int`: truncates toward zero; `None` for NaN, ±Inf or out of range.
pub fn float_to_int(f: f64) -> Option<i64> {
    const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;
    let t = f.trunc();
    if t.is_nan() || t < -TWO_POW_63 || t >= TWO_POW_63 {
        return None;
    }
    Some(t as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_to_int_truncates_and_rejects_non_representable() {
        assert_eq!(float_to_int(3.9), Some(3));
        assert_eq!(float_to_int(-3.9), Some(-3));
        assert_eq!(float_to_int(-9_223_372_036_854_775_808.0), Some(i64::MIN));
        assert_eq!(float_to_int(9_223_372_036_854_775_808.0), None);
        assert_eq!(float_to_int(f64::NAN), None);
        assert_eq!(float_to_int(f64::INFINITY), None);
        assert_eq!(float_to_int(f64::NEG_INFINITY), None);
    }

    #[test]
    fn wire_roundtrip() {
        for c in [
            NumConv::IntToFloat,
            NumConv::FloatToInt,
            NumConv::IntToBigInt,
            NumConv::BigIntToInt,
            NumConv::IntToDecimal,
            NumConv::DecimalToInt,
        ] {
            assert_eq!(NumConv::from_u8(c as u8), Some(c));
        }
        assert_eq!(NumConv::from_u8(6), None);
    }
}
```

En `lib.rs`: `pub mod numeric_conv;` y
`pub use numeric_conv::{float_to_int, NumConv, NumericDomain};`.
Verificar que `varn-core` ya depende de `serde` (`TypeTag` lo deriva: sí).

Run: `cargo test -p varn-core numeric_conv` → PASS.

- [ ] **Step 3: Opcode y nodos IR**

- `crates/varn-core/src/opcode.rs`: añadir `Convert,` justo después de
  `CheckNarrowRange` (se borrará en A.8; mantener orden de discriminantes
  estable no es requisito porque `BUILD_FINGERPRINT` invalida cachés).
- `crates/varn-compiler/src/ssa/ir.rs`, junto a `Cast`:

```rust
    /// Explicit numeric conversion (`as`) that changes representation.
    Convert {
        operand: Value,
        conv: varn_core::NumConv,
    },
```

- `crates/varn-types/src/ssa.rs`, junto a `Cast { operand: u32 }`:

```rust
    Convert { operand: u32, conv: varn_core::NumConv },
```

Run: `cargo check --workspace --exclude varn-lsp`
Expected: errores de `match` no exhaustivo. Resolver **cada uno
explícitamente** (Ley 7) según los Steps 4–8; ninguno con `_ =>`.

- [ ] **Step 4: Compilador**

- `ssa/uses.rs`: `Convert { operand, .. }` en las dos listas donde está
  `NarrowRangeCheck { operand, .. }` (líneas ~51 y ~212).
- `ssa/dump.rs`: `InstKind::Convert { operand, conv } => format!("convert {} {conv:?}", val(*operand)),`.
- `ssa/verify.rs` (junto al brazo `Cast`, ~445): el resultado de `Convert`
  debe ser `HirType::Int` para `FloatToInt|BigIntToInt|DecimalToInt`,
  `HirType::Float` para `IntToFloat`, `HirType::Dynamic` para
  `IntToBigInt|IntToDecimal`; el operando, el simétrico. Mensaje de error:
  `format!("convert {conv:?}: operand {op_ty:?} / result {res_ty:?}")`.
- `ssa/portable.rs` (junto a `Cast`, ~267):
  `InstKind::Convert { operand, conv } => SsaOp::Convert { operand: operand.0, conv: *conv },`.
- `ssa/emit/values.rs` (junto a `Cast`):

```rust
        InstKind::Convert { operand, conv } => {
            chunk.emit(OpCode::Convert, line);
            chunk.write(Chunk::pack(d, reg[operand.0 as usize]), line);
            chunk.write(*conv as u16, line);
        }
```

- `passes/dce.rs::is_pure`: `Convert { conv, .. } => !conv.can_fault(),`.
- `passes/const_fold.rs`: plegar `Convert` sobre constantes:

```rust
fn fold_convert(conv: varn_core::NumConv, operand: &InstKind) -> Option<InstKind> {
    use varn_core::NumConv::*;
    match (conv, operand) {
        (IntToFloat, InstKind::ConstInt(n)) => Some(InstKind::ConstFloat(*n as f64)),
        (FloatToInt, InstKind::ConstFloat(f)) => varn_core::float_to_int(*f).map(InstKind::ConstInt),
        (IntToBigInt, InstKind::ConstInt(n)) => Some(InstKind::ConstBigInt(*n as i128)),
        (IntToDecimal, InstKind::ConstInt(n)) => Some(InstKind::ConstDecimal((*n).into())),
        (BigIntToInt, InstKind::ConstBigInt(b)) => i64::try_from(*b).ok().map(InstKind::ConstInt),
        (DecimalToInt, InstKind::ConstDecimal(d)) => {
            use rust_decimal::prelude::ToPrimitive;
            d.trunc().to_i64().map(InstKind::ConstInt)
        }
        (IntToFloat | FloatToInt | IntToBigInt | IntToDecimal | BigIntToInt | DecimalToInt, _) => None,
    }
}
```

  Engancharlo donde el pase recorre instrucciones (mismo lugar que llama al
  plegado de `Binary`), sustituyendo la instrucción cuando devuelve `Some`.
  Ajustar los tipos de `ConstBigInt`/`ConstDecimal` a los reales de `ir.rs`.

- [ ] **Step 5: `from_tir` emite `Convert`**

`crates/varn-compiler/src/from_tir/build.rs`:

1. Nueva función junto a `coerce`:

```rust
    fn numeric_domain(ty: HirType, backend: Option<BackendTy>) -> Option<varn_core::NumericDomain> {
        use varn_core::NumericDomain as D;
        match (ty, backend) {
            (HirType::Int, _) => Some(D::Int),
            (HirType::Float, _) => Some(D::Float),
            (_, Some(BackendTy::BigInt)) => Some(D::BigInt),
            (_, Some(BackendTy::Decimal)) => Some(D::Decimal),
            _ => None,
        }
    }
```

   (Si `from_tir` no tiene a mano el `BackendTy` de un `Value`, usar el del
   `TirExpr` operando — `operand.ty` — que es el que lleva `BigInt`/`Decimal`.)

2. `coerce`: cuando `value_ty(v)` y `target` son dominios numéricos distintos
   y `NumConv::between` da `Some(conv)`, emitir
   `InstKind::Convert { operand: v, conv }`; si no, `Cast` como hoy.

3. Brazo `TirExprKind::Cast { operand }`: **borrar** el bloque del hack
   `v + 0.0` completo (comentario incluido). Reemplazar por:

```rust
            TirExprKind::Cast { operand } => {
                let from = Self::numeric_domain(self.lower_ty_of(operand), Some(operand.ty));
                let v = self.lower_expr(operand)?;
                let to = Self::numeric_domain(ty, Some(e.ty));
                if let (Some(from), Some(to)) = (from, to) {
                    if from == to {
                        return Ok(v);
                    }
                    if let Some(conv) = varn_core::NumConv::between(from, to) {
                        return Ok(self.emit(InstKind::Convert { operand: v, conv }, ty));
                    }
                }
                if matches!(operand.ty, BackendTy::Dynamic(_)) {
                    if let Some(to) = to {
                        return Ok(self.emit_dynamic_convert(v, to, ty));
                    }
                }
                if let Some(tag) = narrow_tag_of(e.ty) {
                    let v = self.emit(InstKind::Cast { operand: v, ty }, ty);
                    return Ok(self.emit(InstKind::NarrowRangeCheck { operand: v, tag }, ty));
                }
                Ok(self.emit(InstKind::Cast { operand: v, ty }, ty))
            }
```

   `lower_ty_of(operand)` = la función existente que mapea `BackendTy` →
   `HirType` (en `from_tir/ty.rs`); usar su nombre real.

4. `emit_dynamic_convert(v, to, ty)`: el operando `dynamic` no tiene dominio
   estático; emitir `InstKind::Convert` con el `conv` "desde Int" **no** es
   correcto si en runtime es float. Para `dynamic` añadir al enum una variante
   por destino que despacha en runtime: extender `NumConv` con
   `DynToInt = 6, DynToFloat = 7` (actualizar `from_u8`, `can_fault` → ambas
   `true`, y el test `wire_roundtrip`). `emit_dynamic_convert` emite
   `Convert { conv: DynToInt | DynToFloat }` según `to`; otros destinos desde
   `dynamic` siguen como `Cast` hasta Fase B.

- [ ] **Step 6: VM**

`crates/varn-vm/src/exec/convert.rs`:

```rust
use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::NumConv;

#[cold]
#[inline(never)]
fn out_of_int_range(what: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::integer_overflow(format!("integer overflow: {what} does not fit int"))
}

fn float_to_int_value(f: f64) -> VmResult<VmValue> {
    varn_core::float_to_int(f)
        .map(VmValue::from_int)
        .ok_or_else(|| out_of_int_range(f))
}

pub(crate) fn convert(conv: NumConv, v: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    match conv {
        NumConv::IntToFloat => Ok(VmValue::from_f64(heap.as_int(v) as f64)),
        NumConv::FloatToInt => float_to_int_value(v.as_f64()),
        NumConv::IntToBigInt => Ok(heap.alloc_bigint(heap.as_int(v) as i128)),
        NumConv::IntToDecimal => Ok(heap.alloc_decimal(heap.as_int(v).into())),
        NumConv::BigIntToInt => match heap.get(v.as_heap_idx()) {
            Some(HeapObj::BigInt(b)) => i64::try_from(**b)
                .map(VmValue::from_int)
                .map_err(|_| out_of_int_range(**b)),
            other => Err(RuntimeError::new(format!("convert: expected bigint, got {other:?}"))),
        },
        NumConv::DecimalToInt => match heap.get(v.as_heap_idx()) {
            Some(HeapObj::Decimal(d)) => {
                use rust_decimal::prelude::ToPrimitive;
                d.trunc().to_i64().map(VmValue::from_int).ok_or_else(|| out_of_int_range(**d))
            }
            other => Err(RuntimeError::new(format!("convert: expected decimal, got {other:?}"))),
        },
        NumConv::DynToInt if heap.is_int(v) => Ok(v),
        NumConv::DynToInt if v.is_f64() => float_to_int_value(v.as_f64()),
        NumConv::DynToInt => convert_heap_numeric_to_int(v, heap),
        NumConv::DynToFloat if v.is_f64() => Ok(v),
        NumConv::DynToFloat if heap.is_int(v) => Ok(VmValue::from_f64(heap.as_int(v) as f64)),
        NumConv::DynToFloat => Ok(VmValue::from_f64(heap.to_f64_val(v))),
    }
}

fn convert_heap_numeric_to_int(v: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    let conv = match heap.get(v.as_heap_idx()) {
        Some(HeapObj::BigInt(_)) => NumConv::BigIntToInt,
        Some(HeapObj::Decimal(_)) => NumConv::DecimalToInt,
        _ => return Err(RuntimeError::new("convert: value is not numeric")),
    };
    convert(conv, v, heap)
}
```

Ajustar nombres a la API real del heap (`alloc_bigint`, `alloc_decimal`,
`as_f64`, `is_f64`): `rg -n "fn alloc_decimal|fn alloc_bigint|fn as_f64|fn is_f64" crates/varn-vm crates/varn-types`.
Si `convert.rs` necesita más de 400 líneas, no: debe quedar < 120.

`exec/mod.rs`: `pub(crate) mod convert;`.
`exec/dispatch/mod.rs`: brazo para `OpCode::Convert`, siguiendo el patrón de
lectura de palabras de `DivInt` (`let w1 = code[*ip]; *ip += 1;`):

```rust
            OpCode::Convert => {
                let w1 = code[*ip];
                let raw = code[*ip + 1];
                *ip += 2;
                let (dst, src) = (hi(w1), lo(w1));
                let conv = varn_core::NumConv::from_u8(raw as u8)
                    .ok_or_else(|| RuntimeError::new(format!("convert: bad operand {raw}")))?;
                let v = self.stack.box_reg(base, src);
                let r = crate::exec::convert::convert(conv, v, &mut self.heap)?;
                self.stack.write_boxed(base, dst, r);
            }
```

(Usar la macro/función de escritura que usan los opcodes vecinos para
escribir en un registro con su `SlotKind`; `w!` si está en alcance.)

`crates/varn-types/src/bytecode.rs` y `crates/varn-debug/src/bytecode.rs`:
añadir `Convert` a las tablas de longitud/desensamblado (2 palabras de
operando), en la misma forma que `CheckNarrowRange`.

- [ ] **Step 7: JIT**

`crates/varn-vm/src/exec/jit_helpers/arith.rs`:

```rust
pub(crate) extern "C" fn jit_convert(ctx: *mut ExecCtx, conv: u64, v_tag: u64, v_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(v_tag, v_payload);
        let Some(conv) = varn_core::NumConv::from_u8(conv as u8) else {
            jit_propagate_error(ctx_ref, crate::error::RuntimeError::new("convert: bad operand"));
        };
        match crate::exec::convert::convert(conv, v, &mut ctx_ref.heap) {
            Ok(r) => ctx_ref.jit_native_result = r,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
```

`crates/varn-jit/src/helper_abi.rs`: añadir `convert => jit_convert,` tras
`pow => jit_pow,`.

`crates/varn-jit/src/clif/from_ssa/scalar.rs::emit_inst`, nuevo brazo antes
de `SsaOp::Cast`:

```rust
        SsaOp::Convert { operand, conv } => {
            let a = load_value(b, ctx, values, *operand)?;
            match conv {
                varn_core::NumConv::IntToFloat => b.ins().fcvt_from_sint(types::F64, a),
                _ => return boxed::emit_convert(b, ctx, *conv, a, ctx.ssa.value_ty(*operand), dest_ty).map(Some),
            }
        }
```

`crates/varn-jit/src/clif/from_ssa/boxed.rs`:

```rust
pub(super) fn emit_convert(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    conv: varn_core::NumConv,
    a: Value,
    operand_kind: SlotKind,
    dest_kind: Option<SlotKind>,
) -> Result<Value, String> {
    let (tag, payload) = match operand_kind {
        SlotKind::Int => box_native(b, a, false),
        SlotKind::Float => box_native(b, a, true),
        _ => b.ins().isplit(a).into(),
    };
    let live = call_helper(b, ctx.cc, ctx.helpers.current_exec_ctx, &[]);
    let conv_v = b.ins().iconst(types::I64, conv as i64);
    call_helper_void(b, ctx.cc, ctx.helpers.convert, &[live, conv_v, tag, payload]);
    let boxed = b.ins().load(types::I128, MemFlags::trusted(), live, ctx.helpers.jit_native_result_offset as i32);
    Ok(match dest_kind {
        Some(SlotKind::Int) => unbox_int(b, boxed),
        Some(SlotKind::Float) => unbox_f64_coerce(b, boxed),
        _ => boxed,
    })
}
```

(Si un operando heap llega como `I128`, `isplit` devuelve `(tag, payload)`;
ajustar a cómo `heapvalue`/`load_value` lo entregan en este archivo.) El
lowering desde **bytecode** (`clif/body/op_dispatch.rs`) no maneja
`OpCode::Convert`: cae en `clif: unsupported opcode` y la función se queda
en el intérprete/SSA — correcto, sin código adicional.

- [ ] **Step 8: LSP**

`crates/varn-lsp/src/features/compiler_inspect.rs`: brazo
`Convert { operand, conv } => format!("convert v{} {conv:?}", operand.0)`
junto a `NarrowRangeCheck`.

- [ ] **Step 9: Verde**

```powershell
cargo test -p varn-core -p varn-compiler
cargo build --release -p varn-cli
.\target\release\vn.exe run tests/122-numeric-conversions.vn
$env:VARN_NO_JIT="1"; .\target\release\vn.exe run tests/122-numeric-conversions.vn; Remove-Item Env:VARN_NO_JIT
```

Expected: `[PASSED] 122...` en ambos. Luego GATE-CHECK, GATE-SUITE, GATE-ERRORS.
Confirmar que el bench del hack sigue coherente:
`.\target\release\vn.exe run tests/benchmarks/bench_csv_etl.vn` en ambos tiers
da la misma salida.

- [ ] **Step 10: Commit**

```bash
git add crates/varn-core crates/varn-compiler crates/varn-types crates/varn-vm crates/varn-jit crates/varn-debug crates/varn-lsp tests/122-numeric-conversions.vn tests/main.vn
git commit -m "feat(compiler): explicit numeric conversions lower to a real Convert op"
```

---

### Tarea A.7: Sin conversiones implícitas con pérdida; literales contextuales

**Files:**
- Modify: `crates/varn-core/src/numeric.rs:86-99` (`binary_operand_kind`)
- Modify: `crates/varn-checker/src/checker/compat/mod.rs:27-130` (`simple_types_compatible`, `literal_fits_type`), `:236-300` (`expr_satisfies_target_type`)
- Create: `crates/varn-checker/src/checker/compat/numeric_literal.rs` (adopción de literales; `compat/mod.rs` ya tiene 1226 líneas: no crecerlo)
- Modify: `crates/varn-checker/src/checker_expressions/check/mod.rs:420-445` (validez de operadores)
- Modify: `crates/varn-checker/src/binder/type_inference.rs:395-424, 426-444` (`numeric_binary_type`, `infer_binary`)
- Modify: `crates/varn-checker/src/checker/refine.rs:159-195`
- Modify: `tests/26-numeric-coercion.vn` (reescritura), `std/**` y `tests/**` que el build señale
- Create: `tests/errors/implicit-int-to-float-rejected.vn`, `tests/errors/mixed-int-float-arith-rejected.vn`, `tests/errors/float-to-decimal-rejected.vn`, `tests/errors/decimal-float-arith-rejected.vn`

**Interfaces:**
- Consumes: `NumConv` (A.6) — un literal adoptado baja por `coerce` → `Convert`, y const-fold lo pliega.
- Produces:
  - `binary_operand_kind`: `(Int, Float)`/`(Float, Int)` → `None`.
  - `pub(crate) fn int_literal_adopts(target: &Type, value: i64, table: &CheckerTyTable) -> bool` en `compat/numeric_literal.rs`.
  - `pub(crate) fn literal_operand_class(arena, expr, other: &Type, table) -> Option<Type>` — si `expr` es literal entero (con signo/paréntesis) y `other` es `float|decimal|bigint` y cabe, devuelve `other`.

- [ ] **Step 1: Fixtures negativos (fallan: hoy se aceptan)**

`tests/errors/implicit-int-to-float-rejected.vn`:

```varn
// expect: error[VN3001]
let i: int = 42
let f: float = i
```

`tests/errors/mixed-int-float-arith-rejected.vn`:

```varn
// expect: error[VN3010]
let a: int = 10
let b: float = 2.5
let c = a + b
```

`tests/errors/float-to-decimal-rejected.vn`:

```varn
// expect: error[VN3001]
let s: float = 1.5
let d: decimal = s
```

`tests/errors/decimal-float-arith-rejected.vn`:

```varn
// expect: error[VN3010]
let d = 1.5d
let f = 2.5
let x = d + f
```

Run: GATE-ERRORS → FAIL en los cuatro.

- [ ] **Step 2: Test positivo de literales (debe seguir pasando)**

Reescribir `tests/26-numeric-coercion.vn` completo:

```varn
// Conversiones numéricas (spec §5, §7, §9): solo int → bigint e int → decimal
// son implícitas; un literal entero exacto adopta el tipo del contexto.

const i: int = 42
const bi: bigint = i
assert("int to bigint implicit", bi.toString() === "42")
const d: decimal = i
assert("int to decimal implicit", d.toString() === "42")

const f: float = 5
assert("int literal adopts float", f === 5.0)
const neg: float = -3
assert("negative int literal adopts float", neg === 0.0 - 3.0)

function takesFloat(x: float): float { return x + 1 }
assert("literal arg adopts float", takesFloat(5) === 6.0)
assert("literal operand adopts float", takesFloat(1.5) * 2 === 5.0)

function takesDecimal(x: decimal): decimal { return x + 1 }
assert("literal adopts decimal", takesDecimal(3) === 4.0d)

const explicit: float = (i as float) + 2.5
assert("explicit int as float", explicit === 44.5)

const xs: float[] = [1, 2, 3]
assert("array literal elements adopt float", xs[2] === 3.0)

print("[PASSED] 26. Numeric coercion ")
```

- [ ] **Step 3: Regla de operandos**

`crates/varn-core/src/numeric.rs`:

```rust
pub fn binary_operand_kind(
    l: Option<NumericOperand>,
    r: Option<NumericOperand>,
) -> Option<NumericOperand> {
    use NumericOperand::*;
    match (l?, r?) {
        (Int, Int) => Some(Int),
        (Float, Float) => Some(Float),
        (Decimal, Decimal) | (Decimal, Int) | (Int, Decimal) => Some(Decimal),
        (Int, Float) | (Float, Int) | (Decimal, Float) | (Float, Decimal) => None,
    }
}
```

Doc de módulo: reemplazar "Mixed `int`/`float` operands promote to `float`"
por "Mixed `int`/`float` operands are a checker error (spec §9); an integer
*literal* adopts the other operand's type when exactly representable."

Tests en `mod tests`:

```rust
    #[test]
    fn int_float_mix_has_no_common_class() {
        use NumericOperand::*;
        assert_eq!(binary_operand_kind(Some(Int), Some(Float)), None);
        assert_eq!(binary_operand_kind(Some(Float), Some(Int)), None);
        assert_eq!(binary_operand_kind(Some(Int), Some(Decimal)), Some(Decimal));
    }
```

D7 (dinámico): `arith.rs` **no** cambia sus ramas mixtas: en runtime la
mezcla solo llega por `dynamic`.

- [ ] **Step 4: Adopción de literales en un módulo propio**

`crates/varn-checker/src/checker/compat/numeric_literal.rs`:

```rust
//! Un literal entero exacto adopta el tipo numérico de su contexto (spec §5).
//! Es la única conversión implícita hacia `float`: la de variables no existe.

use crate::types::{CheckerTyTable, Type};
use varn_core::ast::{AstArena, ExprId, ExprKind, UnaryOp};
use varn_core::{TypeKind, TypeTag};

const F64_EXACT_INT: i64 = 1 << 53;

pub(crate) fn const_int_value(arena: &AstArena, expr: ExprId) -> Option<i64> {
    match &arena.expr(expr).kind {
        ExprKind::IntLiteral { value, .. } => Some(*value),
        ExprKind::Paren { expression } => const_int_value(arena, *expression),
        ExprKind::Unary { op: UnaryOp::Minus, prefix: true, operand, .. } => {
            const_int_value(arena, *operand).and_then(i64::checked_neg)
        }
        ExprKind::Unary { op: UnaryOp::Plus, prefix: true, operand, .. } => {
            const_int_value(arena, *operand)
        }
        _ => None,
    }
}

pub(crate) fn int_literal_adopts(target: &Type, value: i64, table: &CheckerTyTable) -> bool {
    match table.get(target.0) {
        TypeKind::Intrinsic(TypeTag::Float) => (-F64_EXACT_INT..=F64_EXACT_INT).contains(&value),
        TypeKind::Intrinsic(TypeTag::Decimal | TypeTag::BigInt | TypeTag::Int) => true,
        _ => false,
    }
}

/// `other` when `expr` is an integer literal that can take `other`'s numeric type.
pub(crate) fn literal_operand_class(
    arena: &AstArena,
    expr: ExprId,
    other: &Type,
    table: &CheckerTyTable,
) -> Option<Type> {
    let value = const_int_value(arena, expr)?;
    let is_numeric_target = matches!(
        table.get(other.0),
        TypeKind::Intrinsic(TypeTag::Float | TypeTag::Decimal | TypeTag::BigInt)
    );
    (is_numeric_target && int_literal_adopts(other, value, table)).then_some(*other)
}
```

En `compat/mod.rs`: `mod numeric_literal;` + `pub(crate) use numeric_literal::{int_literal_adopts, literal_operand_class};`;
**borrar** la `const_int_value` local de `compat/mod.rs:136-153` y usar la del
módulo nuevo (una sola copia, Ley 6).

Unit tests en el mismo archivo:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_adopts_only_exact_integers() {
        let table = CheckerTyTable::new();
        assert!(int_literal_adopts(&Type::Float, 1 << 53, &table));
        assert!(!int_literal_adopts(&Type::Float, (1 << 53) + 1, &table));
        assert!(int_literal_adopts(&Type::Decimal, i64::MAX, &table));
        assert!(!int_literal_adopts(&Type::Str, 1, &table));
    }
}
```

(`Type::Float`/`Type::Decimal`/`Type::Str` son las constantes de
`types/type_impl.rs`; usar sus nombres reales.)

Run: `cargo test -p varn-checker numeric_literal` → PASS.

- [ ] **Step 5: Asignabilidad sin widening lossy**

`compat/mod.rs::simple_types_compatible`: borrar `Int` de la lista del brazo
`(Float, ...)` (el resto de ese brazo son anchos que A.8 borra) y borrar el
brazo `(Decimal, Float) => true`. Quedan `(Decimal, Int)` y `(BigInt, Int)`.

`compat/mod.rs::expr_satisfies_target_type`: antes del bloque
`if target_ty.is_granular_int()`, añadir:

```rust
    if let Some(value) = numeric_literal::const_int_value(arena, expr) {
        if numeric_literal::int_literal_adopts(target_ty, value, table) {
            return true;
        }
    }
```

y en el bloque de arrays, tratar como candidato a recursión también un
`elem_ty` `float`/`decimal`/`bigint` (hoy solo angostos): la condición
`narrow_elem` pasa a

```rust
        let literal_elem = elem_ty.is_granular_int()
            || matches!(
                table.get(elem_ty.0),
                TypeKind::Intrinsic(
                    varn_core::TypeTag::F32
                        | varn_core::TypeTag::Float
                        | varn_core::TypeTag::Decimal
                        | varn_core::TypeTag::BigInt
                )
            )
            || array_element_type(&elem_ty, table, interner).is_some();
```

(renombrar la variable `narrow_elem` → `literal_elem` en su alcance).

- [ ] **Step 6: Operadores con literal adoptado**

`check/mod.rs`, brazo `ExprKind::Binary` (líneas ~411-445). Tras calcular
`l_base`/`r_base`, sustituir `same_numeric` por el veredicto de la regla
central con adopción:

```rust
                    let arena = &bind.arena;
                    let l_eff = crate::checker::compat::literal_operand_class(arena, left, &r_base, &self.ty_table)
                        .unwrap_or(l_base);
                    let r_eff = crate::checker::compat::literal_operand_class(arena, right, &l_base, &self.ty_table)
                        .unwrap_or(r_base);
                    let same_numeric = crate::binder::type_inference::numeric_binary_type(op, &l_eff, &r_eff, &self.ty_table).is_some();
```

(Usar la ruta de módulo real de `numeric_binary_type`; hoy es
`pub(crate)` en `binder/type_inference.rs`.) Añadir `BinaryOp::Eq |
BinaryOp::NotEq` a la validación: si **ambos** lados son numéricos,
exigir `same_numeric`:

```rust
                        BinaryOp::Eq | BinaryOp::NotEq => {
                            !(is_numeric(&l_base, self) && is_numeric(&r_base, self)) || same_numeric
                        }
```

(Colocar el brazo antes de `_ => true`.) Esto rechaza `i == 2.5` con `i: int`
y acepta `f == 2` con `f: float`.

`binder/type_inference.rs::infer_binary`: en los brazos aritméticos, aplicar
la misma adopción antes de `numeric_binary_type`:

```rust
            let l = infer_expr_type(left, arena, ctx, table);
            let r = infer_expr_type(right, arena, ctx, table);
            let l = crate::checker::compat::literal_operand_class(arena, left, &r, table).unwrap_or(l);
            let r = crate::checker::compat::literal_operand_class(arena, right, &l, table).unwrap_or(r);
            numeric_binary_type(*op, &l, &r, table).unwrap_or(Type::Dynamic)
```

(En el brazo `Add`, después del chequeo de `Str`.) Si esto crea una
dependencia `binder → checker::compat` nueva, mover `numeric_literal.rs` a
`crates/varn-checker/src/types/numeric_literal.rs` (capa `types`, que ambos ya
consumen) y reexportar desde allí — la dependencia debe ir hacia abajo
(AGENTS.md §1).

`checker/refine.rs::numeric_result`: el doc dice "`int / int` is a float":
corregir a "division keeps the operand domain".

Del lado del backend no hay nada que añadir: `from_tir/build.rs:709-716`
ya hace `coerce(l, ty)` / `coerce(r, ty)` hacia el tipo resultado, y `coerce`
emite `Convert{IntToFloat}` (A.6) que const-fold pliega a `ConstFloat`.

- [ ] **Step 7: Migrar `std/` y tests**

Run: `cargo build --release -p varn-cli`
Expected: errores VN3001/VN3010 en `std/` donde había mezcla `int`/`float`
entre **variables**. Corregir cada uno escribiendo la conversión:
`x as float` (o `x.toFloat()` si ya existe en ese sitio). Nunca cambiar un
tipo declarado para esquivar el error sin entender el dominio.

Después: GATE-SUITE; corregir cada test que falle del mismo modo, y
GATE-ERRORS (los 4 fixtures nuevos deben pasar ahora).

- [ ] **Step 8: Commit**

```bash
git add crates/varn-core/src/numeric.rs crates/varn-checker std tests
git commit -m "feat(lang)!: no implicit lossy numeric conversions; exact int literals adopt context (spec §5, §9)"
```

---

### Tarea A.8: Borrar los anchos numéricos públicos

Tres commits, cada uno compilando: (a) la superficie deja de producirlos,
(b) el backend deja de representarlos, (c) desaparecen de `TypeTag` y del
checker.

#### A.8a — La superficie no reconoce `i8..u64/f32`

**Files:**
- Modify: `crates/varn-core/src/intrinsics.rs:85-92` (borrar los 8 nombres de `from_str`)
- Modify: `crates/varn-core/src/type_tag.rs:121-134` (borrar los 8 nombres de `TypeTag::from_str`)
- Delete: `tests/107-narrow-numeric-types.vn`, `tests/108-granular-numerics.vn`, `tests/116-narrow-array-literals.vn`
- Delete: `tests/errors/implicit-narrowing-rejected.vn`, `tests/errors/narrow-array-f32-overflow.vn`, `tests/errors/narrow-array-literal-overflow.vn`, `tests/errors/narrow-array-object-missing-prop.vn`
- Modify: `tests/main.vn` (quitar sus imports)
- Create: `tests/errors/narrow-type-names-rejected.vn`

- [ ] **Step 1: Fixture negativo**

```varn
// expect: error[VN3002]
let b: i8 = 1
```

Run: GATE-ERRORS → FAIL (hoy `i8` existe).

Antes de borrar `narrow-array-object-missing-prop.vn`, leerlo: si lo que
prueba (propiedad faltante en objeto dentro de array literal) no es sobre
anchos, reescribirlo con `int` en vez de borrarlo.

- [ ] **Step 2: Borrar nombres y tests**

Borrar las líneas `"i8" => ...` … `"f32" => ...` de ambas funciones
`from_str`, los tres tests y los fixtures listados, y sus imports en
`tests/main.vn`.

Run: `rg -nw "i8|i16|i32|u8|u16|u32|u64|f32" std tests --glob "*.vn"`
Expected: solo el fixture nuevo.

Run: `cargo build --release -p varn-cli`; GATE-SUITE; GATE-ERRORS → PASS.
(Si `VN3002` no es el código que sale para un tipo desconocido, usar el que
imprima `vn check` y corregir la cabecera.)

- [ ] **Step 3: Commit**

```bash
git add crates/varn-core/src/intrinsics.rs crates/varn-core/src/type_tag.rs tests
git commit -m "feat(lang)!: remove i8..u64/f32 from the surface language (spec §11)"
```

#### A.8b — El backend deja de representarlos

**Files (todos los sitios de la tabla, verificados con `rg` el 2026-09-23):**
- `crates/varn-tir/src/ty.rs:70-89` — borrar `Int8 Int16 Int32 UInt8 UInt16 UInt32 Float32` y su comentario.
- `crates/varn-checker/src/emit/ty.rs`, `emit/body.rs` — brazos que producen esas variantes.
- `crates/varn-compiler/src/from_tir/{ty.rs,build.rs}` — `narrow_tag_of`, ramas `NarrowRangeCheck` (líneas ~745-790, ~835-836 y el `if let Some(tag) = narrow_tag_of(...)` que A.6 dejó en `Cast`), `narrow_elem` en `BuildArray`.
- `crates/varn-compiler/src/ssa/{ir.rs,emit/values.rs,portable.rs,dump.rs,uses.rs}`, `passes/dce.rs` — `InstKind::NarrowRangeCheck`, `narrow_elem`.
- `crates/varn-types/src/ssa.rs:79-126, ~388` — `SsaOp::NarrowRangeCheck`.
- `crates/varn-core/src/opcode.rs` — `CheckNarrowRange`; `crates/varn-types/src/bytecode.rs`, `crates/varn-debug/src/bytecode.rs`.
- Delete: `crates/varn-vm/src/exec/narrow_range.rs`; `crates/varn-vm/src/exec/{mod.rs,dispatch/mod.rs}`.
- `crates/varn-jit/src/clif/from_ssa/scalar.rs:117`.
- `crates/varn-lsp/src/features/compiler_inspect.rs:478`.
- `crates/varn-types/src/vm_value.rs:474-520` — `ArrayRepr::{I8,I16,I32,U8,U16,U32,F32}` y cada `match` sobre `ArrayRepr` en `crates/varn-jit/src/clif/{arrays.rs,emit.rs,vars.rs}`, `crates/varn-jit/src/lib.rs`, `crates/varn-vm/src/exec/{ctx_csv.rs,ctx_json.rs}`, `crates/varn-vm/src/heap/{aggregates.rs,jit.rs}`, `crates/varn-vm/src/jit/helpers.rs`, `crates/varn-checker/src/checker/compat/mod.rs`.
- `crates/varn-types/src/value/object.rs`, `crates/varn-jit/src/clif/{fields.rs,alloc/calls.rs}` — lectura/escritura de campos angostos.

- [ ] **Step 1: Borrar productores → consumidores**

Orden: `varn-tir` (`BackendTy`), luego seguir los errores de
`cargo check --workspace --all-targets` crate por crate. En cada `match` roto,
borrar el brazo (nunca `_ =>`). Tras borrar `ArrayRepr::I8..F32`, los
discriminantes que quedan son `Boxed = 0, I64 = 1, F64 = 2`; el JIT lee el
byte en offset 0 — revisar `ArrayRepr::discriminant` y las constantes del JIT
que comparan contra 3..9 y borrarlas.

Run: `rg -n "NarrowRange|narrow_elem|narrow_tag_of|Int8|Float32|UInt8|ArrayRepr::(I8|I16|I32|U8|U16|U32|F32)" crates`
Expected: sin resultados.

- [ ] **Step 2: Verde** — GATE-CHECK, GATE-SUITE, GATE-ERRORS.

- [ ] **Step 3: Commit**

```bash
git add -A crates
git commit -m "refactor(backend): delete narrow numeric representations (BackendTy, NarrowRangeCheck, ArrayRepr)"
```

#### A.8c — `TypeTag` y el checker

**Files:**
- `crates/varn-core/src/type_tag.rs` — borrar variantes `I8 I16 I32 U8 U16 U32 U64 F32` (líneas 36-43), sus `name()`, `is_primitive()`, `field_repr()`; `from_u8` usa `TypedArray` como último discriminante: sigue válido.
- `crates/varn-core/src/intrinsics.rs:43-50` — constantes `IntrinsicType::I8..F32`.
- `crates/varn-checker/src/types/interned.rs:78-87, 95-120, seed` — `CheckerTyId::I8..F32`; renumerar `THIS` a `12`.
- `crates/varn-checker/src/types/type_impl.rs:202-215` — `is_int` vuelve a ser `self.0 == CheckerTyId::INT`; borrar `is_granular_int` y sus usos.
- `crates/varn-checker/src/checker/compat/mod.rs` — brazos angostos de `simple_types_compatible`, `literal_fits_type` (queda: `Int|Float|Decimal|BigInt|Dynamic => true`, o se borra si `int_literal_adopts` la cubre — preferir borrarla y usar `int_literal_adopts`), `float_literal_fits_f32`, tests `i8_arr/u8_arr` (líneas ~1120-1220).
- `crates/varn-checker/src/binder/type_inference.rs:402-414`, `checker/refine.rs:170-180` — el `operand` queda `Int => Int, Float => Float, Decimal => Decimal`.
- `crates/varn-core/src/intrinsics.rs` o donde viva `numeric_intrinsic` (`crates/varn-core/src/intrinsics.rs:85`) — ya hecho en A.8a.
- `crates/varn-compiler/src/ssa/ir.rs`, `crates/varn-types/src/{ssa.rs,value/object.rs}`, `crates/varn-vm/src/exec/dispatch/mod.rs`, `crates/varn-jit/src/clif/{fields.rs,alloc/calls.rs}` — referencias restantes a `TypeTag::I8..`.

- [ ] **Step 1: Borrar y seguir al compilador**

Run tras cada crate: `cargo check -p <crate> --all-targets`.

Run final: `rg -n "TypeTag::(I8|I16|I32|U8|U16|U32|U64|F32)\b|CheckerTyId::(I8|I16|I32|U8|U16|U32|U64|F32)\b|is_granular_int" crates`
Expected: sin resultados.

- [ ] **Step 2: Doc del contrato**

`docs/TIR_CONTRATO_TIPADO.md`: borrar la sección de anchos angostos y
escribir: "Los escalares del backend son `Int` (i64), `Float` (f64), `Bool`,
`Char`. Una representación más estrecha es decisión del optimizador (spec
§12), no un `BackendTy`."

- [ ] **Step 3: Verde** — GATE-CHECK, `cargo test -p varn-checker`, GATE-SUITE, GATE-ERRORS, y regenerar goldens de debug si cambian:
`$env:UPDATE_DEBUG_GOLDENS="1"; cargo test -p varn-cli --test debug_golden; Remove-Item Env:UPDATE_DEBUG_GOLDENS` — revisar el diff de goldens: solo deben cambiar ids/discriminantes.

- [ ] **Step 4: Commit**

```bash
git add -A crates docs/TIR_CONTRATO_TIPADO.md
git commit -m "refactor(types): delete narrow numeric TypeTags and checker ids"
```

---

### Tarea A.9: API de enteros (§3, §10, §61) y errores nativos tipados

Los métodos nativos hoy no pueden lanzar (`varn_contract.rs:577-600`: solo
las `function` devuelven `Result<_, String>`) y el `String` pierde la clase.
`abs`/`negate` envuelven (`int.rs:41,43`), `pow` usa `wrapping_pow`
(`int.rs:62-64`), `int.parse` devuelve `0` en fallo (`int.rs:77-84`).

**Files:**
- Modify: `crates/varn-types/src/native_ctx.rs:295` (`NativeFnResult`)
- Create: `crates/varn-types/src/native_error.rs`; Modify: `crates/varn-types/src/lib.rs`
- Modify: `crates/varn-vm/src/error.rs` (`From<NativeError>`), cada `map_err(RuntimeError::new)` sobre resultados nativos (`dispatch/reg_ops/method_calls.rs:587`, `frame_ctrl.rs:236,247,254`, `modules.rs:7`)
- Modify: `crates/varn-op-macros/src/varn_contract.rs` (wrappers → `NativeError`; métodos `@fallible`)
- Modify: `crates/varn-builtins/**` — cada `Err(String)` explícito en funciones `NativeFnResult` (~93 sitios: `rg -n 'Err\(format!|Err\("' crates/varn-builtins crates/varn-vm crates/varn-types`)
- Modify: `crates/varn-builtins/src/modules/primitives/int/{int.vn,int.rs}`
- Create: `tests/123-int-api.vn`; Modify: `tests/main.vn`

**Interfaces:**
- Produces:
  - `varn_types::NativeError { pub kind: varn_core::RuntimeErrorKind, pub message: String }` con `From<String>`, `From<&str>`, `NativeError::integer_overflow(impl Into<String>)`, `NativeError::division_by_zero(impl Into<String>)`.
  - `pub type NativeFnResult = Result<VmValue, NativeError>;`
  - Contrato `.vn`: un método decorado `@fallible` genera trait `-> Result<T, NativeError>` y no es elegible para fast path.
  - Métodos `int`: `abs(): int` `@fallible`, `negate(): int` `@fallible`, `pow(e: int): int` `@fallible`, `wrappingAdd/Sub/Mul(o: int): int`, `saturatingAdd/Sub/Mul(o: int): int`, `checkedAdd/Sub/Mul(o: int): int?`, `div/floorDiv/ceilDiv/rem/mod(o: int): int` `@fallible`; `static parse(s: str): int` lanza en fallo (usar `static tryParse(s: str): int?` para el caso opcional).

- [ ] **Step 1: Test `.vn` que falla**

`tests/123-int-api.vn`:

```varn
const MAX: int = 9223372036854775807
const MIN: int = 0 - 9223372036854775807 - 1

assert("wrappingAdd wraps", MAX.wrappingAdd(1) === MIN)
assert("wrappingSub wraps", MIN.wrappingSub(1) === MAX)
assert("wrappingMul wraps", MAX.wrappingMul(2) === 0 - 2)
assert("saturatingAdd clamps", MAX.saturatingAdd(1) === MAX)
assert("saturatingSub clamps", MIN.saturatingSub(1) === MIN)
assert("saturatingMul clamps", MIN.saturatingMul(2) === MIN)
assert("checkedAdd none on overflow", MAX.checkedAdd(1) === null)
assert("checkedAdd some", (40).checkedAdd(2) === 42)

assert("div truncates", (0 - 7).div(2) === 0 - 3)
assert("floorDiv floors", (0 - 7).floorDiv(2) === 0 - 4)
assert("ceilDiv ceils", (7).ceilDiv(2) === 4)
assert("rem follows dividend", (0 - 7).rem(2) === 0 - 1)
assert("mod is euclidean", (0 - 7).mod(2) === 1)

function kind(f: () => int): str {
    try { f(); return "none" } catch (e) {
        if (e instanceof IntegerOverflow) { return "ovf" }
        if (e instanceof DivisionByZero) { return "zero" }
        return "other"
    }
}
assert("abs(MIN) raises", kind(() => MIN.abs()) === "ovf")
assert("negate(MIN) raises", kind(() => MIN.negate()) === "ovf")
assert("pow overflow raises", kind(() => (10).pow(30)) === "ovf")
assert("floorDiv by zero raises", kind(() => (1).floorDiv(0)) === "zero")
assert("parse failure raises", kind(() => int.parse("abc")) === "other")
assert("tryParse failure is null", int.tryParse("abc") === null)

print("[PASSED] 123. Int API")
```

Run: `.\target\release\vn.exe check tests/123-int-api.vn`
Expected: FAIL `property 'wrappingAdd' does not exist on type 'int'`.

- [ ] **Step 2: `NativeError`**

`crates/varn-types/src/native_error.rs`:

```rust
use varn_core::RuntimeErrorKind;

/// Error de una nativa: lleva su clase de plataforma hasta el `catch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
}

impl NativeError {
    pub fn integer_overflow(message: impl Into<String>) -> Self {
        Self { kind: RuntimeErrorKind::IntegerOverflow, message: message.into() }
    }

    pub fn division_by_zero(message: impl Into<String>) -> Self {
        Self { kind: RuntimeErrorKind::DivisionByZero, message: message.into() }
    }
}

impl From<String> for NativeError {
    fn from(message: String) -> Self {
        Self { kind: RuntimeErrorKind::Error, message }
    }
}

impl From<&str> for NativeError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
```

`lib.rs`: `mod native_error; pub use native_error::NativeError;`.
`native_ctx.rs:295`: `pub type NativeFnResult = Result<VmValue, NativeError>;`.

`crates/varn-vm/src/error.rs`:

```rust
impl From<varn_types::NativeError> for RuntimeError {
    fn from(e: varn_types::NativeError) -> Self {
        Self::of_kind(e.kind, e.message)
    }
}
```

y cada `.map_err(RuntimeError::new)` sobre un `NativeFnResult` →
`.map_err(RuntimeError::from)`.

- [ ] **Step 3: Macro**

En `crates/varn-op-macros/src/varn_contract.rs`:
1. Todas las firmas generadas `::core::result::Result<::varn_types::VmValue, String>`
   (líneas ~616 y ~797) → `::core::result::Result<::varn_types::VmValue, ::varn_types::NativeError>`,
   y `quote!(::core::result::Result<#inner, String>)` (~582) →
   `quote!(::core::result::Result<#inner, ::varn_types::NativeError>)`.
2. Leer los decoradores del miembro del contrato (el AST de clase ya los trae:
   `varn-core/src/ast/decl.rs:138,146` campo `decorators`). Añadir al struct
   que describe cada método del contrato un `fallible: bool` = tiene un
   decorador de nombre `fallible`.
3. Para `Kind::Method` con `fallible`: `trait_ret` = `Result<rty, NativeError>`
   (igual que `Kind::Function`) y `ret_encode` usa la rama `is_fn`
   (`#call?`). `is_fast_eligible(m)` devuelve `false` si `m.fallible`.

Run: `cargo check --workspace --exclude varn-lsp`
Expected: errores `expected NativeError, found String` en nativas con
`Err(format!(..))`/`Err(s)`. Corregir cada uno con `.into()`:
`Err(format!(...).into())`. Es mecánico; no cambiar mensajes.

- [ ] **Step 4: Contrato y nativa de `int`**

`int.vn` (reemplaza los métodos correspondientes; conservar el resto):

```varn
export declare class int {
    static MAX_VALUE: int;
    static MIN_VALUE: int;
    static parse(s: str): int;
    static tryParse(s: str): int?;
    static isInteger(val: dynamic): bool;
    static isSafeInteger(val: int): bool;

    toString(): str;
    valueOf(): int;
    toLocaleString(): str;
    toFixed(digits?: int): str;
    @fallible abs(): int;
    sign(): int;
    @fallible negate(): int;
    bitwiseNot(): int;
    min(other: int): int;
    max(other: int): int;
    clamp(lo: int, hi: int): int;
    toHex(): str;
    toBinary(): str;
    toOctal(): str;
    toFloat(): float;
    @fallible pow(exponent: int): int;
    isEven(): bool;
    isOdd(): bool;

    wrappingAdd(other: int): int;
    wrappingSub(other: int): int;
    wrappingMul(other: int): int;
    saturatingAdd(other: int): int;
    saturatingSub(other: int): int;
    saturatingMul(other: int): int;
    checkedAdd(other: int): int?;
    checkedSub(other: int): int?;
    checkedMul(other: int): int?;
    @fallible div(other: int): int;
    @fallible floorDiv(other: int): int;
    @fallible ceilDiv(other: int): int;
    @fallible rem(other: int): int;
    @fallible mod(other: int): int;
}
```

Si el parser de contratos no acepta decoradores en `declare class`, añadir el
soporte en el parser (miembros de clase declarada) en este mismo paso, con un
test de parser.

En `varn_core::numeric` añadir las funciones puras (una fuente para VM, nativa
y futuras optimizaciones), con tests:

```rust
pub fn floor_div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    let q = div_int(a, b)?;
    Ok(if (a % b != 0) && ((a < 0) != (b < 0)) { q - 1 } else { q })
}

pub fn ceil_div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    let q = div_int(a, b)?;
    Ok(if (a % b != 0) && ((a < 0) == (b < 0)) { q + 1 } else { q })
}

/// Euclidean modulo: always in `0..|b|`.
pub fn mod_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a.wrapping_rem_euclid(b))
}
```

```rust
    #[test]
    fn floor_ceil_mod() {
        assert_eq!(floor_div_int(-7, 2), Ok(-4));
        assert_eq!(ceil_div_int(7, 2), Ok(4));
        assert_eq!(ceil_div_int(-7, 2), Ok(-3));
        assert_eq!(mod_int(-7, 2), Ok(1));
        assert_eq!(mod_int(i64::MIN, -1), Ok(0));
        assert_eq!(floor_div_int(i64::MIN, -1), Err(IntDivFault::Overflow));
    }
```

(`floor_div_int(MIN, -1)`: `div_int` ya devuelve `Overflow` antes del ajuste.
En `floor/ceil`, `a % b` con `b == -1` y `a == MIN` no se evalúa porque
`div_int` falló primero.)

Nativa, en `int.rs` dentro de `impl Int` (mapear fallos con un helper):

```rust
        fn abs(_ctx: &mut dyn NativeCtx, this: i64) -> Result<i64, NativeError> {
            this.checked_abs().ok_or_else(|| overflow_error("abs", this))
        }
        fn negate(_ctx: &mut dyn NativeCtx, this: i64) -> Result<i64, NativeError> {
            varn_core::neg_int(this).ok_or_else(|| overflow_error("negate", this))
        }
        fn pow(_ctx: &mut dyn NativeCtx, this: i64, exponent: i64) -> Result<i64, NativeError> {
            let e = u32::try_from(exponent)
                .map_err(|_| NativeError::from(format!("pow: exponent {exponent} must be in 0..=u32::MAX")))?;
            varn_core::pow_int(this, e).ok_or_else(|| overflow_error("pow", this))
        }
        fn wrappingAdd(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.wrapping_add(other) }
        fn wrappingSub(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.wrapping_sub(other) }
        fn wrappingMul(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.wrapping_mul(other) }
        fn saturatingAdd(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.saturating_add(other) }
        fn saturatingSub(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.saturating_sub(other) }
        fn saturatingMul(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.saturating_mul(other) }
        fn checkedAdd(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Option<i64> { varn_core::add_int(this, other) }
        fn checkedSub(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Option<i64> { varn_core::sub_int(this, other) }
        fn checkedMul(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Option<i64> { varn_core::mul_int(this, other) }
        fn div(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::div_int(this, other).map_err(|f| div_fault(f, "div", this, other))
        }
        fn floorDiv(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::floor_div_int(this, other).map_err(|f| div_fault(f, "floorDiv", this, other))
        }
        fn ceilDiv(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::ceil_div_int(this, other).map_err(|f| div_fault(f, "ceilDiv", this, other))
        }
        fn rem(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::rem_int(this, other).map_err(|f| div_fault(f, "rem", this, other))
        }
        fn mod(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::mod_int(this, other).map_err(|f| div_fault(f, "mod", this, other))
        }
```

Fuera del macro:

```rust
fn overflow_error(op: &str, v: i64) -> NativeError {
    NativeError::integer_overflow(format!("integer overflow: {op}({v}) is outside int ({INT_MIN}..={INT_MAX})"))
}

fn div_fault(fault: varn_core::IntDivFault, op: &str, a: i64, b: i64) -> NativeError {
    match fault {
        varn_core::IntDivFault::DivisionByZero => NativeError::division_by_zero(format!("{op}: division by zero")),
        varn_core::IntDivFault::Overflow => {
            NativeError::integer_overflow(format!("integer overflow: {a}.{op}({b}) is outside int"))
        }
    }
}
```

`mod` es palabra reservada en Rust: si el macro genera un `fn mod`, usar
`r#mod` como identificador Rust y conservar `"mod"` como nombre Varn (revisar
cómo el macro deriva el nombre del método; si toma el identificador Rust,
añadir soporte para `r#` quitando el prefijo al registrar el nombre).

`int_parse`: `s.trim().parse::<i64>().map(VmValue::from_int).map_err(|e| format!("int.parse: {e}").into())`;
nueva `int_try_parse` devuelve `null` en fallo. Registrar `tryParse` donde se
registra `parse` (`rg -n "int_parse" crates/varn-builtins`).

- [ ] **Step 5: Verde**

GATE-CHECK; `cargo test -p varn-core -p varn-op-macros`;
`.\target\release\vn.exe run tests/123-int-api.vn` en ambos tiers; GATE-SUITE;
GATE-ERRORS.

- [ ] **Step 6: Commits (Ley 9: dos cambios)**

```bash
git add crates/varn-types crates/varn-vm crates/varn-op-macros crates/varn-builtins
git commit -m "feat(runtime): native errors carry their platform error class"
```

(El primer commit incluye solo `NativeError`, el macro y la migración
`.into()`; hacer `git add -p` excluyendo `primitives/int/` y
`varn-core/src/numeric.rs`.)

```bash
git add crates/varn-core/src/numeric.rs crates/varn-core/src/lib.rs crates/varn-builtins/src/modules/primitives/int tests/123-int-api.vn tests/main.vn
git commit -m "feat(std): int wrapping/saturating/checked/division API; checked abs/negate/pow"
```

---

### Tarea A.10: Documentación y cierre de fase

**Files:**
- Modify: `docs/lang/types.md` (sección de tipos numéricos)
- Modify: `docs/lang/expressions.md` (operadores aritméticos y `as`)
- Modify: `docs/lang/runtime_behavior.md` (errores `IntegerOverflow`/`DivisionByZero`, IEEE)
- Modify: `docs/lang/standard_library.md` (API de `int`)
- Modify: `docs/plans/2026-09-23-new-spec-roadmap.md` §2.1 (marcar ✅ lo cerrado)

- [ ] **Step 1: Escribir las secciones**

`docs/lang/types.md`, sección numérica (reemplaza la existente):

```markdown
## Tipos numéricos

Varn tiene exactamente cuatro: `int`, `float`, `bigint`, `decimal`.

| Tipo | Semántica |
|---|---|
| `int` | entero con signo de 64 bits; `+ - * / % **` y `-x` lanzan `IntegerOverflow` al salir de rango |
| `float` | IEEE 754 binary64 (`-0`, `NaN`, `±Infinity`); `/ 0.0` da `±Infinity` o `NaN` |
| `bigint` | entero de precisión arbitraria, literal `123n` |
| `decimal` | decimal, literal `19.99d` |

Conversiones implícitas: solo `int → bigint` e `int → decimal`. Un literal
entero exacto adopta el tipo numérico del contexto (`let x: float = 5`,
`f * 2`). Todo lo demás se escribe con `as`:

| `as` | Comportamiento |
|---|---|
| `int as float` | redondeo al float más cercano |
| `float as int` | trunca hacia cero; `NaN`, `±Infinity` o fuera de rango lanzan `IntegerOverflow` |
| `bigint as int`, `decimal as int` | exacto/trunca; fuera de rango lanza `IntegerOverflow` |

División: `int / int` es `int` truncado hacia cero; `x / 0` y `x % 0` lanzan
`DivisionByZero`; `int.MIN_VALUE / -1` lanza `IntegerOverflow`;
`int.MIN_VALUE % -1` es `0`. `%` toma el signo del dividendo; `x.mod(y)` es
euclídeo.
```

Las otras tres páginas: misma información en su sección correspondiente, sin
duplicar tablas (enlazar a `types.md#tipos-numéricos`).

- [ ] **Step 2: Cierre**

Run: GATE-PHASE (`.\scripts\verify.ps1 -Fast`)
Expected: 4 cuadrantes `ALL TESTS PASSED`, fmt y clippy limpios.

Run: `cargo xtask compare` y comparar contra la línea base anotada en A.3
Step 5. Registrar en el roadmap cualquier regresión > 10 %.

- [ ] **Step 3: Commit**

```bash
git add docs
git commit -m "docs: numeric core semantics per NEW_SPEC (Fase A closed)"
```

---

## Auto-revisión (hecha al escribir)

- **Cobertura del spec (Fase A):** §2 (A.8), §2.1 checked + nombre de error
  (A.1, A.2, A.3), §3 (A.9), §4 (A.5), §5 (A.6, A.7), §7 conversión con
  chequeo (A.6), §9 tabla (A.7), §10 (A.2, A.4, A.9), §11–12 (A.8), §41
  parcial (A.1), §61 (A.9), §104 anchos (A.8). §6/§8 precisión arbitraria →
  Fase B (roadmap), declarado fuera de este plan.
- **Consistencia de nombres:** `RuntimeErrorKind` (A.1) usado en A.9;
  `div_int`/`rem_int`/`IntDivFault` (A.2) usados en A.4 y A.9;
  `arith::int_div_fault` (A.2) usado en A.4; `NumConv`/`float_to_int` (A.6)
  usados en A.7 (vía `coerce`); `literal_operand_class`/`int_literal_adopts`
  (A.7).
- **Puntos donde el ejecutor debe confirmar un nombre real antes de escribir**
  (se indicó en cada paso): campos de `InstKind::Binary/Unary` (A.3), API de
  heap para bigint/decimal (A.6), función `BackendTy → HirType` de `from_tir`
  (A.6), identificador del macro para `mod` (A.9). Son lecturas de 1 minuto;
  el comportamiento exigido está fijado por los tests.
