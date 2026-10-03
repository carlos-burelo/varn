# Plan de raíz — auditoría 2026-10-03

Plan ejecutable por un agente. Se sigue **en orden, paso a paso, sin saltar puertas**.
Cada fase declara: objetivo, ganancia (a), verificación (b), qué se borra (c) — Ley 10.
Si algo de este plan contradice al código actual, **el código manda**: el agente se
detiene, reporta la discrepancia exacta (archivo:línea) y pregunta.

---

## 0. Reglas de operación (leer antes de tocar nada)

### 0.1 Entorno
- Windows 11. Herramientas: **Bash** (Git Bash, POSIX) y **PowerShell 7**. No hay `cmd.exe`.
- Raíz del repo (Bash): `/c/Users/x/dev/varn/varn-lang`. (PowerShell): `C:\Users\x\dev\varn\varn-lang`.
- Todo comando Bash empieza con `cd /c/Users/x/dev/varn/varn-lang && …`.
- Binario: `./target/release/vn.exe`. Si no existe o el código cambió, recompilar (0.3).
- Variables de entorno en Bash: prefijo inline (`VARN_NO_JIT=1 ./target/release/vn.exe …`).
  En PowerShell: `$env:VARN_NO_JIT='1'; …; Remove-Item Env:VARN_NO_JIT`.
- Salida de `vn` trae colores ANSI: filtrar siempre con `sed 's/\x1b\[[0-9;]*m//g'`.

### 0.2 Prohibido
- **Ningún comando `git`** (ni de lectura) salvo autorización explícita del usuario
  en esta sesión (ver 0.5). Nunca `git add -A`, `git add .`, `git add <directorio>`,
  `git stash`, `git checkout --`, `git reset`, `git clean`: el usuario tiene trabajo
  sin commitear en el árbol.
- `sleep` en primer plano. Builds largos van con `run_in_background: true`; se espera
  la notificación de fin, no se hace polling con sleep.
- Comentarios nuevos en código (Ley 11). Doc-comments nuevos tampoco.
- Tests unitarios Rust nuevos (`#[test]`) (Ley 11). Las regresiones son `.vn` en `tests/`.
- `_ =>` que esconda una variante sin decidir (Ley 7).
- `HashMap`/`HashSet` de `std` (con `RandomState`) en cualquier camino que decida orden
  de salida (Ley 4). Usar `IndexMap`, `BTreeMap`, `Vec` o `rustc_hash` solo para lookup.
- Flags duales, adapters, capas de compatibilidad, "camino viejo por si acaso" (Ley 8).
- Leer `.md` como evidencia de estado (Ley 12). Este plan es la única excepción.
- Archivos `.rs` de más de **400 líneas** tras el cambio (AGENTS.md §6). Si un archivo
  tocado los supera, dividir por dominio **antes** de seguir.

### 0.3 Comandos canónicos

| Nombre | Herramienta | Comando | Notas |
|---|---|---|---|
| CHECK | Bash | `cd /c/Users/x/dev/varn/varn-lang && cargo check --workspace 2>&1 \| tail -40` | timeout 600000. Detecta errores de tipos en `std/` (build.rs) |
| BUILD | Bash, **run_in_background** | `cd /c/Users/x/dev/varn/varn-lang && cargo build --release --bin vn 2>&1 \| tail -30` | ~3–5 min. Esperar notificación |
| CLIPPY | Bash, run_in_background | `cd /c/Users/x/dev/varn/varn-lang && cargo clippy --workspace --all-targets 2>&1 \| tail -60` | 0 errores |
| FMT | Bash | `cd /c/Users/x/dev/varn/varn-lang && cargo fmt --all` | después de cada paso |
| MATRIZ | Bash, run_in_background | `bash /c/Users/x/dev/varn/varn-lang/target/audit/matrix.sh` | 4 cuadrantes. Script en F0 |
| ERRCORPUS | Bash | `cd /c/Users/x/dev/varn/varn-lang && cargo test -p varn-cli --test error_corpus 2>&1 \| tail -30` | corpus negativo `tests/errors/` |
| TYPELOSS | Bash | `bash /c/Users/x/dev/varn/varn-lang/target/audit/typeloss.sh` | opcodes genéricos vs tipados en benches |
| BAILS | Bash, run_in_background | `bash /c/Users/x/dev/varn/varn-lang/target/audit/bails.sh` | funciones que no entran al JIT |
| BENCH | PowerShell | `& C:\Users\x\dev\varn\varn-lang\target\audit\bench.ps1` | medianas intercaladas + memoria pico |
| SIZE | Bash | `cd /c/Users/x/dev/varn/varn-lang && wc -l <archivos tocados> \| awk '$1>400'` | debe salir vacío (salvo `total`) |

**Paso verde** = CHECK sin errores → FMT → BUILD ok → MATRIZ `exit=0` en los 4 cuadrantes
→ ERRCORPUS ok → SIZE vacío → puertas propias de la fase. CLIPPY al cerrar cada fase.

Si MATRIZ falla solo con caché caliente: repetir con `VARN_CACHE_DIR` temporal
(`VARN_CACHE_DIR=$(mktemp -d) bash target/audit/matrix.sh`). Si con caché limpio pasa,
el bug es de frontera de módulo (Ley 2): arreglarlo, no limpiar caché.

Si falla solo en cuadrantes JIT: aislar con
`./target/release/vn.exe debug <archivo.vn> -p clif --fn <nombre>` y
`-p tiers`. Si falla solo en `@embedded`: el bundle stdlib (`crates/varn-cli/build.rs`).

### 0.4 Regresiones `.vn`
- Número siguiente: `cd /c/Users/x/dev/varn/varn-lang && ls tests | grep -E '^[0-9]+-' | sort -n | tail -1`
  y sumar 1. Nombre `NNN-tema-en-kebab.vn`.
- Formato (copiar exactamente este esqueleto):
  ```
  import { describe, it, expect } from "std:test";

  function caso1(): int { … }

  await describe("NNN tema", async () => {
      await it("1. descripción", async () => {
          expect(caso1()).toBe(…)
      })
  })
  ```
- Registrar en `tests/main.vn` añadiendo `import "./NNN-tema.vn"` **antes** de la línea
  `summary()`.
- Corpus negativo: `tests/errors/<tema>.vn` con primera línea `// expect: error[VNxxxx]`.
  Códigos nuevos en `crates/varn-core/src/diagnostics/catalog.rs` (siguiente número libre
  del rango que corresponda; leer el archivo para elegirlo).

### 0.5 Commits (Ley 9)
- Al empezar F0, preguntar al usuario con AskUserQuestion:
  "¿Autorizas commits atómicos por paso verde?" (Sí / No).
- **Sí**: tras cada paso verde, `git add <ruta1> <ruta2> …` (archivos explícitos, uno por
  uno, solo los que tocó el paso) y `git commit -m "<tipo>(<crate>): <qué>"` con mensaje en
  español, sin "y" en el asunto, terminando en línea en blanco +
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Nunca `push`.
- **No**: no ejecutar git. Mantener en `target/audit/CAMBIOS.txt` la lista de archivos
  por paso para que el usuario commitee.

### 0.6 Puertas de diseño (paradas obligatorias)
Las fases marcadas **[PARADA]** requieren aprobación del usuario antes de implementar.
El agente presenta: diseño en ≤40 líneas, (a) ganancia esperada, (b) cómo se verifica,
(c) lista de cosas que se borran, y pregunta con AskUserQuestion. Sin "Sí" no se implementa.

### 0.7 Reporte por fase
Al cerrar cada fase, el agente responde en el chat (no en `.md`): pasos hechos,
números antes/después de cada puerta, archivos tocados, y lo que quedó sin hacer con motivo.

---

## F0 — Preparación y línea base

**Objetivo:** binario fresco, scripts de validación, números de referencia.

1. Preguntar autorización de commits (0.5).
2. BUILD (background). Esperar notificación. Si falla, **parar** y reportar el error.
3. Crear `target/audit/` y los scripts siguientes **con este contenido exacto** (Write tool):

   `target/audit/matrix.sh`
   ```bash
   #!/usr/bin/env bash
   cd /c/Users/x/dev/varn/varn-lang || exit 2
   VN=./target/release/vn.exe
   fail=0
   run() {
     local name="$1"; shift
     local out code summary
     out=$(env "$@" "$VN" run tests/main.vn 2>&1); code=$?
     summary=$(printf '%s\n' "$out" | sed 's/\x1b\[[0-9;]*m//g' | grep -E "PASSED|FAILED|ALL TESTS" | tail -3 | tr '\n' ' ')
     echo "$name exit=$code $summary"
     if [ $code -ne 0 ]; then
       fail=1
       printf '%s\n' "$out" | sed 's/\x1b\[[0-9;]*m//g' | grep -iE "fail|error|panic" | head -20
     fi
   }
   run "Q1 dev+JIT"
   run "Q2 dev+NOJIT" VARN_NO_JIT=1
   run "Q3 emb+JIT" VARN_STD=@embedded
   run "Q4 emb+NOJIT" VARN_STD=@embedded VARN_NO_JIT=1
   exit $fail
   ```

   `target/audit/typeloss.sh`
   ```bash
   #!/usr/bin/env bash
   cd /c/Users/x/dev/varn/varn-lang || exit 2
   VN=./target/release/vn.exe
   for f in tests/benchmarks/vn/*.vn; do
     "$VN" debug "$f" -p typeloss 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep "│" | grep -v "función"
   done | awk -F'│' '{g+=$2; t+=$3; n=split($4,a,","); for(i=1;i<=n;i++){gsub(/^ +| +$/,"",a[i]); split(a[i],b," "); split(b[1],c,"×"); k=c[1]; m=(c[2]==""?1:c[2]); if(k!="") op[k]+=m}} END{print "GENERIC",g,"TYPED",t; for(k in op) print "  ",op[k],k}' | sort -k1,1 -rn
   ```

   `target/audit/bails.sh`
   ```bash
   #!/usr/bin/env bash
   cd /c/Users/x/dev/varn/varn-lang || exit 2
   VN=./target/release/vn.exe
   "$VN" debug tests/main.vn -p bails 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep " words │" | grep -v "razón" \
     | awk -F'│' '{r=$3; gsub(/[0-9]+/,"N",r); gsub(/ +$/,"",r); c[r]++; t++} END{print "BAILS",t; for(k in c) print "  ",c[k],k}' | sort -k1,1 -rn
   ```

   `target/audit/bench/ret.vn`
   ```
   class P { x: int; y: int; constructor(x: int, y: int) { this.x = x; this.y = y } }
   function run(): int {
     let xs: P[] = []
     let i = 0
     while (i < 3000000) { xs.push(new P(i, 1)); i = i + 1 }
     let s = 0
     for (let p of xs) { s = s + p.x }
     return s
   }
   print(run())
   ```
   `target/audit/bench/ret.js`
   ```js
   class P { constructor(x, y) { this.x = x; this.y = y } }
   function run() { const xs = []; for (let i = 0; i < 3000000; i++) xs.push(new P(i, 1)); let s = 0; for (const p of xs) s += p.x; return s }
   console.log(run())
   ```
   `target/audit/bench/modloop.vn`
   ```
   class P { x: int; y: int; constructor(x: int, y: int) { this.x = x; this.y = y } }
   let pt = new P(1, 2)
   let i = 0
   let sum = 0
   while (i < 20000000) { pt.x = pt.x + 1; sum = sum + pt.x + pt.y; i = i + 1 }
   print(sum)
   ```
   `target/audit/bench/fnloop.vn`
   ```
   class P { x: int; y: int; constructor(x: int, y: int) { this.x = x; this.y = y } }
   function run(): int { let pt = new P(1, 2); let i = 0; let sum = 0
     while (i < 20000000) { pt.x = pt.x + 1; sum = sum + pt.x + pt.y; i = i + 1 }
     return sum }
   print(run())
   ```
   `target/audit/bench/iface.vn`
   ```
   interface Shape { area(): float }
   class Sq implements Shape { s: float; constructor(s: float) { this.s = s } area(): float { return this.s * this.s } }
   class Ci implements Shape { r: float; constructor(r: float) { this.r = r } area(): float { return 3.0 * this.r * this.r } }
   function total(xs: Shape[]): float { let t = 0.0; for (let s of xs) { t = t + s.area() } return t }
   function run(): float { let xs: Shape[] = []; let i = 0
     while (i < 1000) { if (i % 2 == 0) { xs.push(new Sq(1.0)) } else { xs.push(new Ci(1.0)) } i = i + 1 }
     let acc = 0.0; let k = 0
     while (k < 5000) { acc = acc + total(xs); k = k + 1 }
     return acc }
   print(run())
   ```
   `target/audit/bench/record.vn`
   ```
   type P = { x: int, y: int }
   function f(p: P): int { return p.x + p.y }
   function run(): int { let ps: P[] = []; let i = 0
     while (i < 1000) { ps.push({ x: i, y: 1 }); i = i + 1 }
     let s = 0; let k = 0
     while (k < 5000) { for (let p of ps) { s = s + f(p) } k = k + 1 }
     return s }
   print(run())
   ```
   `target/audit/bench/generic.vn`
   ```
   function sum<T extends Add>(xs: T[], z: T): T { let t = z; for (let x of xs) { t = t + x } return t }
   function run(): int { let xs: int[] = []; let i = 0
     while (i < 1000) { xs.push(i); i = i + 1 }
     let s = 0; let k = 0
     while (k < 20000) { s = s + sum(xs, 0); k = k + 1 }
     return s }
   print(run())
   ```

   `target/audit/bench.ps1`
   ```powershell
   param([int]$Runs = 5)
   $root = "C:\Users\x\dev\varn\varn-lang"
   $vn = "$root\target\release\vn.exe"
   $b = "$root\target\audit\bench"
   $cases = @(
     @{n="ret.vn"; exe=$vn; a="run $b\ret.vn"},
     @{n="ret.js(node)"; exe="node"; a="$b\ret.js"},
     @{n="ret.js(bun)"; exe="C:\Users\x\.bun\bin\bun.exe"; a="$b\ret.js"},
     @{n="modloop.vn"; exe=$vn; a="run $b\modloop.vn"},
     @{n="fnloop.vn"; exe=$vn; a="run $b\fnloop.vn"},
     @{n="iface.vn"; exe=$vn; a="run $b\iface.vn"},
     @{n="record.vn"; exe=$vn; a="run $b\record.vn"},
     @{n="generic.vn"; exe=$vn; a="run $b\generic.vn"}
   )
   $times = @{}; $peaks = @{}; $outs = @{}
   foreach ($c in $cases) { $times[$c.n] = @(); $peaks[$c.n] = 0 }
   for ($r = 0; $r -lt $Runs; $r++) {
     foreach ($c in $cases) {
       $o = "$env:TEMP\varn_bench_out.txt"
       $sw = [Diagnostics.Stopwatch]::StartNew()
       $p = Start-Process $c.exe -ArgumentList $c.a -PassThru -NoNewWindow -RedirectStandardOutput $o
       $pk = 0
       while (-not $p.HasExited) { try { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $pk) { $pk = $p.PeakWorkingSet64 } } catch {}; Start-Sleep -Milliseconds 5 }
       $sw.Stop()
       $times[$c.n] += $sw.ElapsedMilliseconds
       if ($pk -gt $peaks[$c.n]) { $peaks[$c.n] = $pk }
       $outs[$c.n] = (Get-Content $o -Raw).Trim()
     }
   }
   foreach ($c in $cases) {
     $s = $times[$c.n] | Sort-Object
     $med = $s[[int][math]::Floor($s.Count / 2)]
     "{0,-14} median={1,6} ms  peak={2,5} MB  out={3}" -f $c.n, $med, [math]::Round($peaks[$c.n] / 1MB), $outs[$c.n]
   }
   ```

4. Crear las reproducciones de bugs en `target/audit/repro/` (se usan en F1):

   `target/audit/repro/nullish_getter.vn`
   ```
   let calls = 0
   class C { get v(): int? { calls = calls + 1; return 7 } }
   let c = new C()
   let r = c.v ?? 0
   print(calls)
   ```
   Esperado: `1`. Hoy: `2`.

   `target/audit/repro/compound_getter.vn`
   ```
   let calls = 0
   class C { v: int = 1 }
   class H { _c: C = new C(); get c(): C { calls = calls + 1; return this._c } }
   let h = new H()
   h.c.v += 5
   print(calls)
   h.c.v++
   print(calls)
   ```
   Esperado: `1` y `2`. Hoy: `2` y `4`.

   `target/audit/repro/shortcircuit_hoist.vn`
   ```
   let calls = 0
   function side(): int? { calls = calls + 1; return 5 }
   let ok = false
   let r = ok && ((side() ?? 0) > 1)
   print(calls)
   let t = ok ? (side() ?? 0) : 9
   print(calls)
   ```
   Esperado: `0` y `0`. Hoy: `1` y `2`.

   `target/audit/repro/implicit_dynamic.vn`
   ```
   function mystery(cb) { return cb(1) }
   print(mystery((n) => n + 1))
   ```
   Esperado tras F3: error de compilación. Hoy: compila e imprime `2`.

5. Ejecutar y guardar la línea base:
   - `bash target/audit/matrix.sh > target/audit/base_matrix.txt 2>&1` (background). Debe dar
     `exit=0` ×4. Si no, **parar**: el árbol ya está rojo; reportar al usuario.
   - `bash target/audit/typeloss.sh > target/audit/base_typeloss.txt`
   - `bash target/audit/bails.sh > target/audit/base_bails.txt` (background)
   - PowerShell: `& C:\Users\x\dev\varn\varn-lang\target\audit\bench.ps1 | Tee-Object C:\Users\x\dev\varn\varn-lang\target\audit\base_bench.txt` (timeout 600000)
   - Para cada repro: `./target/release/vn.exe run target/audit/repro/<x>.vn` y
     `VARN_NO_JIT=1 …`; guardar salidas en `target/audit/base_repro.txt`.
6. Reportar la línea base (0.7). Referencia de la auditoría (puede variar por ruido):
   typeloss GENERIC 1858 / TYPED 1886; BAILS ≈ 700 (654 async/generator, 39 "passes Dynamic");
   `ret.vn` 498 MB vs node 356 / bun 315; `modloop` ≈ 1.8× `fnloop`.

---

## F1 — Secuenciación de expresiones en TIR (corrección)

**Bugs:** (1) `is_pure` sintáctico duplica getters/índices al clonar subárboles TIR;
(2) `hoist` empuja `Let` a `pending`, que se vacía **antes del statement**, sacando
evaluaciones de ramas condicionales (`&&`, `||`, `?:`, `??`).
**(a)** Semántica correcta; un `MapGetIndex` menos en `m[k] ?? d`. **(b)** Las 3 repros +
regresión `.vn` + MATRIZ. **(c)** Se borra `is_pure` (checker) y la duplicación por `clone()`.

Archivos de partida (verificados 2026-10-03):
- `crates/varn-tir/src/node.rs` — `pub enum TirExprKind` (l.107).
- `crates/varn-checker/src/emit/body/scope.rs` — `fn hoist` (l.51), `pending`, `take_pending`.
- `crates/varn-checker/src/emit/body/expr_member.rs` — `fn is_pure` (l.21), uso en l.62.
- `crates/varn-checker/src/emit/body/expr_binary.rs` — `fn lower_logical` (l.166).
- `crates/varn-checker/src/emit/body/expr_assign.rs` (l.62, l.67), `expr_update.rs` (l.24, l.29),
  `body.rs` (l.248, l.284), `expr_try.rs` (empuja `TirStmt::If` a `pending`), `expr_convert.rs`.
- `crates/varn-compiler/src/from_tir/build.rs` — lowering de `TirExprKind::Select` (l.79).
- `crates/varn-tir/src/verify/` — verificador TIR.

Pasos:
1. Añadir a `TirExprKind` la variante
   `Seq { stmts: Vec<TirStmt>, value: Box<TirExpr> }` (efectos en orden, luego valor; tipo = `value.ty`).
2. CHECK. Cada `match` sobre `TirExprKind` que rompa (compiler `from_tir`, verificador TIR,
   cualquier visitor/`coverage.rs`, JIT si consume TIR): decidir el brazo explícito (Ley 7).
   En `from_tir`: bajar `stmts` con el mismo builder de statements que usa el cuerpo de
   función, luego bajar `value`. Listar los sitios con
   `grep -rn "TirExprKind::Select" crates --include=*.rs` (cada uno necesita su brazo `Seq`).
3. En el emisor del checker, crear un ámbito de pendientes por rama:
   `fn lower_scoped(&mut self, e: ExprId) -> TirExpr` que guarda `pending` actual,
   baja `e`, toma lo nuevo de `pending`, restaura, y si lo nuevo no está vacío devuelve
   `Seq { stmts: nuevo, value }`.
4. Usar `lower_scoped` para **toda subexpresión evaluada condicionalmente**:
   rhs de `&&`/`||`, rhs de `??`, ambas ramas de `?:`, el acceso de `?.`, y cualquier otra
   que construya `Select`/rama. Buscar todos con
   `grep -rn "TirExprKind::Select {" crates/varn-checker/src --include=*.rs`.
5. Sustituir cada `if is_pure(..) { x } else { hoist(x) }` por `hoist(x)` incondicional
   (getters, índices y operadores ya no se duplican). Borrar `fn is_pure` y sus
   referencias. Confirmar con `grep -rn "is_pure" crates/varn-checker` → vacío.
6. Verificar que la copia de un temporal no cuesta: `./target/release/vn.exe debug target/audit/repro/nullish_getter.vn -p bytecode`
   tras BUILD: no debe haber `Move` extra sin uso; si aparecen, es trabajo de copy-prop en
   SSA (`crates/varn-compiler/src/passes/`), no razón para restaurar `is_pure`.
7. BUILD, correr las 3 repros en JIT y `VARN_NO_JIT=1`. Esperado: `1`; `1 2`; `0 0`.
8. Regresión `tests/NNN-expr-sequencing.vn` con los tres casos (contadores de llamadas
   como `expect(...).toBe(...)`), más: `m[k] ?? d` con `Map`, `a?.b ?? c`, `x ||= f()`,
   `x ??= f()`, `try`-operador dentro de rama de `?:` (si `expr_try.rs` aplica).
9. Paso verde. Commit (si autorizado): `fix(checker): secuenciar efectos de subexpresiones condicionales en TIR`.

**Puerta F1:** 3 repros correctas en ambos tiers; MATRIZ verde; `is_pure` no existe en checker.

---

## F2 — Identidad de tipos portable

**Problema (código):** `content_id` (`crates/varn-checker/src/types/interned/hash.rs:21`)
hashea `InternedTypeKind` que contiene `Atom(u32)` posicional
(`crates/varn-core/src/atom.rs:6`). La identidad depende del orden de interning, por eso
`ImportResolver` (`module_resolver/resolver_api.rs`) expone `interner_snapshot`,
`interner_len`, `intern`, `set_ty_table`, y `DiskResolver::set_interner`. El hash son dos
`FxHasher` (no resistente a colisiones). `Symbol` (`crates/varn-checker/src/symbol.rs`)
marca `name`, `doc`, `type_params`, `origin_module` como `#[serde(skip)]`.
**(a)** Ids de tipo iguales sin importar orden de bindeo/paralelismo; se elimina el estado
mutable compartido. **(b)** Determinismo (pasos 9–10) + MATRIZ con caché frío y caliente.
**(c)** Se borran los 5 métodos de tabla compartida del trait y del `DiskResolver`, los
`#[serde(skip)]` de nombres en `Symbol`, y la traducción de nombres en `PortableModule`.

Pasos:
1. Dependencia de hash de 128 bits: `cd /c/Users/x/dev/varn/varn-lang && cargo add -p varn-core xxhash-rust --features xxh3`.
   Si falla por red, **parar** y preguntar (no implementar un hash a mano).
2. En `varn-core`: tipo `NameId(u128)` = `xxh3_128(texto)`, `Copy, Eq, Ord, Hash, Serialize, Deserialize`,
   y `NameTable` (`id → Arc<str>`, unión conmutativa e idempotente). En `intern`, si el id
   ya existe con texto distinto → `Diagnostic` interno (no `panic`, Ley 5).
3. Cambiar el parámetro `N` de `InternedTypeKind` (`types/interned/ids.rs`) de `Atom` a `NameId`.
4. Cambiar `hash128` (`types/interned/hash.rs`) a `xxh3_128` sobre una serialización
   estable del shape (bytes de `postcard::to_allocvec(kind)` o un `Hasher` que alimente
   `xxh3`). Prohibido seguir usando `FxHasher` para identidad.
5. CHECK y arreglar cada sitio que construye o lee `Named/Generic/Infer/EnumVariant/…`
   con `Atom`: convertir en la frontera `Atom → NameId` con el texto del interner local del
   módulo. El `AtomInterner` queda como tabla **de una sola fase y un solo módulo** (Ley 3).
6. `Symbol`: campos de nombre → `NameId`; quitar `#[serde(skip)]` de `name`, `doc`,
   `type_params`, `origin_module` (ahora son portables). `full_range` puede seguir skip.
7. Borrar de `ImportResolver` y de su única impl (`module_resolver/resolver_trait.rs`,
   `resolver_disk.rs`): `interner_snapshot`, `interner_len`, `intern`, `set_ty_table`,
   `ty_table_snapshot` si queda sin uso, `set_interner`. Cada llamador debe pasar a usar
   `NameId`/su tabla local. `grep -rn "interner_snapshot\|interner_len\|set_interner\|set_ty_table" crates` → vacío.
8. Simplificar `PortableModule` (`module_resolver/cache/cache_types.rs`, `from_live`/`into_live`):
   lo que ya es portable se serializa tal cual; borrar remapeos de nombres.
9. Determinismo: con caché limpio, dos corridas:
   `VARN_CACHE_DIR=$(mktemp -d) ./target/release/vn.exe debug tests/main.vn -p check:types 2>&1 | sed 's/\x1b\[[0-9;]*m//g' > target/audit/types_a.txt`
   y lo mismo a `types_b.txt`; `cmp target/audit/types_a.txt target/audit/types_b.txt` → sin diferencias.
   Repetir con caché caliente (sin `VARN_CACHE_DIR`) dos veces y comparar con `types_a.txt`.
10. Bytecode idéntico: `./target/release/vn.exe debug tests/main.vn -p bytecode > a.txt` dos veces
    (frío/caliente) → `cmp` igual (guardar en `target/audit/`).
11. Paso verde. Commits separados: (i) `NameId` + hash, (ii) borrado del estado compartido del resolver.

**Puerta F2:** pasos 9–10 idénticos; MATRIZ verde con caché frío y caliente; greps vacíos.

---

## F3 — `Error` separado de `dynamic`; `dynamic` implícito prohibido

**Problema (código):** `Type::Dynamic` aparece ~136 veces en `crates/varn-checker/src` como
fallback (p. ej. `types/type_impl.rs`); `function mystery(cb)` compila; `emit/ty/lower.rs`
baja lo desconocido a `BackendTy::Dynamic(DynReason::Unannotated)`; `emit/body/scope.rs`
cae a `Resolution::ByName` (búsqueda por nombre en runtime).
**(a)** Ningún error de tipos se convierte en código dinámico; base para layouts estáticos.
**(b)** Corpus negativo + `typeloss` no sube + MATRIZ. **(c)** Se borran `DynReason::Unannotated`,
`Resolution::ByName` y los fallbacks silenciosos.

Pasos:
1. Inventario: `grep -rn "Type::Dynamic" crates/varn-checker/src --include=*.rs > target/audit/dyn_sites.txt`.
   Clasificar cada línea en: **U** (el usuario escribió `dynamic` / frontera host) o
   **E** (fallo de inferencia/resolución). Guardar la clasificación en el mismo archivo.
2. Añadir el intrínseco `Type::Error` (id reservado junto a `Type::Dynamic` en
   `types/type_impl.rs` y `types/interned/ids.rs`). `Error` es asignable a/desde todo
   **sin emitir diagnóstico adicional** (evita cascadas) y nunca llega a emit.
3. Sitios **E** → `Type::Error` **más** un diagnóstico en el punto de origen
   (si ya existe uno, no duplicar). Sitios **U** se quedan.
4. Parámetros sin anotación: si no hay tipo contextual (lambda pasada donde el parámetro
   esperado tiene tipo función), diagnóstico nuevo `ImplicitDynamic` en `catalog.rs`.
   Variables sin inicializador y sin anotación: mismo diagnóstico.
5. Prelude/std: `cd /c/Users/x/dev/varn/varn-lang && grep -rn "dynamic" std --include=*.vn | wc -l` y CHECK.
   Cada error nuevo en `std/` se corrige anotando el tipo real. `print(...args: dynamic[])`
   se queda (frontera host explícita). `None: dynamic` → tipo `Option` correcto.
   Utilitarios `Pick/Omit/Partial/Readonly/ReturnType/Extract/NonNullable/Mutable`: si el
   checker los resuelve a `dynamic`, implementarlos sobre `ObjectMembersId` (Pick/Omit/
   Partial/Readonly/Mutable/NonNullable) o, si no se puede en esta fase, **parar y preguntar**.
6. Emit: `BackendTy::Dynamic(DynReason::Unannotated)` deja de producirse. Borrar la
   variante `Unannotated`; cada `match` decide su brazo (Ley 7).
7. `Resolution::ByName` (`emit/body/scope.rs`): un nombre sin resolver es error del checker,
   no búsqueda en runtime. Borrar la variante y el camino `LoadGlobal` por nombre si queda
   sin uso (`grep -rn "OpCode::LoadGlobal\b" crates`); si `OpCode::LoadGlobal` queda sin
   emisor, borrar el opcode y su dispatch.
8. Verificador TIR (`crates/varn-tir/src/verify/`): rechazar cualquier nodo con tipo `Error`.
9. Corpus: `tests/errors/implicit-dynamic-param-rejected.vn` (= repro `implicit_dynamic.vn`
   con `// expect: error[VNxxxx]`), `implicit-dynamic-let-rejected.vn`, y uno de nombre
   inexistente si no existe ya. ERRCORPUS.
10. TYPELOSS: GENERIC no debe subir respecto a base (debería bajar).
11. Paso verde. Commits: (i) `Type::Error`, (ii) diagnóstico `ImplicitDynamic` + std anotada,
    (iii) borrado de `Unannotated`/`ByName`.

**Puerta F3:** repro `implicit_dynamic.vn` rechazada; ERRCORPUS verde; MATRIZ verde.

---

## F4 — Modelo de objetos estático + heap con dueño único **[PARADA]**

Fase más grande. Se divide en F4.0 (investigación) → PARADA → F4.1–F4.5.

### F4.0 Investigación obligatoria (solo lectura y medición)
Responder con evidencia (archivo:línea o salida de comando):
1. ¿Qué opcodes construyen clases en runtime? Ejecutar
   `./target/release/vn.exe debug target/audit/bench/iface.vn -p bytecode | sed 's/\x1b\[[0-9;]*m//g' | grep -E "MakeClass|DeclareField|Method|Define|Inherit|BindMethod"`.
   Emisores: `crates/varn-compiler/src/ssa/emit/values/closure.rs:62`, `ssa/emit/effects.rs`.
2. ¿Algo muta una clase después de construida? Revisar decoradores
   (`crates/varn-checker/src/emit/decorators.rs`), `DefineStatic*`, `vtable_version`
   (`crates/varn-types/src/value/class.rs`), `Inherit` (`crates/varn-vm/src/exec/class.rs`).
3. Tipado estructural (verificado 2026-10-03): una clase **sin** `implements` se acepta como
   interfaz, y una instancia de clase con campos extra se acepta como record
   (`type P = {x:int}` ← `new Q()` con `x,y`). Los literales con campos extra se rechazan.
   Confirmar de nuevo con un `.vn` en `target/audit/` antes de diseñar.
4. Dónde se emiten `GetProperty`/`SetProperty`/`CallMethod`
   (`ssa/emit/values/access.rs:65`, `values/build.rs`, `values/calls.rs:40` `emit_method_call`).
5. Layout de clases existente: `crates/varn-types/src/class_layout.rs` (`ClassLayout`,
   `FieldLayout`, `GcLayout`), `GetFixedField/SetFixedField`, `InvokeVirtual`.
6. Heap: `crates/varn-vm/src/heap/` (`obj.rs` `HeapObj`, `structs.rs` `Heap`, `core.rs`,
   `gc.rs`, `access.rs`), nursery, `ObjRef(Rc<ObjData>)`, `InstanceRef(Rc<InstanceData>)`.
   Contar usos: `grep -rn "HeapObj::" crates --include=*.rs | wc -l`.

### Diseño a presentar en la PARADA (propuesta base; el agente la ajusta con F4.0)
- **Vistas (un único mecanismo para interfaz y record).** Toda forma con nombre tipado
  (clase, record cerrado, literal tipado) es una *clase* con `class_id` y layout fijo
  (`ClassLayout`). Un literal `{x, y}` cuyo tipo es un record cerrado instancia una clase
  anónima sintetizada por el checker (determinista: por `CheckerTyId` del record).
  Una **vista** = (tipo destino interfaz/record) con `view_id`. El checker, al probar una
  asignabilidad clase→vista, registra el par `(class_id, view_id)` en el artefacto del
  módulo. Al cargar, la VM construye una tabla densa `view_table[class_id][view_id]` →
  `[slot]` (offset de campo o índice de vtable). Acceso: `GetViewField r, view, k` y
  `CallView r, view, k` = leer `class_id` del objeto → tabla → offset/función. Sin nombres,
  sin IC, O(1), determinista.
- **Clases estáticas.** `ClassDescriptor` (nombre, `class_id`, layout, vtable inmutable,
  getters/setters como entradas de vtable, superclase resuelta) se emite en el artefacto del
  módulo y se registra al cargar. Se borran `MakeClass`, `DeclareField`, `Method`,
  `DefineStatic*`, `DefineGetter/Setter`, `Inherit`, `BindMethod` (si queda sin uso),
  `vtable_version`, `RefCell` de vtables, `vtable_owners`.
- **`dynamic` y mapas** siguen con `ObjData`/`Map`; índices `{[k: str]: T}` son `Map`.
- **Se borran en código tipado:** `GetProperty/SetProperty/CallMethod` por nombre,
  `PolyICSlot`, `FeedbackVector`, `ic_cache`, `resolved_shapes`, `BuildObjectWithShape`
  para literales tipados, y las transiciones de `Shape` fuera de `dynamic`.
- **Heap con dueño único.** Objetos con cabecera inline (`class_id: u32`, bits GC) y payload
  contiguo; nursery bump + copia; old-gen mark-sweep por clases de tamaño; `VmValue.payload`
  = puntero. Se borra la tabla de handles `Vec<Option<HeapObj>>` y los `Rc` de payload.
  Migración: el heap nuevo es el diseño final desde el primer paso; las variantes de
  `HeapObj` se mueven una a una (Instance → Array → Str → resto) y el handle table se borra
  cuando queda vacío. Cada movimiento es un paso verde.
- **(a)** objetivo medible: `ret.vn` memoria pico ≤ node (356 MB en la referencia) y
  `iface.vn`/`record.vn` −30 % de tiempo; **(b)** BENCH + TYPELOSS (GetProperty/CallMethod
  en `bench_*` sin `dynamic` → 0) + MATRIZ; **(c)** lista anterior.

Preguntar con AskUserQuestion: "¿Apruebas el diseño F4 (vistas + clases estáticas + heap sin handles)?"
Opciones: Aprobar todo / Solo objetos (sin heap) / Rechazar. **Sin aprobación, saltar a F5 y reportar.**

### F4.1–F4.5 (tras aprobación; cada uno es uno o más pasos verdes)
- **F4.1 Clases estáticas.** `ClassDescriptor` en el artefacto (`crates/varn-types/src/chunk/`),
  emisión desde `from_tir`, registro en la carga del módulo de la VM, `new` directo por
  `class_id`. Borrar opcodes de construcción de clases (listar con `grep -rn "OpCode::MakeClass\|OpCode::DeclareField\|OpCode::Method\b\|OpCode::Inherit\|OpCode::DefineGetter\|OpCode::DefineSetter\|OpCode::DefineStatic" crates`)
  en `varn-core/src/opcode.rs`, dispatch (`varn-vm/src/exec/dispatch/opgroups.rs`), JIT
  (`varn-jit/src/clif/alloc/safepoints.rs` y lowering), `varn-debug`. Puerta: el grep → vacío;
  MATRIZ verde.
- **F4.2 Records cerrados como clases sintetizadas.** Puerta: `record.vn` bytecode sin
  `GetProperty`; BENCH `record.vn` mejora.
- **F4.3 Vistas para interfaz y record estructural.** `GetViewField`/`CallView` en
  intérprete y JIT. Puerta: `iface.vn` sin `CallMethod`; TYPELOSS sin `GetProperty`/`CallMethod`
  en funciones sin `dynamic`.
- **F4.4 Borrado de IC/feedback.** `PolyICSlot`, `FeedbackVector`, `ic_cache`, `cache_count`
  si queda sin uso, `resolved_shapes` para tipados. `grep -rn "FeedbackVector\|PolyICSlot" crates` → vacío
  o solo en el camino `dynamic` (justificar cada línea que quede).
- **F4.5 Heap sin handles** (sub-pasos por variante). Puerta final: BENCH `ret.vn` peak
  ≤ 356 MB y tiempo ≤ node; MATRIZ verde; `grep -rn "Vec<Option<HeapObj>>" crates` → vacío.

---

## F5 — Monomorfización de genéricos **[PARADA]**

**Problema (código):** `sum<T extends Add>` emite `Add` genérico (ver
`./target/release/vn.exe debug target/audit/bench/generic.vn -p bytecode`); los parámetros
de tipo se bajan a `Dynamic` en `crates/varn-checker/src/emit/ty/lower.rs`.

### F5.0 Investigación
1. ¿Cómo baja `T`? Leer `emit/ty/lower.rs` (`TypeKind::Generic`, `resolve_type_ref`).
2. ¿Llega el cuerpo TIR de una función genérica importada al módulo consumidor? Revisar
   qué contiene la interfaz de módulo (`module_resolver/cache/cache_types.rs`).
3. Medir `generic.vn` (BENCH) como base.

### Diseño a presentar
- Clave de especialización = (función, tupla de tipos **backend** de los argumentos de tipo),
  ordenada con `BTreeMap` (Ley 4). Se emite una `TirFunction` por clave en el módulo que
  instancia. Los genéricos exportados llevan su TIR portable en la interfaz del módulo
  (requiere F2). Límite de explosión: si una función supera N especializaciones (proponer N=16),
  cae a una versión por clase física (GPR/FPR/REF/DYN), nunca a `dynamic` silencioso.
- Operadores restringidos (`T extends Add`) se resuelven al op tipado en cada especialización.
- **(a)** `generic.vn` sin `Add` genérico y ≥2× más rápido; **(b)** BENCH + bytecode + MATRIZ;
  **(c)** se borra el borrado de tipos (`T`→`Dynamic`) en emit.

Preguntar aprobación. Implementar en pasos verdes: (i) especialización intra-módulo,
(ii) TIR genérico en la interfaz + especialización cross-module, (iii) borrado del camino erasure.
Regresión `.vn` con genéricos sobre `int`, `float`, `str`, clase, y genérico importado.

---

## F6 — Async y generadores en el JIT **[PARADA]**

**Problema (código):** `crates/varn-jit/src/clif/lower.rs:275` rechaza todo
generator/async; BAILS base ≈ 654 por este motivo (incluye los `describe/it` de la suite).

### F6.0 Investigación
1. `crates/varn-compiler/src/ssa/suspend.rs` (análisis de suspensión),
   `crates/varn-vm/src/exec/jit_helpers/suspend.rs`, `crates/varn-vm/src/exec/scheduler/suspend.rs`,
   `FunctionProto.suspend_live` y `state_size` (`crates/varn-types/src/chunk/proto/definition.rs`).
2. Cómo reanuda hoy el intérprete un frame suspendido (`resume_ip`).
3. `./target/release/vn.exe debug tests/21-async.vn -p tiers` como base.

### Diseño a presentar
- Una función suspendible compila a una función CLIF con entrada `(frame, resume_state)`:
  bloque de despacho `switch resume_state` → bloque de reanudación de cada punto de
  suspensión. En cada `Await`/`Yield`: guardar los vivos (`suspend_live`) en el frame,
  escribir el estado, retornar al scheduler. Mismo formato de frame que el intérprete para
  que ambos tiers puedan reanudar lo que el otro suspendió.
- **(a)** BAILS por async/generator → 0; **(b)** BAILS + MATRIZ + bench async (crear
  `target/audit/bench/async.vn` con 1M awaits de una función trivial); **(c)** se borra el
  rechazo de `lower.rs:275` y el error "Await defines no value".

Preguntar aprobación. Pasos: (i) generadores síncronos, (ii) async sin captura, (iii) async
general, (iv) async generators. Cada uno verde.

---

## F7 — Módulos unificados, `FunctionProto` partido, SSA verificada

### F7.1 Un solo cargador de módulos
Estado (código): `varn_modules::loader::ModuleLoader` (`crates/varn-modules/src/loader.rs:124`)
con impls `ProviderLoader`, `ModuleRegistry`, `FilesystemLoader`, `MemoryLoader`;
`varn_vm::loader::ModuleLoader` (`crates/varn-vm/src/loader.rs:30`) + `CompositeLoader`;
impls en `crates/varn-pipeline/src/stdlib_loader.rs` (`FileLoader`, `StdlibLoader`);
`StdlibProvider` (`crates/varn-modules/src/provider.rs`) con `embedded_source`,
`source_path`, `interface_blob`, `bytecode_blob`, `bundled_source`;
`CarrierKind` (`crates/varn-checker/src/module_resolver/mod.rs:21`);
caso especial `core:types/aliases` en `module_resolver/cache/cache_io.rs:61,98`.

Pasos:
1. Mapa de llamadas: `grep -rn "dyn ModuleLoader\|impl ModuleLoader\|CompositeLoader\|StdlibProvider\|CarrierKind" crates --include=*.rs > target/audit/modules_map.txt`.
2. Diseño destino (presentar y aplicar sin parada salvo que rompa formato `.vnb`):
   - `varn_modules::ModuleLoader::load(id) -> ModuleArtifact { source, provenance, interface: Option<bytes>, code: Option<bytes> }`.
   - `StdlibProvider` se reduce a `fn module(&self, spec) -> Option<BundledModule>`.
   - La VM pide código compilado vía un único trait `ModuleCompiler` (en `varn-vm`, Ley 1)
     con una sola impl en `varn-pipeline`. Se borran `varn_vm::loader::ModuleLoader`,
     `CompositeLoader` y las impls duplicadas.
   - `CarrierKind` se deriva de `provenance`; si es igual a `Provenance`, se borra.
3. Borrar el caso especial `core:types/aliases`. MATRIZ con caché frío y caliente dos veces.
   Si falla: diagnosticar (Ley 2), no restaurar el caso especial.
4. Puerta: `grep -rn "pub trait ModuleLoader" crates` → 1 resultado; MATRIZ verde.

### F7.2 Re-exports enlazados por slot
Problema: el `<module>` de `std:math` hace `GetProperty×68` (ver TYPELOSS base).
1. `./target/release/vn.exe debug std/math/<entrada>.vn -p bytecode` para ver el patrón
   (encontrar la entrada con `ls std/math`).
2. El checker resuelve cada re-export a `(módulo, slot)` y el compilador emite
   `LoadModuleSlot` (ya existe: `ssa/emit/values/access.rs:170`).
3. Puerta: TYPELOSS sin `GetProperty` en funciones `<module>` de std.

### F7.3 `FunctionProto` = artefacto inmutable
Estado: `crates/varn-types/src/chunk/proto/definition.rs` mezcla campos serializados con
`Cell/RefCell` de runtime (`jit_entry`, `clif_raw`, `jit_code`, `jit_failed`, `jit_epoch`,
`jit_serial`, `jit_osr_*`, `trivial_init_memo`, y lo que sobreviva de F4.4).
1. Mover todo campo `#[serde(skip)]` de runtime a `FnRuntime` propiedad de la VM
   (por isolate), indexado por id de función. `FunctionProto` queda sin `Cell/RefCell/Rc<dyn Any>`.
2. Puerta: `grep -n "Cell<\|RefCell<" crates/varn-types/src/chunk/proto/definition.rs` → vacío; MATRIZ verde.

### F7.4 Invariantes SSA en el compilador, no en el JIT
Estado: ~39 bails "block N passes Dynamic value to Int parameter" detectados en
`crates/varn-jit/src/clif/from_ssa/cfg.rs`.
1. Añadir a `crates/varn-compiler/src/ssa/verify.rs` la regla: tipo de cada argumento de
   salto == tipo del parámetro de bloque destino.
2. Corregir los productores (emit/pases) hasta que la suite compile; cada violación es un
   bug del productor (insertar conversión explícita o unificar tipo del phi).
3. Puerta: BAILS sin "passes Dynamic"; MATRIZ verde.

### F7.5 El runtime no arrastra el frontend
Estado: `varn-vm → varn-op-macros → varn-parser` (`crates/varn-op-macros/Cargo.toml`).
1. Mover el parseo de contratos `.vn` a `build.rs` de `varn-builtins` (parser como
   `[build-dependencies]`), generando tablas Rust; `varn-op-macros` deja de depender de
   `varn-parser`/`varn-lexer`.
2. Puerta: `cd /c/Users/x/dev/varn/varn-lang && cargo tree -p varn-vm -e normal 2>/dev/null | grep -c "varn-parser"` → `0`.

---

## F8 — Costos directos del bytecode

### F8.1 Locales de módulo en registros
Estado: todo `let` top-level es `Resolution::GlobalSlot` (`crates/varn-checker/src/emit/body/scope.rs:149`,
`stmt_decl.rs:156`); `modloop.vn` ≈ 1.8× `fnloop.vn`.
1. Regla: binding top-level **no exportado** y **no referenciado por ninguna función/closure**
   del módulo → local del `<module>` (registro). El resto sigue global.
2. Puerta: BENCH `modloop.vn` ≤ 1.1× `fnloop.vn`; bytecode de `modloop.vn` sin
   `LoadGlobalIdx/DefineGlobalIdx` dentro del loop; MATRIZ verde.

### F8.2 Sin slot `this` en funciones libres
Estado: `fn f(p)` tiene `arity: 2`; cada llamada emite `LoadNull` + `Move`.
1. Investigar convención: `crates/varn-compiler/src/ssa/emit/values/calls.rs`,
   `crates/varn-vm/src/exec/calls/`, `CallSelf`, closures, `jit_call_native`, ABI CLIF de llamadas
   (`crates/varn-jit/src/clif/from_ssa/call/`).
2. `has_this == false` → la ventana de argumentos empieza en el arg 0; `arity` = parámetros reales.
3. Puerta: `-p bytecode` de `target/audit/repro/nullish_getter.vn` y de una función libre
   muestran `arity` = nº de parámetros, sin `LoadNull` previo al `Call`; MATRIZ verde; BENCH `bench_fib`
   (`./target/release/vn.exe run tests/benchmarks/vn/bench_fib.vn`) no empeora.

### F8.3 Cola muerta
Estado: toda función termina con `LoadNull; Return` inalcanzable tras un `Return`.
1. En el emisor de bytecode, no emitir el retorno implícito si el último bloque ya termina.
2. Puerta: `grep -c` de `LoadNull` tras `Return` en `-p bytecode` de `tests/main.vn` → 0.

---

## F9 — Re-auditoría de cierre (Ley 12: solo código y ejecución)

1. BUILD, MATRIZ, ERRCORPUS, CLIPPY.
2. TYPELOSS, BAILS, BENCH → `target/audit/final_*.txt`.
3. Repros de F0 en ambos tiers.
4. Reportar en el chat tabla base vs final de: GENERIC/TYPED, BAILS por motivo, cada fila de
   BENCH, repros, y lista de fases no ejecutadas con motivo (p. ej. PARADA rechazada).
5. Actualizar memoria del proyecto (directorio de memoria de la sesión) con el estado por fase.

---

## Orden y dependencias

```
F0 → F1 → F2 → F3 → F4[PARADA] → F5[PARADA] → F6[PARADA] → F7 → F8 → F9
                       F7.3 después de F4.4 · F5 (cross-module) después de F2
                       F8 es independiente: si una PARADA queda pendiente, avanzar a F7/F8
```
