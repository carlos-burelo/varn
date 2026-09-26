# Compartición estructural para `AtomInterner`/`CheckerTyTable` — plan

Fecha: 2026-09-25. Nace de una sesión de investigación de memoria del LSP
(`varn.memoryStats` + `dhat-heap`, ver `crates/varn-lsp/src/backend/mem.rs` y
`crates/varn-cli --features dhat-heap`). Se lee junto con
`docs/plans/2026-09-20-PLAN-PENDIENTE.md` (plan único vigente) — este documento
es un anexo específico, no lo reemplaza.

Prioridad de verdad: **código > tests > comportamiento > docs** (igual que el
plan principal). Reglas de trabajo: `AGENTS.md`.

---

## 1. El hallazgo (con evidencia medida, no estimada)

Indexando `varn-lang` completo (386 archivos `.vn`) desde el LSP:

- RSS observado: **1.45GB** (mimalloc) / **1.92GB** (bajo `dhat-heap`, overhead
  propio del profiler).
- **`dhat-heap.json` (bytes genuinamente vivos al salir, no "slack" del
  allocator): 1,473.8MB** antes del fix de esta sesión; **1,120.6MB** después
  de eliminar las capacidades fijas de `Checker::check_internal` (commit
  `6c3904d3`, ~350MB recuperados, verificado con profiler real, no solo RSS).
- De lo que queda vivo, los dos mayores contribuyentes por sitio de
  asignación:
  - `Arc<CheckerTyTable>::clone_from_ref_in` (deep clone disparado por
    `Arc::make_mut` en `DiskResolver::set_ty_table`): **~260MB**.
  - `DiskResolver::interner_snapshot` (el clon defensivo que
    `crates/varn-lsp/src/pipeline/mod.rs` sigue pagando una vez por archivo,
    tras eliminar el segundo clon redundante en `df821a56`): **~60MB**.

Ambos son **el mismo patrón**: una tabla que solo crece durante toda la sesión
(`AtomInterner`/`CheckerTyTable` en `DiskResolver`), compartida vía `Arc`, pero
con **copy-on-write que nunca ahorra nada** porque cada `BindResult` cacheado
en `Workspace.files` retiene su propio `Arc` **para siempre** (no hay
eviction). Cada vez que la tabla viva necesita crecer, `Arc::make_mut` la
encuentra con refcount > 1 y clona el contenido completo — el "COW" se
convierte en "clona siempre", igual de caro que un `.clone()` liso, pero con
más pasos.

### Por qué no es "Varn es lento", es este mecanismo específico

Comparado con LSPs de referencia del mismo tipo:

- **rust-analyzer** (salsa): el prelude/std se type-checkea una vez por
  sesión; cada consulta memoiza por revisión, nada se re-clona mientras la
  dependencia no cambió.
- **tsserver**: un `Program`/`Checker` único por proyecto; la tabla de tipos
  es global, no por archivo.
- **gopls** / **clangd**: mismo principio — lo importado se referencia, no se
  copia por quien importa.

`varn-checker` en cambio: `Binder::bind_with_globals_iter`
(`crates/varn-checker/src/binder.rs:298`) empuja cada símbolo del prelude
dentro del `arena`/`scope` **local de cada archivo**, y el mecanismo de
publicación (`DiskResolver::set_interner`/`set_ty_table`) fue diseñado como
"clona y fusiona" (Ley 6 lo documenta como append-only por texto, correcto en
espíritu) pero implementado sobre estructuras que **no soportan crecimiento
compartido barato** (`FxHashMap`/`Vec` lisos). El resultado: cada archivo paga
un clon O(n) de una tabla que solo crece, N veces por sesión → O(n²) agregado.

---

## 2. Por qué esto SÍ se puede arreglar sin tocar el eje que Varn mide

Ley 10 (`AGENTS.md`): *"el eje de rendimiento de Varn es el runtime
(TIR→SSA→regalloc→JIT→VM); una micro-ganancia en tiempo de compilación no
justifica `unsafe` ni romper garantías."*

`AtomInterner` y `CheckerTyTable` viven enteramente en `varn-checker`
(tiempo de compilación/check) y **nunca cruzan hacia `varn-vm`/`varn-jit`** —
esos consumen bytecode/SSA ya resuelto, con su propia representación de
valores runtime (`varn-types`, "dos mundos de tipos", ver AGENTS.md §1). Un
cambio a la representación interna de estas dos tablas no puede degradar la
ejecución de programas Varn compilados: no está en esa ruta.

Para el propio tiempo de compilación (no medido por Ley 10, pero sí relevante
para la experiencia de `vn check`/`vn build`/el LSP): cambiar a una estructura
con compartición estructural sube el costo de un lookup individual
(O(log₃₂ n) en vez de O(1) amortizado), pero **elimina el clon O(n) repetido
N veces** — la ganancia neta es clara mientras N (archivos por sesión) sea
mayor que 1, que es siempre el caso para el LSP.

---

## 3. Enfoques

### A — Estructura persistente completa (HAMT/RRB-tree)

Reemplazar el almacenamiento interno de `AtomInterner` (`FxHashMap<Box<str>,
Atom>` + `Vec<Box<str>>`) y de `CheckerTyTable` (cuatro `FxHashMap`) por
estructuras de tipo `im::HashMap`/`im::Vector` (o `rpds`), donde clonar es
O(1) (comparten nodos) y una escritura solo copia el camino hasta la raíz
(O(log n)), no la tabla entera.

- **Ventaja**: elimina el problema de raíz, en las dos tablas, de una vez.
  Ningún camino paralelo que mantener (Ley 8).
- **Costo**: nueva dependencia externa; cada lookup pasa de "un hash + un
  bucket" a "log₃₂(n) saltos de puntero" — más lento en el caso común,
  aunque el caso común (miles de átomos, no millones) hace esto
  imperceptible en la práctica.
- **Alcance del cambio**: la API pública de ambos tipos (`intern`, `resolve`,
  `get`, `len`, `iter_strings`) no cambia — solo su almacenamiento interno y
  su `impl Clone` (que pasa a ser barato). Los ~40 call sites que usan estas
  APIs no se tocan.

### B — Base congelada + delta chico (recomendado)

Partir cada tabla en dos partes:

- **Base**: un `Arc<[Box<str>]>`/equivalente inmutable, congelado la primera
  vez que se publica (p.ej. tras compilar el prelude completo). Lookups
  contra la base siguen siendo O(1) puro (índice directo a un slice, sin
  hashing siquiera si el `Atom` ya es su índice).
- **Delta**: un `FxHashMap`/`Vec` chico con lo interneado **después** de esa
  congelación — lo que un archivo individual añade. Clonar el delta es
  barato porque es chico por construcción (crece con lo que UN archivo o un
  puñado de archivos recientes agregó, no con toda la sesión).
- Periódicamente (o cuando el delta supera un umbral), el delta se "funde"
  en una nueva base congelada — una operación O(n) pero infrecuente, no una
  vez por archivo.

- **Ventaja**: sin dependencia nueva; el camino caliente común (resolver un
  átomo que ya estaba en la base, que es la mayoría después del arranque)
  no cambia de complejidad ni de constante — sigue siendo un lookup directo.
  Solo lo NUEVO paga el costo extra, y ese costo es proporcional a lo nuevo,
  no a la sesión completa.
- **Costo**: dos caminos de lectura (base + delta) en cada `resolve`/`get` —
  más código que mantener que el enfoque A, aunque acotado (Ley 8:
  "absolutista dentro del subsistema", una sola implementación, sin
  variantes duales una vez migrado).
- **Riesgo**: el umbral de fusión y la política de cuándo fusionar son un
  parámetro nuevo que ajustar (determinismo: Ley 4 exige que la fusión NO
  afecte el resultado, solo el momento en que ocurre — verificar con la
  matriz de `tests/main.vn`).

### C — No compartir el snapshot en absoluto (descartado)

Dejar de retener `bind.ty_table`/`bind.interner` en los `BindResult`
cacheados y resolver siempre contra la tabla viva del resolver (mismo
patrón que `resolve_atom_text` en `checker_expressions/members/mod.rs`,
ya usado esta sesión). Descartado como solución única: `bind.ty_table` se
usa genuinamente para resolución cross-módulo cuando otro archivo importa
símbolos de este (confirmado leyendo `binder/types.rs:297`,
`BindView::ty_table`) — un archivo importado puede necesitar responder
sobre sus propios tipos exportados mucho después de que se cacheó, y para
eso necesita SU tabla, no la del importador. Vale como optimización
puntual en sitios donde de verdad no hace falta retención (como ya se hizo
para `SemanticDB.types` vía `Arc::make_mut`, commit `cffed5d2`), pero no
sustituye compartición estructural en la tabla base.

**Recomendación**: B primero (measure again tras implementarlo); si el
delta se vuelve la parte cara del perfil, migrar a A para esa tabla
específica. No implementar A de entrada sin medir B — Ley 10 exige
declarar ganancia medible antes de romper compatibilidad interna, y B es
la ruta más barata de medir sin comprometerse a una dependencia nueva.

---

## 4. Alcance del cambio (qué se toca, qué no)

Toca únicamente:
- `crates/varn-core/src/atom.rs` (`AtomInterner`).
- `crates/varn-checker/src/types/interned.rs` (`CheckerTyTable`).
- `crates/varn-checker/src/module_resolver/resolver.rs` (`DiskResolver`'s
  `interner`/`ty_table` fields y `set_interner`/`set_ty_table`).

No toca (por diseño, ver §2):
- `varn-vm`, `varn-jit`, `varn-types` — el runtime no ve estas tablas.
- La API pública de `AtomInterner`/`CheckerTyTable` — los ~40 call sites en
  `varn-checker`/`varn-lsp`/`varn-pipeline` que llaman `intern`/`resolve`/
  `get`/`len` no cambian.

---

## 5. Puerta de validación

Además de la matriz estándar (`AGENTS.md` §5):

1. `varn.memoryStats` (comando LSP, `crates/varn-lsp/src/features/
   compiler_inspect/mod.rs`) antes/después sobre el mismo workspace
   (`varn-lang` completo, 386 archivos) — comparar `residentKb` y el
   desglose por campo.
2. Repetir la captura `dhat-heap` (`cargo build -p varn-cli --features
   dhat-heap`) para confirmar que `clone_from_ref_in`/`interner_snapshot`
   dejan de aparecer entre los mayores contribuyentes por bytes vivos.
3. `tests/main.vn` en los 4 cuadrantes — Ley 4 (determinismo): el momento en
   que la base se congela/funde no puede cambiar qué bytecode se produce.
4. Tiempo de `vn check`/`vn test` sobre `tests/main.vn` antes/después — para
   confirmar que el costo extra por lookup (enfoque A) o por delta (enfoque
   B) no es perceptible en la práctica, no solo en teoría.

---

## 6. Riesgos

- **Determinismo**: cualquier política de "cuándo fundir el delta" o
  cualquier estructura persistente con iteración no determinista
  (`im::HashMap`'s iteration order, si se usa el enfoque A) puede violar
  Ley 4. Verificar que `iter_strings()`/iteración de `CheckerTyTable` siga
  siendo determinista para quien dependa del orden (cache en disco,
  `module_resolver::cache`).
- **Costo de migración**: `impl Clone` para ambas tablas deja de ser un
  `#[derive(Clone)]` trivial; hay que escribirlo a mano y cubrirlo con los
  tests existentes de `checker_ty_table_invariants`/
  `module_cache_atom_roundtrip` (ya existen, confirmado en
  `crates/varn-checker/tests/`).
- **No hacerlo a medias**: si se elige B, no dejar una tercera tabla
  "por si acaso" — la base y el delta son el único mecanismo (Ley 8); si se
  elige A, borrar por completo el camino `FxHashMap` viejo en el mismo
  esfuerzo.

---

## 7. Estado

Pendiente real, no iniciado. Este documento es el resultado de la
investigación de memoria de esta sesión (commits `df821a56`, `cffed5d2`,
`6c3904d3`, `a508d849`, `73c10191`, `c094cc83` en `varn-lang`); no reemplaza
al plan principal (`2026-09-20-PLAN-PENDIENTE.md`), que sigue siendo la
prioridad activa (Fase F5/F6 del JIT). Este anexo espera a que se decida
dedicarle una sesión propia.
