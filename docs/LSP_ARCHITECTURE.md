# Arquitectura del Language Server (`varn-lsp`) — Rediseño

Este documento define la arquitectura objetivo del servidor de lenguaje de **Varn** y el plan de migración desde la implementación actual. Es un documento de **destino**, no una descripción del estado presente: cada sección marca explícitamente qué existe hoy y qué debe reemplazarse.

La implementación actual es funcional a medias y está construida sobre heurísticas. Este rediseño elimina la causa raíz de esas heurísticas en lugar de refinarlas.

---

## Tabla de Contenidos

- [1. Diagnóstico de la arquitectura actual](#1-diagnóstico-de-la-arquitectura-actual)
- [2. Principio rector y sus límites](#2-principio-rector-y-sus-límites)
- [3. La decisión keystone: checker sin estado global](#3-la-decisión-keystone-checker-sin-estado-global)
- [4. Lo que deliberadamente NO se hace](#4-lo-que-deliberadamente-no-se-hace)
- [5. Las capas](#5-las-capas)
  - [L0 — Lexer total](#l0--lexer-total)
  - [L1 — Parser como función total](#l1--parser-como-función-total)
  - [L2 — Identidad estable: muerte del offset-keying](#l2--identidad-estable-muerte-del-offset-keying)
  - [L3 — Un solo modelo semántico](#l3--un-solo-modelo-semántico)
  - [L4 — Motor de queries con firewall](#l4--motor-de-queries-con-firewall--parcial-y-deliberadamente)
  - [L5 — Propiedad y threading](#l5--propiedad-y-threading)
  - [L6 — Superficie LSP](#l6--superficie-lsp)
  - [L7 — Extensión](#l7--extensión)
- [6. Plan de migración](#6-plan-de-migración)
- [7. Validación](#7-validación)
- [8. Inventario de eliminación](#8-inventario-de-eliminación)

---

## 1. Diagnóstico de la arquitectura actual

Cinco defectos estructurales, ordenados por gravedad. Todos verificados contra el código.

### 1.1 Unsoundness (UB real) — ✅ corregido en L5

`crates/varn-lsp/src/document/mod.rs:209-210`:

```rust
unsafe impl Send for DocumentState {}
unsafe impl Sync for DocumentState {}
```

`DocumentState` contiene `SemanticDB` → `BindResult` + `Type`, saturados de `Rc<str>` y `Rc<CoreMembers>`. El backend clona `Arc<DocumentState>` hacia `spawn_blocking` (`backend.rs:81`) mientras handlers async leen el mismo `Arc`. Cada clone/drop de un `Type` toca refcounts **no atómicos** desde dos hilos: data race, refcount corrupto, use-after-free.

Ese `unsafe impl` no resuelve nada: apaga la única verificación que impedía el error.

### 1.2 Caché de módulos `thread_local` en un servidor multihilo

`crates/varn-checker/src/module_resolver/store.rs:8-14` guarda el grafo de módulos en `thread_local! { RefCell<...> }`. Hay dos cachés más del mismo tipo en `binder/type_resolution/aliases.rs:18-19` y `core/loader.rs:11-12`.

Consecuencia: `Workspace::update_file` llama `invalidate_module_cache()` (`workspace/mod.rs:41`), que invalida **solo el hilo actual**. El resto de workers del pool conserva binds obsoletos. La resolución cross-module es correcta o no según qué worker atendió la petición. Es la causa del comportamiento no determinista.

Efecto secundario: cada worker nuevo re-parsea y re-bindea la stdlib completa desde cero.

### 1.3 Modelo semántico paralelo

`pipeline/mod.rs` (460 líneas) ejecuta lex + parse + check y **además** materializa `SymbolRecord`/`MemberRecord`: una segunda base de datos semántica, denormalizada, con los tipos ya aplanados a `String`. Es información que `CheckResult` ya contiene tipada.

Cada pulsación de tecla reconstruye todo eso, más un re-análisis **síncrono** de todos los dependientes (`workspace/mod.rs:85-98`).

No hay memoización. El comentario de `update_file` anuncia un *"Query Firewall check"* que no existe; `workspace/revision.rs::Cached` se define, se reexporta y nunca se construye.

### 1.4 Heurísticas en cascada por doble fuente de verdad

`resolve_receiver_type_name_at` (`document/chain_queries.rs:238`) devuelve un **`String`** como identidad de tipo y decide comparándolo contra `"dynamic"` / `"unknown"` — 21 ocurrencias de esos literales en el crate.

Se apoya en `receiver_ast::find_member_receiver_type`, que **re-camina el AST entero desde la raíz** en cada hover/goto (O(AST) por petición) con un `match` manual que ya tiene huecos (`ExportDefaultDecl::Class => None`, expresiones `match`, cuerpos de arrow).

Y es solo la primera de tres capas: AST-walk → token anterior al punto → lexema crudo. Tres fallbacks porque ninguno es autoritativo.

El checker ya responde esto: `member_resolutions[offset]` contiene `receiver_ty: Type`, `member_kind`, `def_range`, `origin_module`, `doc` (`semantic_info.rs:35-43`). `features/semantic_tokens/classify.rs` lo usa correctamente y es, en consecuencia, el archivo más limpio del crate.

### 1.5 El checker no puede ver código incompleto

**Este es el defecto que hace que los cuatro anteriores no se puedan arreglar por separado.**

- El AST **no tiene nodo de error**: `ExprKind` tiene más de cuarenta variantes y ninguna es `Missing` o `Error`.
- `parse_program_partial` recupera a nivel de statement: ante un fallo reporta `InvalidStatement` y llama `recover()`, que salta al siguiente `;` o cierre de llave. **El statement fallido se descarta entero.**

Cuando el usuario escribe `const x = foo.`, la declaración completa desaparece del árbol. El checker nunca ve `foo`, no bindea `x`, y no hay entrada en las tablas de tipos. No existe nada que una completion basada en el checker pueda leer.

Por eso `completion::dot_receiver` trabaja sobre el token stream crudo. **Esas heurísticas no son deuda por descuido: compensan un hueco real del frontend.** Reconstruir el LSP sin cerrar este hueco las hace volver idénticas.

---

## 2. Principio rector y sus límites

> **El checker es la única autoridad sobre semántica. Para que eso baste, el parser debe producir un árbol para todo input, incluido el inválido.**

El principio tiene un límite explícito, y omitirlo genera heurísticas nuevas en la dirección contraria:

| Dominio | Autoridad | Ejemplos |
|---|---|---|
| Sintaxis y texto | lexer + parser | formatting, folding, on-type formatting, selection ranges, trivia, posiciones |
| Semántica | checker | tipos, resolución de símbolos y miembros, llamadas, `origin_module`, sitios de definición |
| Grafo de módulos | motor de queries | workspace symbols, referencias cross-file, auto-import, dependientes |

Tres fuentes, dominios disjuntos. Ninguna reconstruye lo que otra ya decidió.

---

## 3. La decisión keystone: checker sin estado global ✅ implementado

Hoy el checker guarda su grafo de módulos en tres bloques `thread_local!`. Es un god-object encubierto: estado mutable de proceso, invisible en las firmas, invalidado por efecto secundario.

**Destino:**

```rust
pub trait ImportResolver {
    fn resolve(&self, from: ModuleId, spec: &str) -> Option<ModuleId>;
    fn exports(&self, module: ModuleId) -> Option<&ExportMap>;
    fn bind(&self, module: ModuleId) -> Option<&BindResult>;
}

pub fn check(
    program: &Program,
    imports: &dyn ImportResolver,
    opts: CheckOptions,
) -> CheckResult;
```

El checker pasa a ser una **función pura**: no abre archivos, no cachea, no invalida. Quien posee el grafo lo inyecta — el LSP inyecta su motor de queries; el CLI, un resolver de disco.

Lo que desbloquea de un solo golpe:

- Desaparece el bug de invalidación por hilo (§1.2): no queda caché que invalidar.
- Desaparece la razón del `unsafe impl Send` (§1.1): nadie extrae `Rc` de un `thread_local`.
- `vn check` se vuelve determinista; hoy el resultado depende de qué tenía cacheado el hilo.
- El checker se vuelve usable desde contextos paralelos: isolates, compilación batch, tests.
- El motor de queries puede memoizar de verdad, porque las dependencias quedan **explícitas en la firma**.

Es el breaking change más profundo del plan y el de mayor retorno.

### Lo que la implementación reveló

El obstáculo real no era enhebrar un parámetro. Era que `impl TypeContext for BindResult` le daba a un tipo **serializable y cacheado como `Rc`** la capacidad de resolver módulos — y un valor `'static` cacheado no puede sostener un resolver, así que esa capacidad **solo podía ejercerse vía estado global**. El `thread_local` no era una comodidad: era la única forma de que ese diseño funcionara.

La separación que lo desbloquea:

```rust
pub struct BindView<'r> {
    pub bind: &'r BindResult,             // dato: cacheable, serializable
    pub resolver: &'r dyn ImportResolver, // capacidad: no serializable
}
```

`BindResult` conserva solo métodos `_local`. Seguir un import exige construir una vista, y **el compilador lo impone**: no queda forma de resolver sin haber recibido un resolver.

Elección de `&'r dyn` sobre `Rc<dyn>`: `DiskResolver::bind_and_cache` construye binders mientras resuelve, así que un handle propietario haría la propiedad circular. Con préstamo, pasa `self`.

### Estado

`varn-checker` no contiene **ningún** `thread_local!`. Se eliminaron los tres:

| Origen | Destino |
|---|---|
| `module_resolver/store.rs` (6 cachés) | `ModuleGraph`, poseído por `DiskResolver` |
| `binder/type_resolution/aliases.rs` (caché + flag de reentrancia) | el flag pasó a ser `DiskResolver::in_flight`, un guard de ciclo general |
| `core/loader.rs` (`CORE_EXPORTS`/`CORE_MEMBERS`) | `ImportResolver::core_exports`/`core_members`, memoizados por resolver |

Mover el preludio corrigió un bug latente aparte: era un memo **de proceso** de la stdlib, así que un proceso que cambia de procedencia de std (el LSP lo hace, entre el árbol y el bundle embebido) seguía respondiendo con la primera que vio, para siempre.

Cada host posee ahora su resolver: `varn-pipeline` uno por hilo de pipeline (correcto — cada `vn run` analiza un programa en un hilo y termina), `varn-lsp` el suyo.

### Lo que esto NO arregla todavía

**El bug de invalidación por hilo del LSP sigue vivo.** `DiskResolver` memoiza con `Rc`/`RefCell`, así que no puede compartirse a través del pool de bloqueo donde corre el análisis hoy; cada worker construye e invalida su propia copia.

Lo que cambió es que ahora ese es el **único** sitio donde puede pasar, y es visible: el checker ya no esconde una caché tras sus firmas. Confinar el análisis a un hilo (L5) colapsa esto a un grafo coherente sin volver a tocar el checker.

---

## 4. Lo que deliberadamente NO se hace

**No se adopta un CST lossless al estilo `rowan`.**

Un árbol sin pérdida con reparse incremental e identidad de nodo estable entre ediciones compra latencia de reparse en archivos grandes. Cuesta reescribir `varn-parser` y sus dos consumidores (checker y lowering) al modelo de árbol no tipado con fachada tipada.

Para Varn es la decisión equivocada: los archivos son de cientos a pocos miles de líneas y el reparse completo es sub-milisegundo. El cuello de botella real no es reparsear un archivo, es **reanalizar el workspace entero por pulsación** — y eso lo resuelve el firewall de §L4, no el CST.

Copiar la arquitectura de `rust-analyzer` aquí sería, en sí mismo, el error arquitectónico: pagar una reescritura del frontend del compilador por latencia que ya se tiene.

---

## 5. Las capas

### L0 — Lexer total ✅ implementado

**Invariante:** ningún byte del fuente se pierde en el lexeo. Rangos de tokens + rangos de comentarios + el texto fuente cubren el archivo entero.

`scanner/comments.rs` descartaba comentarios de línea y bloque; solo los doc comments se emitían. Se perdían antes de que existiera un árbol, y por eso `FoldingRangeKind::Comment` estaba declarado sin que nada pudiera producirlo nunca.

**Decisión (corrige la primera versión de este documento):** la trivia va en un **stream paralelo**, no mezclada en el de tokens con un flag `is_trivia`.

El motivo es el parser. Tolera `TokenKind::DocComment` inline solo porque los doc comments son legales en exactamente tres posiciones, donde los comprueba a mano (`stmts.rs:9`, `decls/class.rs:61`, `decls/type_decls.rs:352`). Un comentario corriente aparece en cualquier sitio — `a + /* c */ b` — así que interleavearlos obligaría a filtrar en cada `peek`/`advance` del camino caliente del parser. El stream paralelo deja el vector de tokens byte a byte idéntico y el parser sin tocar.

**Qué se materializa:** solo comentarios. Los espacios son exactamente el hueco entre rangos de tokens adyacentes, así que siguen siendo derivables; los comentarios no lo son. `Trivia` lleva rango, no texto — el consumidor ya tiene el fuente.

**Coste cero para el compilador:** `LexerConfig::emit_trivia` es `false` por defecto y `varn_lexer::scan` no cambió de firma; sus diez llamadores siguen intactos. Solo `scan_with_trivia` la activa, y solo el LSP la llama.

Consumidor conectado: folding de comentarios (`features/folding.rs::fold_comments`) — bloques `/* */` multilínea y rachas de `//` consecutivas. El formatter con preservación de comentarios es el siguiente consumidor natural.

### L1 — Parser como función total ✅ implementado

**Invariante:** `&[Token] -> Program` nunca falla y nunca descarta input. Todo byte del fuente es alcanzable desde el árbol.

Dos variantes nuevas en `varn-core`:

```rust
ExprKind::Missing,                       // hueco donde se esperaba una expresión
StmtKind::Error { recovered: SourceRange } // span recuperado, no descartado
```

`recover()` (`parser/mod.rs:93`) **envuelve** el span consumido en `StmtKind::Error` en vez de tirarlo. El rango basta: el token stream se re-corta por rango, sin duplicar tokens dentro del AST.

La variante que importa para completion es `ExprKind::Missing`: ante `foo.` seguido de fin de línea o EOF, el parser produce `Member { object: foo, property: Missing }` y la declaración que lo contiene **parsea correctamente**. El símbolo `foo` se bindea, obtiene tipo, y la completion pasa a ser una consulta al checker.

Contratos de los consumidores:

- **Checker**: `Missing` → `Dynamic`, sin diagnóstico. El parser ya reportó el error; duplicarlo produce ruido rojo mientras se escribe.
- **Compiler**: `Missing` / `StmtKind::Error` → error duro antes del lowering. El backend nunca los ve.

### L2 — Identidad estable: muerte del offset-keying

**Invariante:** la posición del cursor se traduce a identidad **una sola vez, en el borde**. Hacia adentro todo es `AstId` o `SymbolRef`.

Hoy `CheckResult` expone cuatro mapas indexados por offset `u32` — `expr_types`, `node_scopes`, `member_resolutions`, `call_resolutions` — que existen únicamente porque el LSP pregunta por posición. Eso fuerza escaneos lineales (`expr_info_at_token` recorre el mapa entero) y la borrosidad de "¿este offset cae dentro del token?".

Destino: todos indexados por `AstId`. `expr_table` (que ya es el registro autoritativo) se conserva; **`expr_types` se elimina**.

El LSP construye un único índice espacial por parse:

```rust
struct SpatialIndex { by_start: Vec<(SourceRange, AstId)> } // ordenado por start

impl SpatialIndex {
    fn innermost_at(&self, offset: u32) -> Option<AstId>; // O(log n)
}
```

Trabajo colateral obligatorio:

- Eliminar `varn_core::ast::assign_ast_ids` — hoy es un cuerpo vacío (`ast/mod.rs:33`) que el LSP invoca en cada pipeline creyendo que hace algo.
- Eliminar `Expr::new_with_range` (`expr.rs:289`), que asigna `id: 0` y colisiona con el primer nodo real. Bug latente.

### L3 — Un solo modelo semántico — 🔄 en curso

**Hecho: el receptor de un acceso a miembro viene del checker.**

`resolve_receiver_type_name_at` consulta `member_resolutions[offset].receiver_ty`. Antes re-caminaba el AST entero desde la raíz en cada hover y goto — O(AST) por petición, con un `match` manual que necesitaba una rama por cada `ExprKind` nuevo y ya tenía huecos.

`document/receiver_ast.rs` (365 líneas) **borrado**. El fallback por token sigue, pero ya no tapa un hueco del checker: responde el único caso del que el checker legítimamente no tiene nada que decir, un miembro leído sobre un valor `dynamic`.

Cubierto por `tests/receiver_from_checker_test.rs`, con los casos que el walk fallaba y que ningún fallback por token puede responder — el token antes del punto es `)` o `]`:

| Caso | Receptor |
|---|---|
| `make().length` | tipo de retorno de la llamada |
| `xs[0].length` | tipo del elemento |
| `s.trim().length` | tipo de retorno del miembro anterior |

**Hecho: los miembros builtin vienen de la stdlib.**

`util/intrinsic_members.rs` (153 líneas) **borrado**. Era una transcripción a mano de las firmas de `std/` —`Map.set`, `Range.toArray`, `Array.length`— indexada troceando el tipo impreso del receptor (`split('<')`, `ends_with("[]")`), y **respondía antes que el checker**: para todo tipo que cubría, la tabla ganaba. Cualquier divergencia con `std/` salía como un hover seguro de sí mismo y equivocado.

Ahora `resolve_chain_at` lee `member_resolutions[offset]` y construye el registro desde lo que el checker decidió. La mejora es medible en fidelidad, no solo en procedencia:

```
a.map     -> (method) int[].map(callback: (item: int, index: int, array: int[]) => U): U[]
mp.values -> (method) Map<int>.values(): int[]
```

La tabla no podía expresar un callback genérico, y reconstruía `Map<int>` troceando su forma impresa.

**Hecho: una sola construcción de la clave de símbolo, y references/rename sobre miembros.**

Había tres constructores de `global_key`. Dos discrepaban: `stable_global_key` normaliza una ruta desnuda a un `file://` URI y `pipeline/symbols.rs` interpolaba `origin_module` crudo — para cualquier símbolo cuyo origen fuese una ruta, claves que nunca comparan iguales. Latente (ahí el origen siempre es `std:`/`core:`/`runtime:`), pero el contrato declarado no se cumplía. Ahora hay un solo constructor.

El tercero **sí estaba vivo**. `references` y `rename` derivaban el *objetivo* con `symbol_global_key_for_id` (forma `u:`/`m:`) y cada *candidato* con `token_global_key` (forma `member:{tipo}:{nombre}` para miembros de clase). Dos formas para una misma pregunta: sobre un miembro nunca podían comparar iguales, así que **"find all references" sobre un campo devolvía nada y "rename" no editaba nada**, en silencio.

Pasó desapercibido porque sobre una función top-level ambas formas coinciden — y eso es lo primero que se prueba a mano. El arreglo es simetría: una sola función deriva ambos lados, así que coinciden por construcción.

Verificado contra el servidor vivo: `references` sobre un campo pasó de `null` a 4 (declaración, `this.value`, dos `w.value`); `rename` de 0 ediciones a 4. Fijado en `tests/member_references_test.rs`, que empareja siempre el caso miembro con el caso función para que un arreglo no pueda cambiar uno por otro.

**Pendiente:** el resto del modelo paralelo. `SymbolRecord`/`MemberRecord` los consumen 16 archivos; borrarlos exige reescribir hover, completion, symbols, definition, inlay hints e index builder para proyectar de `CheckResult`. Quedan 18 literales `"dynamic"`/`"unknown"` como identidad de tipo, todos fuera del camino del receptor.

**Invariante:** el LSP no construye estructuras semánticas. Proyecta `CheckResult` a tipos LSP en el momento de responder.

- Identidad de símbolo: `SymbolRef(FileId, SymbolId)`, newtype `Copy`. **`global_key: String` desaparece**, y con él `stable_global_key`, `symbol_global_key_for_id` y el `starts_with("member:…")` de `features/definition.rs:176`.
- `type_str` / `params_str` se formatean al responder, nunca se precomputan ni se almacenan.
- El checker expone API **tipada** para lo que el IDE necesita, en lugar de dejar que lo reconstruya. Aplicación directa de `<backend_principle>`:

```rust
fn members_of(&self, ty: &Type) -> Vec<ResolvedMemberSummary>;   // ya existe
fn def_site(&self, s: SymbolRef) -> Option<(ModuleId, SourceRange)>;
fn doc(&self, s: SymbolRef) -> Option<&str>;
fn signature(&self, s: SymbolRef) -> Option<&FunctionType>;
```

### L4 — Motor de queries con firewall — ⚠️ parcial, y deliberadamente

**Invariante:** editar el cuerpo de una función no recomputa ningún archivo dependiente.

**Ese invariante ya se cumplía** (`update_file` compara el mapa de exports antes de re-analizar dependientes). Lo que no se cumplía era un invariante más básico: cada pulsación llamaba `reset()` sobre el grafo de módulos entero, tirando toda la stdlib.

Corregido: se invalida solo el módulo editado y, por el BFS de `reverse_deps`, sus importadores transitivos.

#### Medición

Latencia por pulsación, leída del `[Nms]` que el servidor registra por análisis, sobre el binario real por stdio. 8 ediciones, mediana:

| Archivo | `reset()` | Invalidación dirigida |
|---|---|---|
| 2 imports de std | 10 ms | **5 ms** |
| 7 imports de std | 14 ms | **7 ms** |

Sonda en `scratchpad/lspbench.py`; workload y método arriba.

#### Por qué el motor de queries completo no se construyó

La medición es la razón. Se esperaba que `reset()` fuera catastrófico —re-parsear toda la std por tecla— y resultó costar 5–7 ms, porque `try_load_cache` deserializa los blobs `.vnm` precompilados en lugar de re-parsear.

A 5–7 ms por pulsación, un motor demand-driven memoizado (`source → tokens → tree → bind → check`) es complejidad que no compra latencia observable. Se construye cuando una medición muestre que hace falta, no antes.

Lo que **sí** sigue pendiente y es independiente de esto: `run_pipeline` reconstruye el modelo paralelo (§1.3) en cada análisis. Ese coste lo elimina L3, borrando el modelo, no memoizándolo.

```
source(FileId) → tokens(FileId) → tree(FileId) → bind(FileId) → check(FileId)
                                       ↓
                                 exports(FileId)   ← el firewall
```

`check(A)` depende de `exports(B)`, **no** de `check(B)`. Al editar un cuerpo de función `check` cambia pero `exports` no, y los dependientes no recomputan. Es el mayor salto de latencia del plan y es exactamente lo que `update_file` simula hoy sin implementarlo.

Este motor implementa el `ImportResolver` de §3. Aquí vive el grafo de módulos que hoy está en los `thread_local` del checker.

### L5 — Propiedad y threading ✅ implementado

**Invariante:** ninguna estructura con `Rc` cruza un límite de hilo, y **el compilador lo verifica**.

`DocumentState` ya no es `Send` ni `Sync`. El `unsafe impl` que lo forzaba está borrado y no se reemplazó por nada.

El actor (`analysis/mod.rs`) es un hilo dedicado que posee el `Workspace`. Las peticiones llegan como **closures**:

```rust
pub async fn run<R, F>(&self, f: F) -> Option<R>
where
    F: FnOnce(&mut Analyzer) -> R + Send + 'static,
    R: Send + 'static;
```

`R: Send` es la cota que sostiene todo. Un handler que intentara devolver un `DocumentState` — o cualquier cosa que lo contenga — **no compila**. La frontera dejó de ser una promesa en un comentario.

Se eligió un hilo de SO, no una tarea tokio: los trabajos son CPU-bound y el estado que tocan no es `Send`, así que debe quedarse quieto en vez de migrar entre workers.

**Esto cierra el bug de invalidación por hilo.** `DiskResolver` memoiza por hilo; con el análisis repartido sobre un pool, cada worker mantenía su copia y una invalidación en uno dejaba obsoletos los demás. Un hilo, un grafo, una invalidación.

Lo que se queda fuera del hilo de análisis: el debounce (esperar ahí aparcaría todas las peticiones detrás de una tecla) y el I/O del indexado inicial (recorrido de directorios y lectura de archivos van en `spawn_blocking`; solo el análisis de cada archivo se envía al actor).

`Rc → Arc` queda disponible como evolución posterior si alguna vez se quiere análisis paralelo por archivo. Nota sobre su viabilidad: el sistema de tipos ya enruta su puntero de nombres por un único alias —
`SemanticTypeKind = TypeKind<Box<Type>, Rc<str>, …>` (`checker/src/types/mod.rs:32`) — de modo que una línea voltea el sharing de strings de todo el sistema de tipos. El obstáculo real no es el `Rc`, son los `RefCell` de los `thread_local`, que este plan elimina de todos modos.

### L6 — Superficie LSP

**Invariante:** las features son funciones puras `fn(&Analysis, FilePos) -> Option<T>`. Sin acceso al workspace, sin I/O.

Hoy `references`, `implementation` y `code_lens` reciben `&Workspace` por parámetro, lo que las hace intesteables sin levantar el servidor.

Protocolo a completar: sync incremental, `completionItem/resolve`, pull diagnostics, deltas de semantic tokens, `didChangeWatchedFiles` (el cliente ya crea el watcher y el servidor no implementa el handler), `didChangeConfiguration`, `$/progress` para el indexado, y `workspaceFolders` en lugar del `std::env::set_current_dir` global de `backend.rs:134`.

### L7 — Extensión

**Invariante:** la extensión no hace nada que el servidor pueda hacer.

- TypeScript + esbuild. Hoy es JS plano sin bundler, lint ni tests.
- Eliminar el CodeLens client-side (`providers/codelens.js`): duplica el que ya anuncia el servidor, y el usuario ve dos juegos de lentes.
- Eliminar el hot-reload de `.js` (`manager.js:310-356`): activo por defecto en producción, mantiene dos watchers de FS vivos, y ni siquiera funciona — `restart()` no purga la caché de `require`.
- La vista de AST pasa a ser un custom request al servidor, no un spawn de `vn`.
- Sacar del repo `syntaxes/vn.tmLanguage.full.backup.json` (5762 líneas) y `varn-language-0.3.0.vsix`.

---

## 6. Plan de migración

| # | Fase | Crates | Escala |
|---|---|---|---|
| 0 ✅ | Parser total (`Missing` / `Error`) | core, parser, checker, compiler | media |
| 1 ✅ | Lexer total (trivia) | core, lexer, lsp | media |
| 2 ✅ | **Checker sin estado** (`ImportResolver`) | checker, pipeline, cli, lsp | **grande** |
| 3 | Indexado por `AstId`, fin del offset-keying | checker, lsp | media |
| 4 ⚠️ | Invalidación dirigida hecha; motor de queries aplazado por medición | lsp | media |
| 5 | Borrar el modelo paralelo; proyectar de `CheckResult` | lsp | grande, casi todo borrado |
| 6 | Actor ✅ + superficie del protocolo | lsp | media |
| 7 | Extensión en TypeScript | varn-extension | media |

Orden obligatorio: **0 antes que 5**. Sin nodos de error, la completion no puede ser checker-driven y el modelo paralelo no se puede borrar.

Las fases 0-3 tocan el compilador y exigen la matriz de validación completa. Las 4-7 solo tocan `varn-lsp`, que la matriz no cubre; requieren una suite propia (§7).

---

## 7. Validación

### Fases que tocan el compilador (0-3)

Matriz de cuatro, toda verde, según `<validation>` de `CLAUDE.md`:

```
cargo run --release --bin vn -- bench ./tests/main.vn -v
cargo run --release --bin vn -- run  ./tests/main.vn
```

cruzado con `VARN_NO_JIT=1` y con las dos procedencias de la stdlib (árbol `std/` y `VARN_STD=@embedded`). Purgar `vn cache clean` al cambiar de procedencia.

### Fases que solo tocan el LSP (4-7)

`tests/main.vn` no ejerce el servidor. Hace falta una suite de fixtures con aserciones sobre posiciones concretas — hover, goto definition, completion, semantic tokens. Los tests actuales en `crates/varn-lsp/tests/` son el embrión.

Requisito específico de la fase 0: fixtures de **código incompleto** (`foo.`, `const x = `, llave sin cerrar) que hoy no tienen ninguna cobertura y son el caso de uso dominante de un LSP.

---

## 8. Inventario de eliminación

Código que este plan borra sin reemplazo directo:

| Ruta | Líneas | Motivo |
|---|---|---|
| `lsp/src/pipeline/` | ~460 | modelo paralelo; se proyecta de `CheckResult` |
| ~~`lsp/src/document/receiver_ast.rs`~~ | 365 | ✅ borrado — sustituido por `member_resolutions` |
| `lsp/src/document/chain_queries.rs` | 350 | heurística de tres capas sobre strings |
| ~~`lsp/src/util/intrinsic_members.rs`~~ | 153 | ✅ borrado — los miembros vienen del checker |
| `lsp/src/queries/indexes.rs` | 41 | reemplazado por `SpatialIndex`; hoy se construye y nunca se consulta |
| `lsp/src/workspace/revision.rs::Cached` | — | nunca se construye |
| `SymbolRecord` / `MemberRecord` / `global_key` | — | modelo paralelo |
| `unsafe impl Send/Sync for DocumentState` | 2 | UB |
| `CheckResult::expr_types` | — | proyección posicional del `expr_table` |
| `varn_core::ast::assign_ast_ids` | 1 | cuerpo vacío |
| `Expr::new_with_range` | 3 | centinela `id: 0` |
| 3 bloques `thread_local!` del checker | — | estado global |
| `extension/src/providers/codelens.js` | 43 | duplica el CodeLens del servidor |
| hot-reload en `extension/src/manager.js` | ~50 | hack de desarrollo en producción |
| `extension/syntaxes/*.backup.json` | 5762 | deuda muerta versionada |

Estimación: `varn-lsp` pasa de ~7 500 líneas a menos de la mitad. Casi toda su complejidad actual es reconciliación entre dos modelos que no deberían ser dos.
