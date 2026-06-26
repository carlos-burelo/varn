# Diseño — Motor Query Incremental para varn-lsp

Fecha: 2026-06-26
Rama: `perf/lsp-incremental-query-engine`
Estado: aprobado (brainstorming)

## 1. Problema

El LSP es lento y no deja margen para añadir features. El camino caliente
recomputa **todo** en cada pulsación de tecla:

```
did_change → analyze_and_publish → workspace.update_file → run_pipeline
             (lex + parse + assign_ast_ids + checker completo + SymbolRecords)
```

`TextDocumentSyncKind::FULL`, sin debounce, sin cancelación, sin
incrementalidad. Bottlenecks confirmados en el código actual:

1. **Cache de módulos tirada en cada request.** `invalidate_module_cache()`
   (`varn-checker/src/module_resolver.rs:201`) se llama al final de cada
   handler (hover, completion, definition, …) y al final de cada
   `run_pipeline`. El cache es `thread_local` `RefCell<Option<…>>`. Cada
   análisis re-resuelve stdlib + cada import desde disco/parse/check. Costo
   probable #1.
2. **Análisis síncrono en el hilo del executor.** `analyze_and_publish`
   (`backend.rs:36`) es `async` pero llama `run_pipeline` (CPU pesado) directo,
   sin `spawn_blocking`. Bloquea el worker de tokio → hover mientras se escribe
   se traba.
3. **Dependientes re-analizados full + síncrono por tecla.**
   `workspace/mod.rs:45`: editar un archivo muy importado re-corre el pipeline
   completo de cada dependiente, en el mismo hilo, en cada keystroke.
4. **`Rc<str>` en todo `Type`** (`varn-checker/src/types/mod.rs:30`) → no
   `Send`/`Sync` → `unsafe impl Send/Sync for DocumentState`
   (`document/mod.rs:203`). Razón raíz del monohilo; bloquea paralelismo.
5. **Proyección eager por análisis.** `inject_stdlib_symbols` + `to_string()`
   de `type_str`/`params_str` para cada símbolo (incl. stdlib) en cada
   análisis; `arena.clone()` + `scopes.clone()` enteros por análisis
   (`pipeline/mod.rs`).
6. **Scans O(n) por query.** `scope_at_offset` (`document/mod.rs:171`) recorre
   lineal todos los `node_scopes`; `tokens.iter().find()` por offset en cada
   query.
7. **Infra `Cached`/`Revision` muerta** — `Cached` nunca se instancia.

## 2. Objetivos / No-objetivos

**Objetivos**
- Recomputar **solo lo que cambió** por edición (incrementalidad real).
- Latencia por-keystroke O(1) en el handler (set input + bump revisión);
  cómputo pesado off-thread, debounced, cancelable.
- Editar el cuerpo de una función **no** invalida a los importadores.
- Análisis `Send + Sync`; eliminar `unsafe impl`.
- Dejar margen para features futuras (call hierarchy, type hierarchy, find-all
  cross-file, etc.) sin volver a tocar la arquitectura.

**No-objetivos**
- No adoptar `salsa` ni dep de motor incremental externo.
- No incrementalidad intra-función / sub-archivo (granularidad = archivo).
- No cambiar la semántica del lenguaje ni del checker (solo su forma de
  invocación y los tipos `Rc`→`Arc`).

## 3. Arquitectura objetivo

### 3.1 Motor query (hand-rolled, por archivo)

Grafo de queries memoizadas, demand-driven, con invalidación por revisión.
Las features piden outputs; el motor recomputa lo mínimo y sirve el resto de
cache.

**Inputs (única fuente mutable):**
- `FileId` interned (u32) reemplaza claves `String`/URI en todo el LSP.
  Interner bidireccional URI ↔ FileId.
- `source_text(FileId) -> Arc<str>`. `set_source` bumpea una revisión global
  monotónica y marca el `FileId` como cambiado en esa revisión.

**Queries derivadas (memoizadas):**
| Query | Depende de | Notas |
|---|---|---|
| `tokens(FileId)` | source_text | lex |
| `parse(FileId)` | tokens | Program + parse errors + ast ids |
| `module_exports(FileId)` | parse, bind | **firewall**: superficie pública |
| `bind(FileId)` | parse | scopes / resolución de nombres |
| `check(FileId)` | bind(self), module_exports(imports) | typecheck |
| `indexes(FileId)` | check | offset→token, offset→scope (ordenados) |
| `analysis(FileId)` | check, indexes | proyección LSP (partes caras lazy) |

**Memoización + invalidación.** Cada entrada cacheada guarda:
`{ value: Arc<T>, verified_at: Revision, changed_at: Revision, deps: Vec<QueryKey> }`.
Al pedir una query en la revisión `R`:
1. Si `verified_at == R` → hit directo.
2. Si no, re-verificar deps recursivamente. Si ningún dep tiene
   `changed_at > self.verified_at` → revalidar (subir `verified_at = R`) sin
   recomputar (**early-cutoff**).
3. Si algún dep cambió → recomputar; comparar resultado nuevo vs viejo; solo
   subir `changed_at = R` si el valor difiere (cutoff por igualdad, clave para
   el firewall).

Esto es el modelo rojo/verde de salsa reducido a granularidad de archivo y a
unos pocos cientos de líneas, sin proc-macros ni paradigma externo.

### 3.2 Query firewall: `module_exports`

`check(B)` depende de `module_exports(A)` (no de `check(A)`) para cada `A`
importado por `B`. `module_exports` proyecta solo la superficie pública
(nombres, kinds, firmas/tipos exportados) y se compara por igualdad.

- Editar el **cuerpo** de una función en A → `check(A)` recomputa, pero
  `module_exports(A)` da el mismo valor → `changed_at` no sube →
  `check(B)` revalida sin recomputar.
- Editar la **firma pública** de A → `module_exports(A)` cambia → `check(B)`
  recomputa.

Este cutoff es lo que hace al motor rápido en workspaces con imports y
sustituye al thread_local module cache + su invalidación destructiva.

### 3.3 Concurrencia (habilitada por Rc→Arc)

- Todos los valores derivados son `Arc<T>` `Send + Sync`. Se elimina
  `unsafe impl Send/Sync`.
- Handlers de request: toman un **snapshot** del DB (clon barato de Arcs /
  handle inmutable a la revisión actual) y ejecutan el cómputo en
  `spawn_blocking` (o pool rayon). Dejan de bloquear el executor de tokio.
- **Cancelación.** Un `did_change` que bumpea la revisión señala un
  `CancellationToken` para los cómputos de la revisión anterior. Las queries
  chequean el token en checkpoints (entre archivos, entre fases). Un cómputo
  cancelado descarta su resultado parcial (no envenena el cache).

### 3.4 Scheduling del hot path

- `did_change`: `set_source` + bump revisión. O(1). Sin cómputo.
- Diagnostics: **debounce** (~150 ms tras la última edición), cómputo
  off-thread, cancelable. Publica al terminar si la revisión sigue vigente.
- Cero re-análisis síncrono de dependientes. Se recomputan lazy on-demand
  (cuando una query los pide) o en el pass background de diagnostics del
  archivo activo.

### 3.5 Proyección lazy + índices posicionales

- Eliminar `inject_stdlib_symbols` por análisis. Stdlib se resuelve una vez como
  query cacheada keyed a la versión de stdlib (no se invalida en la sesión),
  compartida por `Arc`.
- `type_str` / `params_str` se computan **on-demand** en hover/completion, no
  `to_string()` de cada símbolo por análisis. `SymbolRecord` guarda el `Type`;
  el string se formatea al mostrar.
- `indexes(FileId)`: arrays ordenados `offset→token` y `offset→scope` con
  binary search, reemplazando los scans O(n) de `scope_at_offset` y
  `tokens.iter().find()`.

### 3.6 Estructura de crate (anti-god-object)

```
varn-lsp/src/
  db/
    mod.rs        # Database, snapshot, revisión, FileId interner
    input.rs      # inputs: source_text, set_source
    query.rs      # storage de memo + lógica verify/recompute/cutoff
    cancel.rs     # CancellationToken por revisión
  queries/
    syntax.rs     # tokens, parse
    semantics.rs  # bind, check
    exports.rs    # module_exports (firewall)
    indexes.rs    # índices posicionales + proyección
  features/       # handlers existentes, ahora consumidores de queries
  ...
```

## 4. Cambios en varn-checker (Rc→Arc)

- `Type` y tipos asociados: `Rc<str>` → `Arc<str>`, `Rc<BindResult>` → `Arc<…>`,
  cualquier `Rc<…>` en estructuras semánticas expuestas.
- Ripple mecánico a `varn-compiler` y `varn-vm`, que consumen `Type`. Ajuste de
  firmas; sin cambio de lógica.
- `check_for_lsp`: recibe los `module_exports` resueltos de los imports en lugar
  de resolver internamente vía el thread_local cache. El thread_local
  `MODULE_BIND_CACHE` / `MODULE_EXPORT_CACHE` y `invalidate_module_cache()` se
  eliminan; su rol lo cumple el motor query.
- Medir posible micro-overhead atómico de `Arc` vs `Rc` (esperado
  despreciable; la mayoría son clones de punteros en caminos no-hot).

## 5. Breaking changes

- `Rc`→`Arc` en la API pública de `varn-checker::types` (afecta compiler/VM).
- Claves `String`/URI → `FileId` interner en todo varn-lsp.
- `DocumentState` / `DocumentAnalysis` reemplazados por outputs de queries;
  `unsafe impl Send/Sync` borrado.
- `Cached` / `Revision` (`workspace/revision.rs`) eliminados; revisión la
  maneja el motor.
- `module_resolver` pierde el cache thread_local y `invalidate_module_cache`;
  los handlers dejan de llamarlo.

No se mantiene ruta legacy paralela (política de `evolution_strategy`): el
`workspace`/`pipeline` actual se reemplaza, no se duplica.

## 6. Fases (para el plan de implementación)

- **F0 — Rc→Arc + Send.** Migrar `varn-checker` y consumidores. Validar
  build + `tests/main.vn`. Sin cambio de comportamiento. Punto de medición base.
- **F1 — Motor query + inputs/derivadas por archivo.** `db/` + `queries/`
  para `tokens/parse/bind/check`. Reemplazar `run_pipeline` por queries para el
  archivo activo. Sin cross-file aún.
- **F2 — Firewall + cross-file.** `module_exports` con cutoff por igualdad;
  `check` depende de exports de imports. Eliminar thread_local module cache.
- **F3 — Concurrencia + scheduling.** Snapshot + `spawn_blocking`, cancelación
  por revisión, debounce de diagnostics, fin del re-análisis síncrono de
  dependientes.
- **F4 — Proyección lazy + índices + limpieza.** `type_str` lazy, stdlib query
  única, índices posicionales binary-search, borrar dead code
  (`Cached`/`Revision`, helpers muertos).

Cada fase deja el árbol compilando y `tests/main.vn` verde.

## 7. Validación y medición (performance_rules)

Benchmark obligatorio, mismo workload antes/después, documentando workload +
config + impacto:
- **Cold open** del workspace `tests/`.
- **Latencia por-keystroke** (tiempo del handler `did_change`).
- **Hover / completion** latencia bajo edición continua.
- **Edit en archivo muy importado** (medir que los dependientes NO recomputan
  cuando solo cambia un cuerpo).

Correctitud: `tests/main.vn` + suite de tests del checker verdes en cada fase.

## 8. Riesgos

- **Invalidación incorrecta** → resultados stale. Mitigación: cutoff por
  igualdad bien testeado; tests de incrementalidad (editar A, assert B no
  recomputa / sí recomputa según superficie).
- **Cancelación que envenena cache.** Mitigación: resultados parciales nunca se
  commitean; solo cómputos completos suben `verified_at`.
- **Blast radius de Rc→Arc** en compiler/VM. Mitigación: F0 aislada, build +
  tests antes de seguir.
- **Ciclos de imports.** Mitigación: detección de ciclo en la resolución de
  `module_exports`; romper con valor parcial determinista.
```
