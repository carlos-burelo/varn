# Plan: eliminar sistemas duales antes de la release pública (living, 2026-10-23)

Origen: auditoría de código (Ley 12) del 2026-10-23. Cada item declara
ganancia, verificación y borrado (Ley 10). Un commit atómico por item (Ley 9).
Orden: capa más baja primero (Ley 14). Validación por item:
`.\scripts\verify.ps1 -Quick`; matriz completa (`-Fast`) antes de cerrar cada fase.

## Fase A — cimientos del compilador

### A1. Borrar `HirType`; SSA consume `BackendTy`
- Hallazgo: `varn-compiler/src/hir` define un segundo sistema de tipos
  (`HirType`, `TyId`, `ClassId`, `TyTable`) y `from_tir/ty.rs` lo degrada:
  `Decimal/BigInt/Void/Never/Dynamic → Dynamic`, `Char/Bytes/Enum/Fn → Ref`.
  `class_id` deduplica por nombre sin origen (Ley 16).
- Ganancia: un tipo del checker llega entero a SSA/regalloc/JIT; desaparece la
  clase de bugs "tipo re-derivado"; identidad de clase deja de ser por nombre.
- Verificación: `tests/main.vn` 4 cuadrantes sin cambios de resultado; bytecode
  determinista (dos corridas idénticas); `vn debug -p bails` no empeora.
- Se borra: `hir/mod.rs`, `from_tir/ty.rs`, `TyTable` duplicada, las
  conversiones `lower::*` que dependan de `HirType`.

### A2. Vaciar `DynReason::NotYetSupported`
- Hallazgo: 79 sitios (casi todos en `varn-emit`) degradan un tipo conocido.
- Ganancia: menos `Dynamic`, menos guards `verify_at_runtime`, menos rutas por
  nombre en VM/JIT.
- Verificación: `Coverage::of` sobre `tests/` y `std/` antes/después; la cuenta
  de `NotYetSupported` llega a 0 y la variante se borra.
- Se borra: la variante y los sitios; cada sitio pasa a su tipo real o a una
  razón con nombre (`HostBoundary`, `Union`, …).
- Línea base (2026-10-23, `vn debug -p tir:check` sobre `tests/*.vn`):
  nodos dinámicos por razón: función 9491, `NotYetSupported` 1634, task 1327,
  declared 1105, object 447, union 294, generator 89.
- Sitios más calientes (llamadas a `NotYetSupported` por `vn check tests/*.vn`):
  `ty/lower.rs:23` (`opaque()`), `body/scope.rs:46-47`, `module_globals.rs:109`,
  `body/stmt_decl.rs:258`, `body/expr_closure.rs:28,37,167`, `functions.rs:216-217`.
- Causas dentro de `opaque()`: parámetros de tipo genéricos (`T`,`A`,`B`: ~2200),
  interfaces y alias no objeto (`HasId`, `Printable`, `Options`, `Profile`, `Link`),
  y tipos de nivel de tipo sin resolver (`This`, `Intersection`, `TemplateLiteral`).
- Orden: (1) razón propia para parámetros de tipo borrados; (2) alias/interfaces
  no objeto se bajan por su destino con guarda de recursión; (3) defaults de
  `scope.rs`/`closure` se sustituyen por el tipo del checker; (4) borrar la variante.
- Hecho: parámetros de tipo borrados llevan `DynReason::Generic` (NYS 1634 -> 1438;
  type parameter 196). El resto de NYS son fallbacks por falta de dato del checker:
  `expr_ty` sin entrada en `expr_table`, símbolos global sin `ty`, `this` fuera de clase,
  y defaults de `scope.rs` que luego se sobrescriben. Raíz: el checker no registra el
  tipo de toda expresión; arreglarlo ahí, no en emit (Ley 14).
- Hecho (2026-10-23): funciones anidadas, closures prebound y resultados de
  decoradores llevan su tipo; los bindings sin tipo de origen leen el tipo del símbolo
  del checker (`declared_types`, indexado por offset de declaración). NYS 1438 -> 1321.
- Hallazgo de raíz: las clases core (`Error`, `RangeError`, ...) no tienen
  `BackendTy`: `resolve_type_ref` no las encuentra porque `collect_foreign_classes`
  excluye builtins, así que `catch (e)` y todo valor `Error` caen en NYS. Siguiente
  paso real de A2: representar las clases core en la tabla de clases de TIR.
- Trampa medida: `guard_dynamic` sobre el valor de `for await (const msg of rx)`
  cuelga `tests/47`; el binding tipado va sin guarda.
- Hecho (2da tanda): `collect_foreign_classes/enums` se siembran con todo tipo
  alcanzable (expr, símbolos, anotaciones), así `catch (e)` es `Class(Error)`;
  parámetros con default/desestructurados llevan el tipo de la firma; el valor
  capturado crudo es `Dynamic(Runtime)`; `null` y los argumentos ausentes usan
  `NULL_TY = Nullable(Never)`. NYS en `tests/*.vn`: 1634 -> 863.
- Perfil de lo que queda (por nodo TIR): Var Local ~295 (bindings de payload de
  variantes, `match_pattern`), Var global/native ~147 (slots de funciones de namespace
  sin tipo: `Json::parse`, `Task::all`), Call/MethodCall ~160, Index 54, Yield 52.
- Análisis de diseño (2026-10-23): `Fn(SigId)` en `lower_type` NO es el cimiento
  correcto por ahora. Medido: 0 accesos por nombre vienen de `Dynamic(Callable)`; el
  sitio de llamada ya obtiene la firma del checker, así que no hay doble derivación
  que pague. Además `BackendTy` no tiene subtipado: `Fn(sig)` por igualdad de SigId
  rompería asignaciones compatibles (variancia de parámetros) en el verificador.
  Solo se justifica si un consumidor del backend usa la firma del valor.
- Medido en cambio: 5807 resoluciones `ByName` en TIR (de 66061 nodos; 37% estático).
  Por receptor: Array/Str/Bytes/Map ~900 (miembros core que el compilador baja a
  ArrayLength/StrLength, el nombre es engañoso), `Dynamic(NYS)` 420, `Dynamic(Shape)`
  277+44, `Class` 392 (casi todo getters/setters: `value`, `size`, `current`,
  `doubled`, `celsius`; los accesores no tienen slot estático en `ClassInfo`),
  `Enum` 234, `Dynamic(Declared)` 118.
- HECHO (accesores): `ClassObj` tenía tres tripletas paralelas (método/getter/setter x
  mapa/vtable/dueños); ahora hay una sola tabla de miembros con clave canónica
  `AccessorKind::key` (varn-core). Las lecturas y escrituras de accesores de una clase
  conocida se resuelven estáticamente como llamadas al accesor. Los accesores salían con
  `fresh_sig` (firma toda NYS, retorno boxed): ahora usan la firma construida desde el
  tipo de la propiedad. Medido (quick, JIT, 20M iteraciones): lectura de getter
  1880 ms -> 130 ms; escritura por setter 2064 ms -> 149 ms; método plano 130-160 ms.
  Antes el getter caía en un `invoke` reentrante del helper JIT (`get_property_value`).
- Cerrado también: accesores de clases foráneas (`ForeignClass.accessors`; get 143 ms,
  set 220 ms por 20M, una subclase local de una foránea 131 ms), accesores y métodos de
  enums (`EnumInfo.members`, firmas del checker, el verificador acepta ranura de vtable
  en receptor enum; getter 733 -> 250 ms por 5M) y getter bajo `?.` (ya resolvía
  estático).
- Abierto, distinto de los accesores: llamar un método de enum cuesta ~50 ns frente a
  ~7 ns de una clase. Diferido; análisis completo en E1 (al final de este documento).
- Candidatos a cimiento, en orden: (1) accesores de clase (getter/setter) con slot
  estático en la tabla de métodos; (2) formas (`Shape`) con layout fijo; (3) cola de
  receptores NYS.
- Cimiento mayor pendiente: `lower_kind` baja todo `TypeKind::Fn` a `Dynamic(Callable)`
  (9609 nodos) aunque `BackendTy::Fn(SigId)` existe; el sitio de llamada vuelve a
  derivar la firma (`callee_signature`) del checker. Requiere que el internador de
  firmas viva en `TyTable` para que `lower_type` pueda producir `Fn(SigId)` (Ley 6).
- Aparte: `Dynamic(Callable)` aparece 9491 veces; un `Fn(SigId)` estático es el
  mayor candidato a ganancia de rendimiento tras A2.

- HECHO (3ra tanda, 2026-10-23, `tests/*.vn` con `vn debug -p tir:check`): NYS dinámicos
  863 -> 0 y NYS por nombre 5583 -> 0; la variante se llama ahora `DynReason::Unrecorded`
  (el checker no registró tipo para el sitio) y queda como alarma con cero aciertos en el
  corpus. Cimientos corregidos, uno por commit:
  - `Range<T>` -> `Dynamic(Range(T))`; `TaskHandle<T>` -> `Dynamic(TaskHandle(T))`.
  - Alias no objeto se bajan por su destino (guarda `enter_alias`); tipos de nivel de tipo
    (`TypePredicate` = Bool, `TemplateLiteral`/`KeyOf` = Str, resto = Generic).
  - Nombre sin representación (interfaces, intersecciones) = `Dynamic(Interface)`, nombrado
    y medible (232 nodos): el cimiento real es dar layout/itable a las interfaces.
  - Enums foráneos: alcanzabilidad transitiva (firmas, objetos, miembros de clases),
    payloads con sus tipos y parámetros de tipo (`Option`/`Result` -> `Generic`), miembros
    (`ForeignEnumMember`) y llamadas a miembros por ranura.
  - Clases foráneas: métodos con firma del checker y `statics` con el mismo layout que las
    locales (`build_statics`); las llamadas estáticas a clases importadas resuelven.
  - `TirExprKind::Length`: el backend ya no elige `ArrayLength`/`StrLength`/`BytesLength` por
    el nombre del miembro (Ley 6/16); `a.length` sobre Array/Tuple/Str/Bytes es un nodo.
  - Constantes de variante `Enum.Variant` cargan su slot global (`Resolution::EnumVariant`
    sobre `Field`); `this` tipado en extensiones; campos tipados al desestructurar objetos.
  - Razones por nombre: `CoreMember` (miembro de tipo builtin servido por la tabla core,
    fuera del registro de ops: `Map.size`, `Bytes.alloc`, `Int.parse`, `slice`, `rawValue`) y
    `Spread` (llamada con spread, aridad solo en runtime). Son la cola real de C1.
  Gotcha: `DynReason::Runtime` es `SlotKind::Ref` (referencia a heap segura), no "valor
  crudo"; usarlo para resultados que pueden ser enteros corrompe. Lo desconocido es
  `Declared` (conservador: se verifica en runtime).
- Abierto en A2: extensiones sobre tipos no clase siguen con firma `fresh_sig` toda
  `Unrecorded` (falta `this_ty` en `TirFunction`); funciones sin tipo de checker y
  placeholders de closures usan `Unrecorded` por construcción.

### A3. Inferencia solo en el checker
- Experimento (2026-10-23): anular `infer_expr_type_self` rompe la std en el primer
  `extends` (`StdError` no resuelve a `Error`): el binder usa su inferencia como
  resolución de nombre→tipo para herencia, tipos provisionales de `const`, arrays
  evolutivos y namespaces. No es solo duplicación: el checker además reutiliza
  `numeric_operand`, `adopt_literal_operands`, `widen_literal` del binder.
- Paso seguro, a hacer primero: mover esos helpers puros de álgebra de tipos a
  `varn-sem` (capa de tipos), de modo que el checker deje de depender del binder
  para ellos. Segundo: cortar la inferencia del binder a lo que es resolución de
  nombres (identificador/miembro para herencia) y que el checker infiera el resto.
- CORRECCIÓN (2026-10-23): la evolución de arrays que el "Experimento 2" creía perdida
  estaba muerta de punta a punta: `TypeEntry.refined` (que `refine()` llenaba desde
  `evolved_array_types`) no lo leía ningún consumidor. Borrada (`array_evolve.rs`,
  `decls_array.rs`, `refine.rs`, `refined`, `evolved_array_types`): `check:types` y
  `typeloss` idénticos en los 250 tests. Efecto real: `let a = []` + `push(1)` sigue
  siendo `dynamic[]`; la inferencia de elemento de arrays vacíos NO existe y es deuda de
  cimiento (Ley 14), a diseñar en el checker con flujo, no con un side-channel.
- Lo que el recorte de la inferencia del binder sí cambia (medido con `check:types`,
  6 tests): (1) miembros `const` de un namespace (`Rates.bonus` pasa de `int` a
  `dynamic`), (2) `const f = (n: int) => ... f(n-1)`: el tipo de la flecha recursiva
  (`fib` en tests/37), (3) campos de objetos literales en `ui-declarative`/`ui-reactive`.
  Esos tres son los consumidores reales que el checker debe cubrir antes de cortar.
- Medido (paso b, 2026-10-23): el tipo provisional del binder gana al del checker:
  `stages.rs` solo reemplaza `sym.ty` si el del binder es débil (`dynamic`). Invertir esa
  precedencia no cambia nada observable (`check:types`/`typeloss` idénticos en 250 tests),
  y recortar la inferencia del binder deja las mismas 6 diferencias de arriba: mientras el
  checker revisa, los identificadores leen `sym.ty` del binder, así que el tipo del checker
  llega tarde. Para cortar de verdad hace falta tipado de símbolos bajo demanda en el
  checker (tipar la declaración al primer uso, con detección de ciclos para la flecha
  recursiva `const fib = (n: int) => ... fib(n-1)`), y que `ModuleInterface` exporte el tipo
  final (namespaces entre módulos, tests 202). Es un rediseño del checker, no un recorte.
- Medido (2026-10-24): cortar solo la inferencia de inicializadores de `const`/`let` en el
  binder cambia 10 de 249 tests en `check:types`, y la causa dominante no es el checker sino
  la frontera de módulo: `ModuleResolver::module_bind`/`stdlib_bind` solo parsean y bindean
  las dependencias, nunca las tipan, así que un import (`Path.join`, `None`, miembros de
  namespace) lee el tipo provisional del binder de la dependencia. `varn-resolver` no depende
  de `varn-checker` (el checker está arriba). Cortar de verdad exige: (1) un trait
  `ModuleChecker` inyectado en el resolver (Ley 1) que tipe cada dependencia una vez, en
  orden topológico, con ciclos de import resueltos explícitamente; (2) cachear el bind ya
  tipado (`try_load_cache`/`save_to_cache`) para que el costo sea una vez por módulo;
  (3) recién entonces borrar `infer_expr_type_self` en `declarations.rs`, `object_members.rs`,
  `namespace_struct.rs`, `patterns_sum.rs`, `binder_stmts.rs`, `binding_types.rs`. Hecho:
  `extends` ya solo resuelve nombres (`heritage_type`, 6812a92c).
- HECHO (2026-10-24): `bind_and_cache` tipa cada dependencia con el checker y reemplaza el
  bind provisional (`replace_bind`; `insert_bind` es `or_insert` y lo dejaba vivo); los
  miembros `const` de namespace se asientan con `settle_members` (finalize y `BindView::settled`
  para usos dentro del mismo archivo); las `const f = (n: int): int => ...` anotadas se tipan antes del
  cuerpo (`declare_annotated_function`); `std:path` ya no sombrea sus imports. El binder ya no
  infiere inicializadores de `const`/`let`, miembros de namespace ni patrones con default:
  `check:types` idéntico en 249 tests y la suite 3798/0. Quedan en el binder:
  `object_members.rs` (campos de literales: decide Method/Property por el tipo inferido) y
  `binding_types.rs` (default de parámetro). Cortar este último pierde el error de
  `f("s")` entre módulos porque la firma de una función exportada se arma en el binder y
  `finalize` no la reconstruye con los parámetros que tipó el checker: pendiente.
- HECHO (paso a, 7abf324d): resolución de tipos e inferencia de expresiones viven en
  `varn-sem` (`resolve/`, `infer/`); binder y checker los importan de la capa baja. Matriz
  quick 3798/0. Falta (b) y (c).
- Hallazgo: binder (`type_infer_*`) y checker (`infer_*`) infieren expresiones.
- Ganancia: un motor; el binder deja de conocer tipos semánticos (AGENTS §3).
- Verificación: suite 4 cuadrantes; diagnósticos del LSP sin regresión.
- Se borra: `type_infer_expr/ops/member`, `inference_utils` (lo que sobreviva
  se muda al checker).

## Fase B — módulos

### B1. Un solo cargador de módulos
- Hallazgo: `varn-resolver` + `varn-modules` + `varn-pipeline/resolver.rs`
  (`Session`) + `stdlib_loader/layer_link/portable`.
- Ganancia: una identidad, una resolución, una carga, una caché (Ley 8).
- Verificación: 4 cuadrantes con `VARN_CACHE_DIR` limpio.
- Se borra: `Session`, `resolver_embed/disk` duplicados, caminos paralelos.

## Fase C — runtime

### C1. Rutas dinámicas por nombre
- Tras A1/A2, medir cuáles de `GetProperty`, `LoadGlobal`, `index_instance_fallback`,
  `fallback_method_call` siguen alcanzables (bails/tiers) y borrar las muertas.
  Las que sobrevivan se justifican como `dynamic` declarado (Ley 17).
- Medido (2026-10-24, `tir:check` sobre `tests/*.vn`, nodos con `Resolution::ByName`): 617 en
  total. `interface` 198 (lecturas de campo: `kind`, `text`, `x`, `w`, `name`), `object type`
  161 (campos de tipos de objeto anónimos), `core member` 111 (`size` 26, `rawValue` 16, `slice`
  9, `alloc` 7, `fromString` 6, `parse`, `fromCode`, estáticos de `Int`/`Bytes`/`Char`),
  `declared` 107, `range` 35, `spread` 3, `union` 2. Los métodos de tipos core ya van por
  `core_ops` (`NativeOp`); lo que queda son propiedades `readonly` de clases core (no tienen op)
  y estáticos. Cada familia exige un mecanismo propio: itable/offset por interfaz (198),
  shape fija para tipos de objeto (161), opcodes tipados para propiedades core (111).
  Arreglado de paso: `coverage.rs` desbordaba `by_name` con las razones nuevas.
- Costo real (2026-10-24, perfil `quick`, bucle de lecturas con JIT): campo por tipo de clase
  200M lecturas 0.25 s; campo por tipo de interfaz 200M 6.6 s (26x, ~33 ns por lectura, igual
  con dos clases polimórficas); campo de tipo de objeto anónimo 100M 0.23 s (ya rápido);
  `.length` de array 100M 0.18 s contra `Map.size` 100M 7.0 s (40x). Conclusión: los dos
  huecos con ganancia medible son campos vía interfaz (198 nodos) y propiedades `readonly` de
  Map/Set; los tipos de objeto no son un problema. `Length` ya tiene SSA/opcode propios, pero un
  opcode SSA nuevo toca ~70 archivos (pases, JIT, VM, portable); el camino barato para
  `size` es un native op `Map.size`/`Set.size` en el contrato (hoy es `readonly size: int`,
  sin op) y que `lower_length` lo emita como `CallNativeOp`.

## Fase D — higiene y release

### D1. Flag muerto `FRAME_LAYOUT_V2_JIT_BAIL`
- Ganancia: menos ramas muertas en el camino de llamada. Se borra el const y sus ramas.

### D2. Modelo de permisos único en `vn run`
- Hallazgo: sin flags = `allow_all`; con un `--allow-*` pasa a restrictivo.
- Decisión del dueño requerida (cambia el comportamiento por defecto).

### D3. Tests de cargo (Ley 11)
- Mover lo que valga a `tests/*.vn`; borrar `crates/*/tests`.

### D4. Dogfooding de usabilidad
- Escribir 5 programas `.vn` reales; registrar fricciones; priorizar.

## Estado
- [x] D1  - [x] A1  - [x] A2 (corpus en 0 `Unrecorded`; cola: extensiones, interfaces, `CoreMember`)
- [~] A3 (helpers movidos a varn-sem, widen_literal borrado; falta cortar la inferencia
  del binder a resolución de nombres)
- [x] B1 (Session borrado, DiskResolver -> ModuleResolver; los tres caminos de
  parse/bind/collect comparten parse_and_cache/bind_and_cache/collect, no se funden más:
  separar bind de exports evita recursión en ciclos de imports)
- [~] C1 medido: GetProperty 5142, Eq 3512, CallMethod 420 genéricos en `tests/`;
  nacen de objetos con forma (`Shape`), valores `Error` y uniones. No se pueden borrar
  las rutas por nombre hasta que formas/uniones/clases core tengan tipo estático
- [ ] D2 (decisión del dueño)  - [ ] D3 (decisión del dueño)  - [ ] D4
- [ ] E1 (diferido por decisión del dueño, 2026-10-23): layout plano de variantes de enum

## E1 — Layout plano de variantes de enum (DIFERIDO)

Estado: **no se hará por ahora** (decisión del dueño, 2026-10-23). Este apartado deja el
análisis completo para retomarlo sin repetir la investigación.

### Síntoma
Llamar un método de un enum cuesta unos 50 ns frente a unos 7 ns de un método de clase,
y leer un getter de enum cuesta lo mismo que llamar un método de enum.

Repro (modo `quick`, JIT, 5 000 000 iteraciones por bucle):

```
import { now } from "std:time";
enum Color {
  Red, Green;
  get code(): int { return 7 }
  twice(): int { return 14 }
}
function runGet(c: Color, n: int): int {
  let s = 0
  for (let i = 0; i < n; i++) { s += c.code }
  return s
}
function runMethod(c: Color, n: int): int {
  let s = 0
  for (let i = 0; i < n; i++) { s += c.twice() }
  return s
}
```

| Medida | Resultado |
|---|---|
| getter de enum, 5M | 250–259 ms (≈ 50 ns por lectura) |
| método de enum, 5M | 243–264 ms (≈ 50 ns por llamada) |
| método de clase, 20M (referencia) | 130–160 ms (≈ 7 ns por llamada) |

Los miembros de enum van tras `;` en la lista de variantes (`Red, Green; get code() ...`).

### Causa (verificada con trazas, no supuesta)
1. **La caché inline del intérprete acierta.** Una traza temporal en `call_method`
   (`crates/varn-vm/src/exec/dispatch/reg_ops/method_calls/mod.rs`) mostró, para
   receptores enum: clase resuelta (`get_class` usa `EnumVariantData.enum_class_id`), IC
   usable y `probe` con acierto en 4 999 999 de 5 000 000 llamadas.
2. **El JIT nunca llega a esa caché.** El guard inline que genera
   `crates/varn-jit/src/clif/from_ssa/call/method.rs` comprueba que la celda del receptor
   sea `CELL_KIND_INSTANCE` y lee el id de clase en
   `INST_CELL_DATA_OFF + INST_CLASS_ID_OFF`. Cualquier otro tipo de celda salta
   directamente al bloque `slow`.
3. **Una variante de enum no es una celda plana.** Es `HeapObj::EnumVariant(EnumVariantData)`
   (`crates/varn-types/src/value/mod.rs`) con `enum_class_id: Option<u32>`,
   `def: Arc<VariantDef>` y `slots: Vec<VmValue>`: indirecciones y un `Vec`, sin id de
   clase en una posición fija de la celda.
4. **Resultado.** Cada llamada pasa por `jit_call_method_window` y de ahí por
   `call_method`, que antes de consultar la IC decodifica el nombre del método, busca una
   plantilla de variante (`enum_variant_template`) y prueba intrínsecos
   (`intrinsic_method`). Esos ~43 ns extra son la diferencia con una clase.

### Intento descartado
Se amplió `jit_call_method_cached_window`
(`crates/varn-vm/src/exec/jit_helpers/calls.rs`) para aceptar `EnumVariant` además de
`instance`. No cambió nada (259/253 ms) y se revirtió: ese helper solo se alcanza después
del guard inline, que ya había rechazado la celda. Un helper intermedio en el bloque
`slow` recortaría una parte de los 43 ns pero no llegaría al nivel de una clase, y añadiría
un tercer camino de llamada (Ley 8).

### Diseño propuesto (no implementado)
Dar a la variante de enum una celda plana con el mismo prefijo que una instancia:

- Nuevo tipo de celda para variantes, con el id de clase del enum en el **mismo offset**
  que `INST_CLASS_ID_OFF`, el tag de variante y los slots de payload inline.
- El guard del JIT acepta ambos tipos de celda y lee el id de clase de la misma posición;
  el resto de la rutina (búsqueda en IC, comparación de versión de vtable, llamada directa
  por vtable) no cambia.
- `EnumVariantData` deja de ser el formato canónico: el `Arc<VariantDef>` pasa a ser una
  tabla por clase de enum, no por valor.

Alternativa: representar la variante como una instancia de la clase del enum con un campo
de tag. Reutiliza todo el camino de instancias, pero cambia la identidad y la igualdad de
variantes y obliga a revisar la comparación estructural.

### Ley 10 (ganancia, verificación, borrado)
- **Ganancia esperada:** acercar la llamada a método de enum a la de clase (de ≈ 50 ns a
  ≈ 7–10 ns) y, de paso, la lectura de getters de enum. Sin medir en cargas reales: el
  coste es de 43 ns extra por llamada, y no hay todavía un benchmark de aplicación que
  muestre enums en un bucle caliente.
- **Verificación:** el repro de arriba debe quedar a menos de 1,5× el método de clase;
  `tests/16-enums.vn`, `tests/136-enum-identity.vn`, `tests/175-enum-instanceof.vn` y
  `tests/189-enum-variant-slots.vn` sin cambios; matriz completa de 4 cuadrantes más
  `VARN_JIT_CACHE=verify`; comparación de tiers (`vn run --compare-tiers`).
- **Se borraría (previsiblemente):** la rama de enum en `get_class`
  (`crates/varn-vm/src/exec/props/intrinsic.rs`) que resuelve la clase por
  `enum_class_id`, y el pre-chequeo `enum_variant_template` de `call_method` para nombres
  que no son constructores de variante.

### Superficie afectada (a revisar si se retoma)
- Recorrido del GC y `for_each_value_mut` sobre los slots de variante.
- Envío entre isolates: `crates/varn-types/src/value/sendable.rs` copia `enum_class_id`.
- Congelado/descongelado de módulos (`module_freeze.rs`) y clases con identidad única.
- Igualdad y comparación de variantes (`crates/varn-vm/src/exec/compare.rs`,
  `same_enum`) y `instanceof` sobre enums (`exec/advanced.rs`).
- Operaciones de payload y tag (`exec/dispatch/reg_ops/enum_ops.rs`, `GetEnumTag`,
  `VariantPayload`) y su lowering en el JIT.
- Serialización de enums en la std (JSON, formatos binarios).

### Criterios para reabrirlo
- Un benchmark o una aplicación real (máquinas de estado, UI, parsers) muestra llamadas a
  métodos de enum en un bucle caliente.
- Se emprende otro cambio de layout de instancias (punteros directos, etapa 3) y conviene
  hacerlo a la vez para no migrar dos veces.
- Se decide que `Option`/`Result`, que son enums, necesitan métodos baratos en la std.

