# Async P2a: preparación del IR para el pase de máquinas de estados — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dejar el IR listo para el pase de máquinas de estados y medir la forma real de las suspensiones del corpus, para que las decisiones de mecánica del pase se tomen con datos en vez de por suposición.

**Architecture:** Cuatro cambios independientes en `varn-compiler`, ninguno de los cuales altera el bytecode generado. Se sustituye el oráculo canonicalizado por comparación byte a byte (posible desde `4dc7916`/`0b6b4f9`); se reformula `live_across` para que no dependa del orden del vector de bloques; se expone `is_async`/`is_generator` a la capa SSA; y se añade un análisis de sólo lectura que localiza los puntos de suspensión con sus conjuntos vivos, visible por `vn debug -p suspend`.

**Tech Stack:** Rust, `varn-compiler` (HIR→SSA→pases→emisión), `varn-debug`. Verificación con el binario `vn`, nunca con `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-16-modelo-asincrono-design.md` (§3.1 el pase de transformación; §9 riesgos abiertos 2 y 3)

## Global Constraints

- **PROHIBIDO `cargo test`.** Es irrelevante como señal de correctitud en este repo.
- **Matriz de 4 obligatoria** al cerrar cada tarea: `run` × `VARN_NO_JIT` × std de árbol/`@embedded`. Purgar con `vn cache clean` al cambiar de procedencia. Script: `verify.sh`.
- **Ninguna tarea de este plan puede cambiar el bytecode generado.** Todas son análisis nuevo o refactor. El oráculo es byte a byte y es duro.
- `cargo build --release` con **cero warnings nuevos** (`unused_crate_dependencies = "warn"`).
- **No añadir dependencias.** `varn-regalloc` no depende de `rustc-hash` y así se queda.
- Directorio temporal (el shell del harness NO conserva exports entre llamadas; exportar en cada llamada que lo use):
  ```bash
  export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
  ```
- Base: `0b6b4f9`.

---

## File Structure

| Fichero | Responsabilidad tras el cambio |
|---|---|
| `crates/varn-compiler/src/ssa/liveness.rs` | Añade `live_after`, que responde desde `live_out` + transferencia hacia atrás dentro del bloque. Sin dependencia del orden del vector de bloques. `live_across` se retira |
| `crates/varn-compiler/src/ssa/ir.rs` | `SsaFunc` gana `is_async` e `is_generator` |
| `crates/varn-compiler/src/ssa/build/mod.rs` | Los rellena desde `HirFunction` |
| `crates/varn-compiler/src/ssa/suspend.rs` (**nuevo**) | Localiza puntos de suspensión y su conjunto vivo. Sólo lectura, no transforma |
| `crates/varn-compiler/src/ssa/mod.rs` | Declara `suspend` |
| `crates/varn-debug/src/flags.rs` | Registra la fase `suspend` |
| `crates/varn-debug/src/suspend.rs` (**nuevo**) | Presenta el análisis |

Separar `suspend.rs` del pase que vendrá después es deliberado: el análisis es útil por sí mismo, es verificable sin transformar nada, y el pase de P2b lo consumirá en vez de recalcularlo.

---

## Task 1: Sustituir el oráculo canonicalizado por comparación byte a byte

El corpus con registros canonicalizados fue un apaño para el no determinismo que `4dc7916` y `0b6b4f9` ya arreglaron. Ahora se puede comparar byte a byte, que es mucho más estricto: detecta permutaciones de registros, no sólo cambios de forma.

**Files:**
- Create: `$SCRATCH/baseline/exact-main.txt`, `$SCRATCH/baseline/exact-corpus.txt` (fuera del repo)
- Create: `$SCRATCH/exact.sh`

**Interfaces:**
- Produces: `exact.sh`, oráculo byte a byte consumido por las tareas 2, 3 y 4.

- [ ] **Step 1: Confirmar el punto de partida**

```bash
cd /c/Users/x/dev/varn/varn-lang
git log --oneline -1     # esperado: 0b6b4f9
git status --porcelain   # esperado: vacío
cargo build --release 2>&1 | grep -E "^(error|warning)"; echo "(vacio = limpio)"
```

- [ ] **Step 2: Crear el oráculo exacto**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cat > "$SCRATCH/exact.sh" <<'SCRIPT'
#!/usr/bin/env bash
# Bytecode identico BYTE A BYTE contra la referencia, sobre tests/main.vn y el
# corpus. Sin canonicalizar registros: desde 4dc7916 y 0b6b4f9 la compilacion
# es reproducible, asi que cualquier diferencia es una regresion real.
set -u
SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang || exit 1
mkdir -p "$SCRATCH/current"
strip() { sed -e 's/\x1b\[[0-9;]*m//g'; }

./target/release/vn.exe debug -p bytecode ./tests/main.vn 2>&1 | strip > "$SCRATCH/current/exact-main.txt"
: > "$SCRATCH/current/exact-corpus.txt"
for f in "$SCRATCH"/corpus/*.vn; do
  echo "### $(basename "$f")" >> "$SCRATCH/current/exact-corpus.txt"
  ./target/release/vn.exe debug -p bytecode "$f" 2>&1 | strip >> "$SCRATCH/current/exact-corpus.txt"
done

fail=0
for f in exact-main exact-corpus; do
  if diff -q "$SCRATCH/baseline/$f.txt" "$SCRATCH/current/$f.txt" >/dev/null 2>&1; then
    echo "OK   $f byte a byte"
  else
    echo "DIFF $f"
    diff "$SCRATCH/baseline/$f.txt" "$SCRATCH/current/$f.txt" | head -30
    fail=1
  fi
done
exit $fail
SCRIPT
chmod +x "$SCRATCH/exact.sh"
```

- [ ] **Step 3: Capturar la referencia y probar que el oráculo es estable**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
mkdir -p "$SCRATCH/baseline"
strip() { sed -e 's/\x1b\[[0-9;]*m//g'; }
./target/release/vn.exe debug -p bytecode ./tests/main.vn 2>&1 | strip > "$SCRATCH/baseline/exact-main.txt"
: > "$SCRATCH/baseline/exact-corpus.txt"
for f in "$SCRATCH"/corpus/*.vn; do
  echo "### $(basename "$f")" >> "$SCRATCH/baseline/exact-corpus.txt"
  ./target/release/vn.exe debug -p bytecode "$f" 2>&1 | strip >> "$SCRATCH/baseline/exact-corpus.txt"
done
wc -l "$SCRATCH/baseline/exact-main.txt" "$SCRATCH/baseline/exact-corpus.txt"

# tres pasadas seguidas sin tocar nada: deben dar verde las tres
for i in 1 2 3; do "$SCRATCH/exact.sh" | tr '\n' ' '; echo "  <- pasada $i"; done
```

Esperado: `exact-main.txt` con ~41976 líneas, y tres pasadas verdes. **Si alguna da `DIFF` contra el árbol sin modificar, el oráculo no sirve** — hay un tercer foco de no determinismo y hay que cazarlo antes de seguir, igual que se hizo con los dos anteriores.

- [ ] **Step 4: Retirar el oráculo canonicalizado**

Ya no aporta: es estrictamente más débil que el exacto.

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
rm -f "$SCRATCH/bytecode_identity.sh" "$SCRATCH/baseline/corpus.txt"
ls "$SCRATCH"/*.sh
```

Esperado: quedan `verify.sh`, `determinism.sh` y `exact.sh`.

- [ ] **Step 5: Sin commit**

Esta tarea no toca el repo. Confirmar:

```bash
cd /c/Users/x/dev/varn/varn-lang && git status --porcelain
```

Esperado: vacío.

---

## Task 2: Reformular `live_across` para que no dependa del orden de bloques

Deuda registrada del plan anterior. Hoy `live_across(idx)` filtra con `def[v] < idx && end[v] > idx`, y el término `def[v] < idx` sólo es sano si el orden del vector `ssa.blocks` es compatible con dominancia — una suposición que el propio doc del módulo declara no verificada.

La formulación robusta no usa índices lineales: parte de `live_out[b]` y aplica la transferencia hacia atrás dentro del bloque. Depende sólo de `live_out` y de la lista de instrucciones del bloque.

**Files:**
- Modify: `crates/varn-compiler/src/ssa/liveness.rs`

**Interfaces:**
- Consumes: `Liveness::live_out`, `SsaFunc`, `crate::ssa::verify::inst_uses`.
- Produces:
  ```rust
  pub fn live_after(&self, ssa: &SsaFunc, b: usize, i: usize) -> Vec<Value>
  ```
  Valores vivos **después** de ejecutar la instrucción `i` del bloque `b` — que es exactamente el conjunto que debe sobrevivir a una suspensión situada en `i`. La tarea 4 y el pase de P2b lo consumen. `live_across` se retira: nadie la llama.

- [ ] **Step 1: Escribir la prueba antes que el código**

`live_after` no tiene consumidor todavía, así que la prueba es un fichero `.vn` cuyo resultado esperado se conoce a mano. Crear `$SCRATCH/corpus-liveness.vn`:

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cat > "$SCRATCH/corpus-liveness.vn" <<'EOF'
async function f(x: int): int {
    const antes = x * 10;      // cruza el await: debe salir en live_after
    const efimero = x + 1;     // NO cruza: se usa antes del await
    const usado = efimero * 2;
    const r = await g(usado);
    return r + antes;
}
async function g(x: int): int { return x * 2; }
print(await f(3));
EOF
cd /c/Users/x/dev/varn/varn-lang
./target/release/vn.exe debug -p ssa "$SCRATCH/corpus-liveness.vn" 2>&1 | sed -n '/fn f:/,/^fn /p'
```

Anotar del volcado: qué número de valor SSA es `antes`, cuál es `usado`, y en qué posición está el `await`. La expectativa a verificar en el paso 4 es que `live_after` en el punto del `await` **contiene** `antes` y **no contiene** `efimero`.

- [ ] **Step 2: Implementar `live_after`**

En `crates/varn-compiler/src/ssa/liveness.rs`, añadir al `impl Liveness`:

```rust
    /// Valores vivos DESPUÉS de ejecutar la instrucción `i` del bloque `b`.
    ///
    /// Es el conjunto que debe sobrevivir a una suspensión situada en `i`: lo
    /// que el objeto de estado de una máquina de estados tiene que guardar.
    ///
    /// A diferencia de la numeración lineal de puntos, esto **no depende del
    /// orden del vector `ssa.blocks`**: parte de `live_out[b]` y camina hacia
    /// atrás por las instrucciones del bloque aplicando la transferencia
    /// `live = (live - def(inst)) ∪ uses(inst)`. Sólo usa información local al
    /// bloque más su `live_out`, ambos independientes del orden del vector.
    ///
    /// El resultado viene ordenado por índice de valor, para que dos
    /// invocaciones sobre el mismo IR den la misma respuesta en el mismo orden
    /// — un consumidor que derive de aquí el layout de un objeto necesita esa
    /// estabilidad.
    pub fn live_after(&self, ssa: &SsaFunc, b: usize, i: usize) -> Vec<Value> {
        let block = &ssa.blocks[b];
        let mut live: FxHashSet<u32> = self.live_out[b].clone();

        // Recorre hacia atrás hasta pasar la instrucción i+1: el estado que
        // queda es "vivo justo después de i".
        for inst in block.insts[i + 1..].iter().rev() {
            if let Some(d) = inst.dest {
                live.remove(&d.0);
            }
            for u in crate::ssa::verify::inst_uses(&inst.kind) {
                live.insert(u.0);
            }
        }

        let mut out: Vec<Value> = live.into_iter().map(Value).collect();
        out.sort_unstable_by_key(|v| v.0);
        out
    }
```

Nota sobre el terminador: `live_out[b]` ya incluye lo que usa el terminador del bloque (el dataflow lo recoge en `uses[b]`), así que el recorrido hacia atrás empieza en las instrucciones y no hay que tratarlo aparte.

- [ ] **Step 3: Retirar `live_across`**

Borrar el método `live_across` completo y su `assert!`, más el bloque de doc del módulo que hablaba de la suposición sobre el orden de `ssa.blocks` — deja de aplicar, porque `live_after` no la necesita. **Conservar** el resto del doc del módulo: la numeración de puntos, el disparador del clamp y la explicación de `end` como intervalo lineal siguen describiendo a `analyze`.

Confirmar que no queda ningún llamante:

```bash
cd /c/Users/x/dev/varn/varn-lang && grep -rn "live_across" --include=*.rs crates/
```

Esperado: cero líneas.

- [ ] **Step 4: Compilar y verificar**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
cargo build --release 2>&1 | grep -E "^(error|warning)" | head; echo "(vacio = limpio)"
"$SCRATCH/exact.sh"
"$SCRATCH/verify.sh"
```

Esperado: build limpio, `OK` byte a byte en los dos ficheros, cuatro `OK` de la matriz. `live_after` no tiene consumidor, así que el bytecode **no puede** haber cambiado; un `DIFF` aquí significa que se tocó algo que no tocaba.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-compiler/src/ssa/liveness.rs
git commit -m "refactor(compiler): live_after sustituye a live_across

live_across filtraba con def[v] < idx, sano solo si el orden del vector
ssa.blocks es compatible con dominancia — suposicion que el propio doc
del modulo declaraba no verificada, porque compact_cfg sale temprano sin
renumerar cuando no hay bloques inalcanzables.

live_after no usa la numeracion lineal: parte de live_out[b] y camina
hacia atras por el bloque aplicando (live - def) union uses. Solo depende
de informacion local al bloque, que no tiene ese problema.

Devuelve ordenado por indice de valor: un consumidor que derive de aqui
el layout de un objeto de estado necesita esa estabilidad."
```

---

## Task 3: Exponer `is_async` e `is_generator` a la capa SSA

El pase de P2b necesita saber qué forma tiene la función que transforma. Hoy esos flags viven en `HirFunction` y en `FunctionProto`, pero **no en `SsaFunc`**, así que un pase sobre SSA no puede consultarlos.

**Files:**
- Modify: `crates/varn-compiler/src/ssa/ir.rs`
- Modify: `crates/varn-compiler/src/ssa/build/mod.rs`

**Interfaces:**
- Consumes: `HirFunction::is_async`, `HirFunction::is_generator`.
- Produces: `SsaFunc.is_async: bool` y `SsaFunc.is_generator: bool`, poblados en `build_function`. La tarea 4 y el pase de P2b los leen.

- [ ] **Step 1: Confirmar los nombres reales de los flags en HIR**

Antes de escribir nada, verificar cómo se llaman exactamente en `HirFunction`:

```bash
cd /c/Users/x/dev/varn/varn-lang
grep -n "pub struct HirFunction" -A 20 crates/varn-compiler/src/hir/mod.rs
```

Usar los nombres que devuelva ese volcado. Si el flag de generador tiene otro nombre, ajustar el resto de la tarea en consecuencia y anotarlo en el informe.

- [ ] **Step 2: Añadir los campos a `SsaFunc`**

En `crates/varn-compiler/src/ssa/ir.rs`, dentro de `pub struct SsaFunc`, tras `nlocals`:

```rust
    /// Forma de la función, propagada desde HIR. El pase de máquinas de estados
    /// las necesita para decidir qué variantes de `Poll` puede emitir el cuerpo:
    /// `async` produce Pending/Ready, `function*` produce Yielded/Ready, y
    /// `async function*` las tres.
    pub is_async: bool,
    pub is_generator: bool,
```

- [ ] **Step 3: Poblarlos en la construcción**

En `crates/varn-compiler/src/ssa/build/mod.rs`, localizar dónde se construye el `SsaFunc` que devuelve `build_function` y rellenar los dos campos desde el `HirFunction` recibido. **Localizar por símbolo**, no por número de línea.

El compilador señalará el sitio: al añadir campos sin `Default`, la construcción por literal falla hasta rellenarlos. Si `SsaFunc` se construye en más de un sitio, rellenarlos en todos.

- [ ] **Step 4: Comprobar que llegan bien**

Los flags no tienen consumidor todavía, así que hay que verificarlos por observación. Añadir temporalmente al volcado SSA — en `crates/varn-compiler/src/ssa/dump.rs`, donde se imprime la cabecera `fn <nombre>:` — un sufijo con los flags, comprobar, y **dejarlo puesto**: es información útil y permanente en un volcado de depuración.

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
cargo build --release 2>&1 | grep -E "^(error|warning)" | head
cat > "$SCRATCH/formas.vn" <<'EOF'
async function a(): int { return 1; }
function* b(): int { yield 1; }
async function* c(): int { yield 1; }
function d(): int { return 1; }
print(1);
EOF
./target/release/vn.exe debug -p ssa "$SCRATCH/formas.vn" 2>&1 | grep "^fn "
```

Esperado: `a` marcada async y no generador; `b` generador y no async; `c` las dos; `d` ninguna.

- [ ] **Step 5: Verificar y commitear**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
"$SCRATCH/exact.sh"
"$SCRATCH/verify.sh"
```

Esperado: byte a byte `OK` (añadir campos no consumidos no puede mover el bytecode), matriz de 4 verde.

```bash
git add crates/varn-compiler/src/ssa/ir.rs crates/varn-compiler/src/ssa/build/mod.rs crates/varn-compiler/src/ssa/dump.rs
git commit -m "feat(compiler): SsaFunc lleva is_async e is_generator

El pase de maquinas de estados necesita saber que forma tiene la funcion
para decidir que variantes de Poll puede emitir su cuerpo. Los flags
existian en HirFunction y en FunctionProto pero no en SsaFunc, asi que un
pase sobre SSA no podia consultarlos.

El volcado -p ssa los muestra en la cabecera de cada funcion."
```

---

## Task 4: Análisis de puntos de suspensión

El producto de este plan. Análisis de **sólo lectura** que, por función, localiza cada `Await`/`Yield` y calcula el conjunto vivo que lo cruza. No transforma nada.

Su valor inmediato es medir la forma real de las suspensiones del corpus y de `tests/main.vn`: cuántos puntos hay por función, cuántos valores cruzan cada uno, cuántos caen dentro de un `try`, y cuántos están dentro de un bucle. **Esas cuatro cifras son las que deciden la mecánica del pase en P2b**, y hoy se desconocen.

**Files:**
- Create: `crates/varn-compiler/src/ssa/suspend.rs`
- Modify: `crates/varn-compiler/src/ssa/mod.rs`
- Create: `crates/varn-debug/src/suspend.rs`
- Modify: `crates/varn-debug/src/flags.rs`, `crates/varn-debug/src/lib.rs`

**Interfaces:**
- Consumes: `Liveness::live_after` (Task 2), `SsaFunc.is_async`/`is_generator` (Task 3).
- Produces:
  ```rust
  pub struct SuspendPoint {
      pub block: usize,
      pub inst: usize,
      pub kind: SuspendKind,      // Await | Yield
      pub operand: Value,         // lo que se espera / se emite
      pub dest: Option<Value>,    // donde aterriza el resultado al reanudar
      pub live: Vec<Value>,       // lo que debe sobrevivir: campos del estado
      pub in_try: bool,           // ¿hay un Try activo cubriendo este punto?
      pub in_loop: bool,          // ¿el bloque es alcanzable desde sí mismo?
  }
  pub fn analyze(ssa: &SsaFunc) -> Vec<SuspendPoint>;
  ```

- [ ] **Step 1: Escribir la prueba antes que el código**

Fichero con las cuatro situaciones que el análisis debe distinguir:

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cat > "$SCRATCH/suspend-cases.vn" <<'EOF'
async function g(x: int): int { return x * 2; }

async function simple(x: int): int {
    const cruza = x * 10;
    const r = await g(x);
    return r + cruza;
}

async function enBucle(n: int): int {
    let acc = 0;
    for (let i = 0; i < n; i = i + 1) { acc = acc + await g(i); }
    return acc;
}

async function enTry(x: int): int {
    try { return await g(x); } catch (e) { return -1; }
}

function* genera(n: int): int {
    let i = 0;
    while (i < n) { yield i; i = i + 1; }
}

print(1);
EOF
echo "creado"
```

Expectativas, a comprobar en el paso 4:

| Función | Puntos | `in_loop` | `in_try` | `live` no vacío |
|---|---|---|---|---|
| `simple` | 1 Await | no | no | sí (`cruza`) |
| `enBucle` | 1 Await | **sí** | no | sí (`acc`, `i`, `n`) |
| `enTry` | 1 Await | no | **sí** | — |
| `genera` | 1 Yield | **sí** | no | sí (`i`, `n`) |

- [ ] **Step 2: Implementar el análisis**

Crear `crates/varn-compiler/src/ssa/suspend.rs`:

```rust
//! Localiza los puntos de suspensión de una función y qué los cruza.
//!
//! Sólo lectura: no transforma el IR. El pase de máquinas de estados consume
//! este análisis en vez de recalcularlo, para que "qué es un punto de
//! suspensión" y "qué debe guardar el estado" tengan una sola definición.

use super::ir::{InstKind, SsaFunc, Terminator, Value};
use super::liveness::Liveness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendKind {
    Await,
    Yield,
}

#[derive(Debug, Clone)]
pub struct SuspendPoint {
    pub block: usize,
    pub inst: usize,
    pub kind: SuspendKind,
    pub operand: Value,
    pub dest: Option<Value>,
    /// Valores vivos cruzando el punto: los campos que necesitará el objeto de
    /// estado. Ordenado por índice de valor.
    pub live: Vec<Value>,
    pub in_try: bool,
    pub in_loop: bool,
}

pub fn analyze(ssa: &SsaFunc) -> Vec<SuspendPoint> {
    let lv = Liveness::analyze(ssa);
    let in_loop = compute_in_loop(ssa);
    let mut out = Vec::new();

    for (b, block) in ssa.blocks.iter().enumerate() {
        // Un `Try` abre cobertura para el resto del bloque; basta con contar
        // los que se han visto antes de la instrucción actual.
        let mut try_depth = 0usize;
        for (i, inst) in block.insts.iter().enumerate() {
            let kind = match &inst.kind {
                InstKind::Await { .. } => Some(SuspendKind::Await),
                InstKind::Yield { .. } => Some(SuspendKind::Yield),
                _ => None,
            };
            if let Some(kind) = kind {
                let operand = match &inst.kind {
                    InstKind::Await { operand } | InstKind::Yield { operand } => *operand,
                    _ => unreachable!("kind ya filtró a Await/Yield"),
                };
                out.push(SuspendPoint {
                    block: b,
                    inst: i,
                    kind,
                    operand,
                    dest: inst.dest,
                    live: lv.live_after(ssa, b, i),
                    in_try: try_depth > 0,
                    in_loop: in_loop[b],
                });
            }
            if matches!(inst.kind, InstKind::Try { .. }) {
                try_depth += 1;
            }
        }
    }
    out
}

/// `true` para los bloques alcanzables desde sí mismos, o sea los que están en
/// un ciclo del CFG. Es la definición que le importa al pase: un estado situado
/// en un bloque así es reentrable.
fn compute_in_loop(ssa: &SsaFunc) -> Vec<bool> {
    let n = ssa.blocks.len();
    let mut reach = vec![vec![false; n]; n];
    for (b, block) in ssa.blocks.iter().enumerate() {
        for s in succs(block) {
            reach[b][s] = true;
        }
    }
    // Clausura transitiva (Floyd-Warshall booleano). Los CFG de una función
    // caben de sobra en O(n^3) a estos tamaños.
    for k in 0..n {
        for i in 0..n {
            if reach[i][k] {
                for j in 0..n {
                    if reach[k][j] {
                        reach[i][j] = true;
                    }
                }
            }
        }
    }
    (0..n).map(|b| reach[b][b]).collect()
}

fn succs(block: &super::ir::Block) -> Vec<usize> {
    let mut v = Vec::new();
    for inst in &block.insts {
        if let InstKind::Try { handler } = &inst.kind {
            v.push(handler.0 as usize);
        }
    }
    match &block.term {
        Terminator::Jump { target, .. } => v.push(target.0 as usize),
        Terminator::Branch {
            then_blk, else_blk, ..
        } => {
            v.push(then_blk.0 as usize);
            v.push(else_blk.0 as usize);
        }
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => {}
    }
    v
}
```

Declarar el módulo en `crates/varn-compiler/src/ssa/mod.rs`, junto a los demás:

```rust
pub mod suspend;
```

- [ ] **Step 3: Añadir la fase de depuración**

Registrar `suspend` en `crates/varn-debug/src/flags.rs`, siguiendo el patrón exacto de `("ssa", "forma SSA")` en las dos listas donde aparece (la de descripción y el `match` que activa el flag). Crear `crates/varn-debug/src/suspend.rs` copiando la estructura de `crates/varn-debug/src/ssa.rs` — leerlo primero y seguir su forma — para volcar, por función, una línea por punto de suspensión con: bloque, instrucción, tipo, operando, destino, tamaño de `live`, y las banderas `in_try`/`in_loop`. Declararlo en `crates/varn-debug/src/lib.rs` junto a los demás módulos.

- [ ] **Step 4: Verificar contra las expectativas del paso 1**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
cargo build --release 2>&1 | grep -E "^(error|warning)" | head
./target/release/vn.exe debug -p suspend "$SCRATCH/suspend-cases.vn" 2>&1
```

Contrastar contra la tabla del paso 1. **Los cuatro casos deben salir como dice la tabla.** Si `enBucle` no marca `in_loop`, o `enTry` no marca `in_try`, el análisis está mal y hay que arreglarlo antes de seguir — son exactamente los dos casos que el spec (§9, riesgos 2 y 3) señala como los que rompen las implementaciones ingenuas.

- [ ] **Step 5: Medir el corpus real — el producto de este plan**

> **No medir sobre `tests/main.vn`.** El volcado SSA se invoca una sola vez, para el módulo de entrada (`compile.rs:90`), a diferencia del de bytecode que además recorre el grafo de módulos (`compile.rs:127`). Y esa vía tampoco serviría: `graph_build.modules` guarda `FunctionProto`, no SSA — a esa altura el SSA ya no existe. `main.vn` es un driver que importa el resto, así que medirlo daría casi cero.
>
> Los ficheros de `tests/` **sí** compilan por separado como módulo de entrada. Verificado: `tests/21-async.vn` da 7 funciones en SSA con 19 `await` en fuente. Recorrerlos uno a uno mide código real por el camino que funciona.

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
: > "$SCRATCH/suspend-corpus.txt"
for f in tests/*.vn std/*.vn; do
  echo "### $f" >> "$SCRATCH/suspend-corpus.txt"
  ./target/release/vn.exe debug -p suspend "$f" 2>&1 \
    | sed 's/\x1b\[[0-9;]*m//g' >> "$SCRATCH/suspend-corpus.txt"
done
echo "puntos de suspension totales: $(grep -cE 'Await|Yield' "$SCRATCH/suspend-corpus.txt")"
echo "dentro de try:               $(grep -c 'in_try=true' "$SCRATCH/suspend-corpus.txt")"
echo "dentro de bucle:             $(grep -c 'in_loop=true' "$SCRATCH/suspend-corpus.txt")"
echo "--- distribucion del tamano de live ---"
grep -oE 'live=[0-9]+' "$SCRATCH/suspend-corpus.txt" | sort -t= -k2 -n | uniq -c
```

Referencia de cordura: el corpus tiene 139 `await` y 103 apariciones de `async` en fuente. Si el total de puntos sale muy por debajo de eso, el análisis no está llegando a todas las funciones y hay que averiguar por qué antes de usar las cifras. Algunos ficheros pueden fallar al compilar sueltos si dependen de otros; eso es esperable y no invalida la medición — anotar cuántos fallaron.

Anotar las cuatro cifras en el informe. **Son la entrada de la decisión de diseño de P2b**: si casi ningún punto cae en un `try` o en un bucle, el pase puede aterrizar primero el caso recto y tratar esos dos como fases posteriores; si son mayoría, hay que resolverlos desde el día uno.

- [ ] **Step 6: Verificar y commitear**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
"$SCRATCH/exact.sh"
"$SCRATCH/verify.sh"
```

Esperado: byte a byte `OK`, matriz de 4 verde. El análisis es de sólo lectura y la fase de depuración no corre salvo que se pida, así que el bytecode no puede haberse movido.

```bash
git add crates/varn-compiler/src/ssa/suspend.rs crates/varn-compiler/src/ssa/mod.rs crates/varn-debug/src/suspend.rs crates/varn-debug/src/flags.rs crates/varn-debug/src/lib.rs
git commit -m "feat(compiler): analisis de puntos de suspension

Localiza cada Await/Yield con el conjunto vivo que lo cruza — los campos
que necesitara el objeto de estado — mas si cae dentro de un try y si su
bloque esta en un ciclo del CFG. Solo lectura, no transforma.

El pase de maquinas de estados lo consumira en vez de recalcularlo, para
que 'que es un punto de suspension' y 'que debe guardar el estado' tengan
una sola definicion.

Visible por vn debug -p suspend."
```

---

## Cierre del plan

- [ ] **Matriz de 4 completa, incluido `bench`**

```bash
cd /c/Users/x/dev/varn/varn-lang
./target/release/vn.exe cache clean
./target/release/vn.exe bench ./tests/main.vn -v 2>&1 | grep -E "p50 e2e|cobertura clif"
VARN_NO_JIT=1 ./target/release/vn.exe bench ./tests/main.vn -v 2>&1 | tail -3
```

- [ ] **Recoger las cifras para P2b**

Copiar al informe final las cuatro cifras del Task 4 paso 5. Son el insumo de la decisión de diseño de P2b y el motivo de que este plan exista por separado.

---

## Fuera de alcance de este plan

- **El pase de transformación en sí.** Es P2b, y **no puede escribirse todavía**: el spec fijó el modelo pero no la mecánica. Siguen sin decidir, y son decisiones de diseño, no de implementación:
  1. Cómo se representa el objeto de estado en el heap: ¿variante nueva de `HeapObj`, objeto con shape, o algo propio?
  2. Cómo invoca la VM a `poll`: ¿opcode nuevo, native, o entrada directa del JIT?
  3. Qué es `Task<T>` en runtime tras el cambio, y cómo convive con `Value::Task`/`Value::TaskHandle` mientras dure la migración.
  4. Cómo entra Cranelift: el spec afirma que `poll` es "una función corriente", lo que hay que confirmar contra `jit/tiering.rs` y el modelo de frames.
  5. Qué pasa con el camino de coste cero cuando el callee no se conoce estáticamente.

  Estas cinco necesitan una ronda de diseño con las cifras del Task 4 delante. Es el paso siguiente tras este plan.
- El scheduler, el I/O por readiness, la concurrencia estructurada y el reparto: pasos 3, 4 y 5 del spec.
- `fork_for_task`, `run_lazy_task_sync`, `NanGenDriver`, `wait_task_handle_value`, `VmSuspend`, el `unsafe impl Send` y los `thread::spawn` de `net.rs` siguen **intactos** al terminar este plan.
