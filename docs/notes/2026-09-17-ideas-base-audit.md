# Auditoría: Varn vs IDEAS_BASE.md (2026-09-17)

Mapeo punto por punto de los 50 puntos de `IDEAS_BASE.md` contra el estado real del código (no memoria, no docs previos). Cuatro sub-auditorías paralelas, evidencia con `file:línea`.

Leyenda: ✅ ya-implementado · 🟡 parcial/compromiso distinto · ❌ ausente.

## Representación de valores y objetos

| # | Punto | Estado | Evidencia / gap |
|---|---|---|---|
| 1 | Pipeline con tipos preservados end-to-end | ✅ | `varn-tir/src/ty.rs` (`TyId`/`ClassId`), `from_tir/build.rs:1728-1745` propaga `BackendTy` hasta CLIF |
| 2 | No NaN-boxing | 🟡 | `vm_value.rs:1-28`: par `tag:u64, payload:u64` (16 bytes), **no** valores nativos sin tag como pide el ideal — es un compromiso distinto, documentado y deliberado en el propio código |
| 3 | Separación valor vs objeto | 🟡 | `KIND_HEAP` referencia índice de tabla (no puntero directo todavía); ints/floats siguen ocupando el mismo VmValue de 16 bytes que un heap ref |
| 4 | Struct = valor real, Class = puntero+layout | ❌ | `StructDecl` existe en parser/checker pero compila por el mismo camino `ClassLayout`/heap que `class` — no hay paso-por-valor en registros |
| 5/6 | Layout fijo, property access → offset | ✅ | `class_layout.rs:86-152` offsets estáticos; `GetFixedField`/`SetFixedField` en VM+JIT+CSE/LICM. **Refuta memoria previa** de "herencia nativa rompe GetFixedField" — es diseño deliberado (campos heredados aplanados, `skip(inherited)`), con un workaround hardcodeado puntual para clases nativas tipo `Error` |
| 20 | TypeId no es runtime hot-path | ✅ | No aparece en `varn-vm`/`varn-jit`, solo checker/tir |
| 38/39 | Sin `undefined`, null eficiente | 🟡 | `null()` es tag explícito distinto (bien, sin colisión), pero no hay niche-optimization pointer=0 — sigue pesando 16 bytes |
| 40 | Representation selection propia | ❌ | La colocación registro/stack es la de Cranelift para marshaling de llamadas, no una política propia de Varn |

## Dispatch, generics, arrays/strings, GC, closures

| # | Punto | Estado | Evidencia / gap |
|---|---|---|---|
| 7 | Virtual solo si necesario | ✅ | `ssa/emit/values.rs:210-223`: `InvokeVirtual` si tipo de clase conocido, `CallMethod` (lookup) si no. Gap: `InvokeVirtual` sigue indexando vtable, no hay llamada 100% directa sin indirección |
| 8 | Monomorfización agresiva | ❌ | **Gap grande.** `checker_generics.rs:41-46`: type param sin inferencia → `Type::Dynamic`. No existe instanciación/especialización de funciones genéricas en `varn-compiler` |
| 9 | Arrays tipados | 🟡 | `ArrayRepr` (`vm_value.rs:472-495`) cubre primitivos homogéneos sin heap refs — bien. Pero `User[]` (clases) sigue siendo `Boxed(Vec<VmValue>)` |
| 10 | Bounds-check elimination | ✅ | `clif/vars.rs:145-157` + `clif/arrays.rs:265-270`, solo en loops JIT read-only con bound hoisteable |
| 11 | Strings byte/codepoint/grapheme | 🟡 | `str_util.rs:6-35` distingue byte vs codepoint con fast path ASCII. No hay grapheme cluster |
| 12/13 | Escape analysis + SRoA | ✅ | `passes/escape.rs` + `fixed_fields.rs`: objetos que no escapan nunca llegan a heap, DCE reenvía lecturas a argumentos originales |
| 14 | Ownership/lifetime más allá de escape local | ❌ | Solo el escape analysis intra-función de 12/13 |
| 15/16 | GC generacional + barriers selectivos | ✅ | `nursery.rs` (nursery+old-gen+remembered set), `heap/gc.rs:24-31` barrier no-op para primitivos |
| 17 | Closures como Environment eliminable | 🟡 | Diseño distinto: upvalues individuales estilo Lua (`closure.rs:18-46`), no un struct Environment único. Sin escape analysis para eliminar upvalues no-escapados |

## IR/optimizer, JIT tiers, control flow, operadores

| # | Punto | Estado | Evidencia / gap |
|---|---|---|---|
| 18/19 | SSA propio + pases (CSE/GVN/DCE/LICM/inline/devirt/range) | 🟡 | SSA real (`ssa/ir.rs`) con `const_fold`, `cse` (local, no GVN global), `dce`, `licm`, `algebraic`, `escape`, `fixed_fields`, `monomorphize` (solo containers), `redundant_guards`, `tco` (**deshabilitado**, corrompe emisión). Sin GVN global, sin range analysis, sin devirtualización de vtables reales |
| 26 | Exceptions vía unwinding, no Result-por-retorno | ✅ | Tabla `TryHandler` (`frame.rs:75-77`), camino normal sin overhead. Unwinding nativo en JIT no confirmado a fondo |
| 27 | Async como state machine | ✅ | `passes/state_machine/{mod,transform,layout}.rs` dedicado |
| 28 | Tail calls reales | ❌ | Implementado (`tco.rs`) pero **apagado** explícitamente — corrompe emisión con loops. Bug real, no solo gap |
| 29 | Match → if/else/binary-search/jump-table según densidad | ❌ | Siempre if/else secuencial (`checker/emit/body.rs:1207-1289`). `HirSwitchCase` existe pero es código muerto sin consumidor |
| 30 | Enum discriminants nativos + tagged union | ❌ | `EnumVariantData` sigue siendo heap object con `variant_tag: i64` + nombres dinámicos, no tagged union compacta |
| 33/34/35 | Baseline+optimizing tiers, profiling, deopt | 🟡 | Solo 2 niveles reales (intérprete + Cranelift), tiering por contador (`jit/tiering.rs`) con OSR. **Sin deopt/guard-failure** — no hay vuelta a intérprete tras especulación fallida |
| 36 | IC solo para dynamic | ✅ | IC usado solo en `method_calls.rs`/`set_property.rs` dinámicos; `fixed_fields.rs` resuelve estático sin IC |
| 37 | Especialización de operadores | ✅ | `lower/mod.rs`: `Add` → `AddInt`/`AddFloat`/`AddImm` según tipo estático; opcode genérico solo fallback dinámico |

## ABI/FFI, layout/SIMD, allocator/arenas, compiler data-oriented

| # | Punto | Estado | Evidencia / gap |
|---|---|---|---|
| 23/24 | RTTI opcional, ABI explícita documentada | 🟡 | `ClassLayout` siempre presente (no es reflection opcional real). ABI de facto (`NativeOpEntry`/`SignatureDescriptor` repr(C)) pero sin contrato público versionado para closures/exceptions/async |
| 25 | FFI directo con tipos nativos | 🟡/✅ | Sin `extern function` a nivel de sintaxis Varn (gap sintáctico). Llamadas nativas Rust→Rust sí evitan boxing (`raw_func_ptr`). FFI a C vía módulo host `dlopen`/`dlsym`, solo escalares, sin marshalling de structs |
| 31 | Layout controlado, repr(packed/C/native) | ❌ | `ClassLayout::from_fields` no reordena campos, no hay anotaciones `repr` en el lenguaje |
| 32 | Cache locality (AoS/SoA) | ❌ | Nada |
| 41 | SIMD intrínseco | ❌ | Nada (`f32x4` etc. no existen) |
| 42 | Allocator reemplazable en std | — | No auditado (requiere revisar `std/` del lenguaje, no Rust) |
| 43 | Arenas internas del compilador | ❌ | Sin `bumpalo`/`typed-arena`. AST (`varn-core/src/ast`) sigue con `Box<Expr>` recursivo — **inconsistente** con SSA/TIR que sí son data-oriented (`Vec<Block>`, `BlockId(u32)`) |
| 44/45 | IDs densos + interning con SymbolId comparable | 🟡 | TIR/SSA ya son ID-based. AST tiene `AstId` pero sigue usando `Box` para la relación padre-hijo (el id no reemplaza el puntero). Interning de strings da `Rc<str>` (comparación por contenido/puntero), no `SymbolId(u32)` |
| 46 | Compiler data-oriented (poco Rc/RefCell/dyn) | 🟡 | SSA/TIR limpios. **`varn-checker` es la excepción**: 334 usos de `Rc<`, 4 `RefCell<`, 1 `Box<dyn>` — el crate más alejado del ideal |
| 47 | Hash maps especializados | ✅ | `rustc-hash` (FxHashMap) en casi todo el workspace |
| 48 | Parallel compilation | ❌ | Sin `rayon`, nada paralelizado |
| 49 | Incremental compilation | 🟡 | Cache por hash de ruta+contenido (`pipeline/cache.rs`), no un grafo `ModuleId`→hash de dependencias transitivas |

## Ya alineado con el ideal, sin tocar

`#1, #5/6, #7 (parcial), #10, #12/13, #15/16, #20, #26, #27, #36, #37, #47`

## Gaps reales — candidatos a "rehacer", por impacto

**Bugs / deshabilitado (arreglar antes que rediseñar):**
- **#28 tail calls**: implementado y apagado por corrupción de emisión con loops. Es deuda técnica cerrable, no diseño nuevo.

**Gaps grandes de rendimiento/semántica (impacto directo, alineados con la premisa central de IDEAS_BASE):**
- **#8 monomorfización de generics**: hoy todo genérico sin type args explícitos cae a `Dynamic`. Es el gap que más contradice "el código estático nunca paga por dynamic".
- **#30 enum discriminants**: enums siguen siendo heap objects con metadata dinámica, no tagged unions — coste que no debería existir en un lenguaje tipado.
- **#4 struct como valor real**: `struct` existe en sintaxis pero no en semántica de ejecución — mismo camino que `class`. Bloquea el punto 4 completo de IDEAS_BASE.
- **#33/34/35 deopt**: falta el mecanismo de reversión ante especulación fallida — sin esto, cualquier especialización agresiva futura (ej. profiling de tipos) es arriesgada.

**Gaps de infraestructura del compilador (no visibles al usuario, pero condicionan todo lo demás):**
- **#43/44/45 AST con Box recursivo + `varn-checker` cargado de `Rc<>`**: el front-end no sigue el mismo patrón data-oriented que TIR/SSA. Si se va a invertir en más pases de optimización, esta inconsistencia se paga en cada uno.
- **#18/19 GVN global + range analysis**: el optimizer tiene buena base (CSE/DCE/LICM/escape) pero le faltan dos pases centrales para bounds-check elimination más agresivo y folding de rangos.

**Gaps de baja prioridad (nice-to-have, sin evidencia de que bloqueen algo hoy):**
- #29 match→jump-table, #31 repr/reorder de campos, #32 SoA, #41 SIMD, #48 parallel compilation.

## Nota sobre memoria previa

La memoria `native-inheritance-kills-fixed-fields.md` ("herencia nativa mata campos fijos, GetFixedField roto") **no se confirmó** con la evidencia actual — el diseño de campos aplanados con `skip(inherited)` es deliberado y funciona; solo hay un workaround puntual hardcodeado para clases nativas tipo `Error`. Recomendado: revisar y actualizar/retirar esa memoria.
