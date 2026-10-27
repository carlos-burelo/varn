# Plan único (living, 2026-10-16)

Sustituye a `2026-10-09-plan-unificado.md`,
`2026-10-09-representacion-un-bloque-por-valor.md`,
`2026-10-10-propagacion-de-tipos.md`, `2026-10-12-plan-b1-resume-jit.md`
y `2026-10-12-try-catch-jit-puro.md` (eliminados; lo pendiente vive
aquí, lo hecho queda en git). Memoria viva: actualizar con cada item
verde. Si contradice al código, manda el código (Ley 12: las cifras
las produce la auditoría, no se copian).

> Validación: `.\scripts\verify.ps1 -Fast` (matriz 4 cuadrantes,
> release); iteración `.\scripts\verify.ps1 -Quick`; sin JIT
> `VARN_NO_JIT=1 vn run tests/main.vn`.

## 0. Reglas

- Cada item declara (a) ganancia, (b) medida, (c) qué se borra (Ley 10).
- Bench pareado mismo perfil + checksum antes que tiempo; `quick`
  para iterar, `release` para números contra rivales (Ley 13).
- Un commit atómico por item, en `main` (Ley 9). Sin re-litigar
  lo cerrado (§4).
- Un item sin ganancia medida se aparca, no se insiste.

## 1. Cerrado (log compacto; detalle en git)

Soundness (2026-10-25): VN4002 al redeclarar un parámetro conserva
el rango del declarador desde el hoisting; ya no señala todo el cuerpo.
Exhaustividad: `wildcard_enum_match_arm` pasa de aviso a error;
checker enumera los casos restantes y el JIT elige cada shift en
el dispatch SSA, sin un segundo match que trate cualquier operación
como `IntUshr`.
Interfaces: el checker entrega formas instanciadas por identidad semántica
(origen y argumentos incluidos); se elimina el índice de propiedades por
nombre del emitter. Decode conserva miembros heredados y recursivos.
Parámetros abiertos (2026-10-26): conversiones internas requieren prueba estática;
decode se difiere solo desde retornos síncronos directos con tipo elegido por el
llamador. Se corrigen scopes de firmas, origen de parámetros, inferencia de retornos
de closures y varianza de sumas; las variantes ausentes usan `never`. `unwrapOr<U>`
devuelve `T | U`. Signals conserva ausencia con `Option<T>`, metadata con mapas
tipados y UI con handles `State<T>`; se borra `Owner.adopt` y el almacenamiento
heterogéneo por posición. Ruptura autorizada por el dueño; consumidores migrados.

Recursión (2026-10-26): VM y JIT cuentan las llamadas sin frame contra el mismo
límite lógico de 10000; se elimina su bypass. La protección física de pila sigue
independiente. ABI JIT 11 invalida imágenes anteriores; salidas normales y suspensión
restauran la profundidad del tramo nativo. Suite de recursión y callbacks verde.

Presentación (2026-10-26): floats enteros muestran `.0`; strings y chars
anidados conservan sus delimitadores. Se elimina el escape JSON separado: una
función de core escribe cadenas entre comillas y escapa todos los controles C0.
Consumidores de snapshots de la suite migrados, sin cambiar los datos JSON.

Contratos de valor (2026-10-26): record tiene tipo propio `#{...}`; el catálogo
nominal del checker resuelve alias e interfaces con origen y argumentos. Se
transportan formas recursivas y `readonly` sin borrarlas por profundidad. Conform
comprueba sin modificar aliases; decode construye valores nuevos. Se eliminan el
normalizador mutante y el reemplazo de argumentos; tres suites migran a decode
explícito. Retornos inferidos conservan tuplas/records y guardas de match se
comprueban después de tipar el patrón, una sola vez.

Igualdad (2026-10-26): operadores requieren tipos por valor; referencias mutables,
funciones, `dynamic` y tipos abiertos exigen comparación explícita. Se añade
`sameValue` y se elimina la invocación implícita de `equals` para esos operadores.
Once suites y cinco consumidores de std migrados; JWT estrecha su algoritmo.

JSON (2026-10-26): decode rechaza claves desconocidas de formas cerradas antes
de proyectar; ya no oculta datos descartándolos. La regla se aplica a miembros
anidados, interfaces y records; mapas y diccionarios conservan sus claves.
Dos suites que dependían del descarte migradas.

Llamadas genéricas (2026-10-26): se elimina la segunda gramática de lookahead
léxico. La ruta especulativa existente usa `parse_type_args` como única fuente
para distinguir tipos de comparaciones, igual que las llamadas de constructor.
Admite contratos inline de objetos, records y funciones; suite JSON migrada.

Conversiones SSA (2026-10-26): `ToStr` usa `ToString`; la conversión de
condición nullable produce `bool` en vez de etiquetar el valor original.
Se borran reinterpretaciones que mentían sobre los valores de strings y bools.

Retorno transaccional (2026-10-26): VM/JIT comparten finalización. La escritura
al llamador puede fallar con el frame todavía vivo; luego se libera almacenamiento.
La propagación común conserva captura y traza en lugar de indexar un frame retirado.

Resultados boxed (2026-10-26): una conversión canónica por `SlotKind` valida
valores de retorno antes de consumir payloads. La ABI nativa identifica los seis
tipos de slot, en lugar de fusionar bool/int o todos los boxed por disposición.
Se elimina la extracción sin prueba en la ruta boxed; la ruta nativa probada
conserva sus valores sin reboxear. ABI JIT 12; 4000 rechazos en caliente en la suite.

Intersecciones de records (2026-10-26): el meet directo de tipos Record mantiene
su identidad después de fusionar campos. Se elimina su normalización a Object;
el caso existente de records anidados en la suite ahora usa una intersección.

Tipos checker→JIT (sesión 2026-10-15/16): EqStr/NeqStr nativo
(`3ee1d01d`); narrowing conserva Tuple/Array (`004ce48f`);
destructuring de tupla por posición (`e8e1717d`); local `dynamic`
anotado se conserva (`8a1a06d0`); matches exhaustivos sin comodines
(`3cb9ea86`); SSO 5→8 (-20% str_ops, -55% allocs, `56f493e0`);
`BytesGetIndex` nativo (-88%, `9c311da9`) + vistas (`72029f3e`);
`BytesSetIndex` nativo (`5bc54ab4`); NYS comprueba al cruzar a
concreto (`b36bcf34`); `char<char` (`f72e14f1`); return-generador
contra lo producido (`5aea3bde`); literal foráneo sustituye
(`764deef1`); affix-inline implementado y revertido (`612f295a`,
`e6edae1a`, ver §4).
Representación: `678580d4` (upvalues en BuildObject), `1263145b`
(Map/Set en celda), `0b963319` (length SSO en chars), `4e08159d`
(tupla literal es tupla).
Unificado: C1 class_fields ~2.9× (`f91f7ee2`); C2-overflow
matrix 1.22× (`1f3a8e05`); cimientos O ajenos verdes; B0
checksums JIT/NOJIT.
Try/catch: sync+async en JIT puro, `jit_error_flag` único,
`JIT_HELPERS_VERSION = 4`, gates try-en-loop e imported-188
pendientes (§3).
Estado: suite 3522/3522 en 4 cuadrantes release; cero abandonos
reales del JIT.
Arranque y stdlib (2026-10-16): DLLs GPU/UI con delay-load (`f06ec0c6`,
`--version` 68→11 ms); huella única xxh3 (`50f70b2c`); imagen std
indexada sin copia (`f479aaca`); checker lee interfaces std+core de la
imagen (`37aecac8`, frío 92→19 ms); core ve `core:errors` (`c2f460e3`);
dev-checkout corre de imagen en caché, el árbol solo la construye
(`aeb1cff7`); interfaz canónica solo-importador (`1f264fde`, 2.25 MB→
512 KB, imagen idéntica fría/tibia). `run` vacío < 1 ms sobre
`--version`.
Caché JIT persistente (2026-10-16): lowering con relocaciones para todo
valor del proceso (`5577e184`, lowering puro sin linker) y caché de
imágenes en `<cache>/jit/` con modo `VARN_JIT_CACHE=verify` como
cuadrante 5 de `verify.ps1` (`5489ec54`). http_routing 262→227 ms
(bun 219).
Globales top-level a registros del módulo (regresión de la migración
HIR→TIR): walker TIR exhaustivo (`7a11028a`) y pase de promoción en
varn-emit (`5e652ef0`); class_fields 6.4→3.6 ms.
Módulos/LBI/ABI (2026-10-16): el macro parsea el contrato (`78a47de6`,
borra contracts.json y gen-contract-tables); build.rs con el parser real
(`9f23745a`, AST de namespace guarda `declare` `f9af54e6`); registro
estático único en lugar de secciones del linker y markers (`0cd1f048`);
permisos `@permission` en el contrato (`9394b438`); ids de op densos con
`KnownOp` reservados (`b88a69e3`); un solo resolvedor de especificadores
(`e6528ef3`, arregla doble evaluación por mayúsculas), un solo lector de
fuente (`8a5a0217`), provider con `module()` (`c546a8b5`); ABI v2 muerta
borrada (`877294fc`). `NATIVE_GLOBALS` queda como única tabla ABI.
Async/generadores (2026-10-17): shape única `{value, done}` (`5fe43f78`);
`Generator` clase nativa del contrato, `return()` funciona (`275cd974`);
clase intrínseca por índice de `RuntimeKind` (`77773a75`); nombre SSO sin
alloc en `call_method` (`ef857719`); `g.next()` baja a `CallNativeOp`
(`967e9aa1`). pingpong 3M: 706→446 ms (quick). Bugs: escritura a captura
durante el aparcamiento se perdía en `thaw` (`989d1e75`, test 223); entrada
OSR/resume leía constantes plegadas de un home nunca escrito (`6058e5b1`,
test 224); async/generador nunca es `SelfCall` (`19ab9f54`, colgaba con
recursión). Tarea perezosa aguardada arranca en el acto (`a99e5412`) y el
`await` JIT continúa sin `longjmp` si termina (`b07bc48d`): cadena await
300k 155→~74 ms (quick; bun 47).
Await como frame (2026-10-17): aguardar una tarea perezosa monta su cuerpo
como frame de la tarea que aguarda; la semántica de `await` vive en
`begin_await` (intérprete y JIT); solo una celda pendiente suspende; drivers
entregan por `resolve_awaiter`/`reject_awaiter`; `raise` único (`1b48a1cb`,
borra ctx de repuesto anidado y `reject_awaited`). `await f(x)` fusionado en
JIT monta el callee sin `LazyTask` (`26194f01`). Cadena await 300k → ~29 ms
(quick; bun ~47).
Entrada directa del callee aguardado con código JIT (`9797ad68`, 29→22 ms).
Generadores (2026-10-17): congelado/descongelado por frame en términos
relativos, frames congelados trazados completos (`e9817dcc`); un generador es
un frame congelado reanudado sobre la pila de quien llama, `yield` por
retorno, `g.next()` entra directo al código JIT (`57724659`, borra
`GeneratorDriver` y el `ExecCtx` por generador). next 136→116 ns, crear
generador ~2× más barato.
Veto de reanudación (2026-10-17): `resumes_in_interpreter` borrado, cada
hecho donde se decide (`b49792a6`): activación vetada solo en OSR,
`direct_entry` solo async/generador, `await` fusionado entra directo a todo
callee compilado. Pareado quick: try síncrono 406→259 ms, cadena await
no-hoja 165→118, recursión async 9000 74→43. Residuo en P2b.
Checker (2026-10-17, hallados al compilar los ejemplos de `docs/`): asignar a
un nombre no declarado no daba error y llegaba al verificador TIR como
`InvalidTir` (`1f274bea`); las sentencias inalcanzables solo avisaban y no se
chequeaban, pero se emitían (`94f63b09`). Corpus `tests/errors` 206/0.
Toolchain (2026-10-24, diseño y plan en `2026-10-24-toolchain-pipeline*.md`):
`Failure` tipado y exit codes en el borde (`df849f7a`); `varn-project` con un
solo `varn.toml` y una sola búsqueda de raíz (`bb302345`); `Session` dueña del
`Project` y del resolver (`ad2234b7`); `Source` (`283499b7`); `lower_module`
único (`682ce5ea`); `Session::compile` con `Compilation` por etapas, sin
`DebugSink` ni `RunOpts` (`767f7430`); `Policy` y `PermissionArgs`
(`52b456c0`); `main` delgado, features `lsp`/`dap`/`pm` fuera, `Target` y
linker en `varn-jit` (`d7510dd7`, `cdd5a247`); `--target exe` (`7aeff0c1`);
`vn task` (`0e338714`); bench sobre `Compilation` (`8426ff49`); `varn-inspect`
(`a795e15c`); LSP sobre `Session` en modo `Tooling` (`0b68dc9b`). Hallados y
corregidos en el camino: `implements` de clase genérica sin verificar
(`3ac6f3b1`), pánico del hover con miembros de clase foránea (`003aa267`),
`.vn/.env` que `vn init` creaba y nadie leía.

## 2. Pendiente, por orden

### 2.1 Representación: un bloque por valor

Cada fase: re-medir filas del probe `repr_rows.vn` antes (paso 1),
pareado ambos tiers; sin fila en bitácora no cierra. Orden lo
decide la medición, no esta lista.

- **Fase A — clave Map O(1)**: bit «internado» en `HeapStr` +
  `canonical_map_key`; borra canonicalización por interner en cada
  acceso (`heap/str.rs`, `heap/strings.rs`, `heap/map_keys.rs`).
  Prueba: ampliar test 160 (misma clave por concat/slice/literal).
- **Fase B — arrays/split un bloque**: elementos en cola de celda
  + derrame; `split_ranges` única fuente
  (`vm_value/{array,vm_array,vm_array_access}.rs`,
  `heap/cells/typed.rs`, `heap/jit.rs`). Prueba: suite nueva
  (push oltre inline, I64→Boxed, pop/shift, split vacío/multi).
- **Fase C — vistas string sin `Arc`**: vista referencia celda
  padre trazada por GC (`heap/str.rs`, `heap/children.rs`).
  Prueba: vista sobrevive a padre, realloc, GC en medio,
  `spawnIsolate`.
- **Fase D — objetos literales inline** (compuerta: `objlit3` >
  1,5× `instance3` tras A–C): JIT toma celda sin helper; borra
  `jit_build_object_with_shape`, `jit_build_record_with_shape`
  (`value/shape.rs`, `value/object/*`, `from_ssa/{classops,heap}.rs`).
- **Fase E — tipos objeto exactos como instancias** (compuerta: D):
  graduar `DynReason::Shape` a variante `BackendTy` con layout;
  decidir flujo tipado→`dynamic`→agregar-campo (prohibir o
  desoptimizar). Prueba: test 171 sin cambios + campos en loop.
- Modos de falla por fase: callback que muta Map/Set en forEach,
  crecimiento oltre inline, vista+GC, cruce isolates, shapes host.

### 2.2 B1: reanudar async/generadores en JIT

Verde si B0 + 4 cuadrantes + soak + A/B con ganancia en
`pingpong` y cadena `await`; sin ganancia se aparca. No tocar
`dispatch/mod.rs:73` (`ip == 0`), ni `direct_entry`, ni
Freeze/thaw (B2, otra sesión); tabla nueva cae en
`BUILD_FINGERPRINT`.

- **Siguiente**: P2b y P1 abajo.

#### P1 — El paso de un iterador es un par, no un objeto

**Evidencia (código, 2026-10-17).**
- Cada `next()` asigna un objeto: `make_iter_result`
  (`varn-vm/src/generator.rs`) → `alloc_object_with_shape_slice` con
  `iter_result_shape()` (TLS + clon de `Rc<Shape>`). Igual en
  `Generator.return` (`iteration.rs`), `task.rs::next_result` y
  `host_tasks.rs::iter_result`.
- `for … of` sobre un iterable no-array
  (`stmt_for_of.rs::lower_for_of_protocol`) baja a `IterInit` (símbolo de
  iterador) + `MethodCall "next"` con `Resolution::ByName` + dos `Field` por
  nombre (`done`, `value`), todo `Dynamic(NotYetSupported)`. Un `for of`
  sobre un generador **no usa `KnownOp::GeneratorNext`**: paga `call_method`
  por nombre, la asignación y dos lecturas de propiedad por nombre por
  elemento.
- El checker tipa el resultado como `{ value: T; done: bool }`
  (`iteration.vn`); el backend lo recibe como `Dynamic(Shape)` (§2.1 Fase E,
  tipos objeto exactos sin graduar).
- Medido (quick, contadores de ciclos temporales): asignar el objeto es ~25%
  de un `next` de ~116 ns.

**Causa raíz.** El protocolo se modela como "método que devuelve un objeto"
aunque el compilador conoce ambos campos y, en el caso abrumador, el objeto
muere en la misma expresión (`g.next(v).value`, `for of`). Es la mentira de
representación que combate §2.1: un hecho estático (paso = valor + bandera)
se materializa en el heap y se relee por nombre.

**Solución de raíz.**
1. *Primitivo único de paso.* `TirExprKind::IterStep { iter, input }` y op
   SSA con **dos resultados** (`value`, `done: Bool`), o un valor de paso
   descompuesto por ops puras `StepValue`/`StepDone` que el regalloc deja en
   registros. Semántica: avanzar el iterador una vez.
2. *Emisor.* (a) `for of` cuyo origen tipa `Generator<T>`/`AsyncGenerator<T>`
   (`Dynamic(Yields)`) baja a `IterStep`, sin `IterInit`/`GetSymbol`/`next`
   por nombre/`Field`. (b) `g.next(v)` cuyo resultado solo se lee por
   `.value`/`.done` (análisis local de usos en el emisor) baja a `IterStep`.
   (c) Si el resultado escapa (se guarda, devuelve o pasa), se materializa con
   **un solo** constructor `IterResult(value, done)` sobre
   `iter_result_shape`: conserva la identidad fresca observable de `next()`.
3. *Runtime.* `resume_generator`/`leave_generator` devuelven
   `Step::{Yielded(v), Returned(v)}` en vez de un objeto; el nativo
   `Generator.next` del contrato (camino dinámico) materializa con el mismo
   constructor. En JIT, `IterStep` reutiliza `jit_generator_enter/leave`: el
   valor en `jit_native_result` y `done` en un slot de salida, sin asignación.
4. *Iterables de usuario* (clases con `next()`): `IterStep` llama `next` por
   la vía normal (IC/vtable) y lee los dos campos con acceso fijo cuando el
   tipo es exacto (Fase E); el objeto lo asigna el usuario.

**Qué se borra.** `make_iter_result` del camino caliente; la rama
`lower_for_of_protocol` para generadores; las lecturas `Field` por nombre de
`done`/`value` cuando el tipo es conocido.

**Ganancia esperada (a medir).** −25% por `next`; −1 asignación por elemento
en `for of` sobre generadores, que además deja de pasar por `call_method`.

**Verificación.** Tests 20, 77, 225; `vn debug -p gc` sobre un `for of` de 1M
elementos: `alloc_count` constante respecto a N; pareado `gp.vn` y
`bench_generator.vn` contra HEAD (mínimo de 10).

**Riesgos.** Identidad cuando escapa (se materializa, se preserva); paso
final con `return` (valor del `return`, `done=true`); generadores async (paso
tras liquidar sus `await`); `return()`.

#### P2b — Los vivos de un `try` viajan por memoria, no por registros

**Evidencia (código + medida, 2026-10-17, tras `b49792a6`).** `tryveto.vn`
(3M llamadas a una función síncrona con `try`) sigue en `framed: alloc`;
el mismo cuerpo sin `try` baja `native` (~260 vs ~62 ms, quick, ruido alto).
Causa: `exceptions.rs::emit_try` derrama `live` a homes, `block.rs` vuelve a
guardar en su home cada valor de `try_live` al definirse y el landing los
recarga de homes; sin `base` eso es `NEEDS_ACTIVATION` → `Framed`.

**Causa raíz.** Un vivo del landing puede definirse dentro de la región
(`x = 1; f(); catch { x }`): el landing necesita una phi por registro. El home
hace de phi en memoria.

**Solución de raíz.** Una `Variable` de Cranelift por registro vivo en algún
landing: `def_var` donde hoy se guarda en el home, `use_var` en el landing
(el builder construye la phi), declaradas para el stack map si son heap. Se
borran el derrame de `emit_try`, el `home_store` de `try_live` en `block.rs`
y la recarga del landing; `try` deja de pedir activación.

**Verificación.** `tryveto.vn` pareado contra `notry.vn` (objetivo: mismo
orden); tests 11, 21, 154, 168; vivo redefinido en ambos brazos de un `if`
dentro del `try`; GC dentro del `try` con vivo heap.

**Riesgos.** Bloques clonados por versionado de rangos (`versions.rs`) y el
`values.to_vec()` que el landing guarda/restaura; sellado de bloques.
- **B1a**: tabla `resume_ip`→bloque en SSA portable + decidir
  params-args vs solo-homes+`live`. Verifica: dump de
  `bench_generator.vn`, sin cambio de ejecución.
- **B1b**: `resume_at(ip)` en `drive.rs` (una sola tabla con
  loop_headers, Ley 8). Verifica: `osr_entries` sube en
  `pingpong`, B0 intacto.
- **B1c**: scheduler pide entrada (`thaw`+`Delivery` con
  `Settled(Rejected)`), quitar veto `tiering.rs:144` solo
  reanudación. Verifica: tests 154+77+20/21, soak 3000 sin fugas.
- **B1d**: `CatchParam`, try/`await`, yield delegado, GC nativo,
  fuzz×GC, AOT vetado. Verifica: 4/4 + A/B final.

### 2.3 Try/catch: gates que quedan (bail a intérprete)

1. **try-en-loop** (`drive.rs`): quitarlo rompe 78
   (`EntryLivein`) y sin loop-opts rompe 179. Causa: dispatch
   compartido fuera del loop.
2. **imported-188** (parche reconocido): sin él, 188 falla por
   dispatch fantasma en catch nativo `Headers`; vía `run` pasa,
   bajo `test` no. Aislar interacción framework/tasks.

### 2.4 Rendimiento JIT (gaps vs rivales, release)

- **O1–O7, B1–B3 en otra sesión (no tocar sin coordinar)**:
  bump-alloc nursery, tax llamada alloc-free, str inline,
  tip-append, Map literal/get, Dyn-compare fast-path, bails
  Dynamic→Int, generadores/tasks/endurecimiento. Zona strings
  solapa con SSO-8/`BytesGetIndex` ya verdes arriba: coordinar
  antes de pisar.
- **C4 str_ops / C3 http_routing / C5 json**: bloqueados tras
  O-items (diseños en doc viejo; revalidar números en release).
- **C6**: re-medir csv/collection; item propio solo si gap
  >1,2× con causa. **C2-bounds/C1-hoist**: aparcados con
  medición. **`matrix_local`**: duplicado de `matrix`, dedupe
  con dueño de benches.
- No tocar: fib/dto/json_pure/gc_alloc (ganan o empatan);
  task_soak/spawn 100% intérprete hasta B1–B3.

### 2.5 Tipos: restante con causa

- **P3 enum-Eq / P4 Decimal-BigInt**: aparcados con medición
  (mismo helper, ~5ns; P4 pide SlotKind+VM+GC).
- **2.3 DynReason**: no graduar (decoración; el backend solo
  necesita estructural-o-no).
- **2.4/2.5/2.6**: layout fijo (Fase E), sin inline foráneo
  (registro global), costo try (proyecto async).
- **2.8 despacho** (6ns vs 81ns medido, 13,5×): fusión
  verify/despacho o memo con GC; sin diseño, proyecto runtime.
- **Genéricos en calls**: proyecto secuenciado (inferencia
  anidada + defaults importados + error VN3201); el error solo
  rompía 500 sitios honestos. No parche a medias.
- **Struct en namespace**: bug real con repro (check pasa,
  runtime rechaza `Geo::Point`): toca registro de shapes
  cualificados.
- **Campo-sin-init**: proyecto definite-assignment (lectura
  `null` actual como ínterin documentado).
- **Task flotante**: diferido (falta supresión + lifecycle B2).
- **Decidido sin código**: `int`→`float` variable se rechaza
  (pérdida de precisión, mensaje preciso); `this`-antes-`super`
  sound por allocate-first; `??` ya es VN3027/VN3028;
  labels+finally verificado; condicionales/mapeados y
  task-values de sus dueños.
- **Deuda declarada**: `Union`→estructural comprueba (solo NYS
  se hizo); `case` +9% sin mecanismo (ruido hasta réplica).

## 3. Validación (puerta)

`.\scripts\verify.ps1 -Fast` release 4 cuadrantes antes de
commitear; `tests/errors/*.vn` sin nuevos fallos; cada bench con
checksum antes que tiempo; A/B pareado mismo perfil, bloques
alternos, descartar primera.

## 4. Decisiones cerradas (no re-litigar)

NaN-boxing `VmValue` 8B; Map SmallVec; `EqChar/EqEnum`;
startsWith-SSO-inline (medido dos veces, cero a nivel fila);
sin-inline-foráneo; `DynReason` sin graduar; P4 bloqueado;
`matrix_local` duplicado; `new` sin deducir = VN3201 (extender
a calls exige el proyecto de §2.5, no un parche).
