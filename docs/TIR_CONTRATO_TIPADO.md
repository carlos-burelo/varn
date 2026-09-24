# TIR: el contrato tipado entre el checker y el backend

Diseño aprobado el 2026-09-06, sobre `main @ 082d2ba`.

Sustituye la tabla lateral de anotaciones por un IR tipado único. Es el
primer bloque del rediseño del motor tipado (ver
`plans/2026-09-20-PLAN-PENDIENTE.md`), y el que habilita todos los demás.

---

## 1. La tesis

`AUDITORIA_TIPOS.md` §5 la enuncia así:

> El tipo no viaja **en** el programa. Viaja **al lado**. […] Cómo se ve un
> cimiento correcto aquí: el tipo es un campo obligatorio del nodo, no una
> entrada opcional de un mapa.

Este documento la ejecuta. Y añade un hecho que la auditoría original no
recoge: en `HirExpr`, el tipo no solo se omite — en varias variantes **no hay
dónde escribirlo**.

```rust
Binary { op, lhs, rhs, ty }         // tiene tipo
Member { object, name, ty }         // tiene tipo
Array(Vec<HirArrayEl>)              // NO tiene tipo
Object { properties }               // NO tiene tipo
Assign { target, value }            // NO tiene tipo
OptionalChain { object, property }  // NO tiene tipo
TryOp(Box<HirExpr>)                 // NO tiene tipo
```

`ty` es campo de *algunas variantes* del enum, no del nodo. Ninguna cantidad
de anotación arregla eso: el consumidor tiene que asumir.

A eso se suma `impl Default for Type`, que devuelve `Dynamic`. El tipo del
checker tiene un valor por defecto, así que la ausencia se fabrica sola antes
incluso de la proyección.

---

## 2. Estado medido que motiva el cambio

Sobre `main @ 082d2ba`, corpus de 107 `tests/*.vn` + 84 módulos de `std/`.

Las etiquetas `D-n` que aparecen en §10 vienen de la auditoría de estaticidad
del 2026-09-06: `D-1` es el guard de overflow del JIT, `D-2` el campo tipado
sin inicializar y `D-3` el literal `i64::MIN`. Las tres están reproducidas
sobre binario release.

### 2.1 Cobertura estática

| par tipado / genérico | tipado | genérico | tipado % |
|---|---|---|---|
| `GetFixedField` / `GetProperty` | 419 | 1037 | 29 % |
| `EqInt` / `Eq` | 575 | 666 | 46 % |
| `AddInt` / `Add` | 93 | 90 | 51 % |
| `SetFixedField` / `SetProperty` | 221 | 129 | 63 % |
| `ArrayGetIndex` / `GetIndex` | 329 | 159 | 67 % |
| `ArrayLength` / `GetProperty "length"` | 150 | 81 | 65 % |
| `LtInt` / `Lt` | 131 | 29 | 82 % |
| `InvokeVirtual` / `CallMethod` | 450 | 423 | no medible — ver §2.3 |

### 2.2 El backend no recibe tipos

`vn debug -p tiers` sobre el corpus completo:

```
5950 funciones ruteadas a CLIF   ·   gate 0   ·   bail 0
5854 con registros boxed         ·   98,4 %
```

El JIT rutea todo y casi nada en forma nativa. `has_boxed_slots` considera
escalar únicamente `Int`, `Float` y `Bool`: un `str`, un `int[]` o una
instancia de clase declarada son tan opacos como un `dynamic`.
`is_frame_aware` empieza por `proto.has_this`, de modo que ningún método
obtiene la entrada directa clif→clif.

### 2.3 Las resoluciones se re-derivan en runtime

* **Métodos.** `InvokeVirtual` se emite exactamente cuando el compilador
  conoce la clase del receptor (`ssa/emit/values.rs:184`), y pasa
  `cs = usize::MAX` como índice de call-site
  (`ops_control_calls.rs:213`). En `method_calls.rs:190` la guarda es
  `cs < cache_len`, luego el opcode mejor informado es el único **sin inline
  cache**, y resuelve el método por nombre igual que `CallMethod`.
  `ClassObj` tiene `vtable`, pero ningún opcode la indexa.
* **Globales.** El compilador emite 4027 `LoadGlobal` con el índice del
  *nombre* y cero `LoadGlobalIdx`. `globals/resolve.rs` reescribe el bytecode
  antes de ejecutar, clona los protos compartidos con `Rc::make_mut` e
  invalida el código JIT ya compilado. La causa raíz, según el comentario del
  propio archivo, es que los globales se numeran por orden de ejecución.
* **Intrínsecos.** `exec_call_method_reg` compara el nombre contra `push`,
  `pop`, `startsWith`, `endsWith` e `indexOf` antes de llegar al inline cache,
  con tres bloques de ~35 líneas duplicados por copia.
* **Campos.** Una sola sede en todo el checker
  (`record_fixed_field_layout`) publica slots, y se cierra entera si algún
  ancestro de la jerarquía es una clase nativa
  (`checker_annotations/exprs.rs:211`).

### 2.4 Sedes de anotación

| función | sedes |
|---|---|
| `record_cg_ty_at` | 27 |
| `record_slot_idx` | 5 |
| `record_numeric` | 4 |
| `record_intrinsic` | 3 |
| `record_exported_slot_idx` | 2 |
| `record_array_index` | 2 |
| `record_type_only` | 1 |
| `record_native_op` | 1 |
| `record_call_mapping` | 1 |
| `record_fixed_field_layout` | 1 |

Cada forma sintáctica que no pase por una de ellas es una desoptimización
silenciosa, sin diagnóstico.

### 2.5 Lo que el canal no puede expresar

| construcción | usos en el corpus | qué recibe el backend |
|---|---|---|
| `T?` | 55 anotaciones + 9 campos opcionales | `TypeTag::Dynamic` |
| uniones `A \| B` | 27 | `Dynamic` |
| `enum` | 16 declaraciones | `Dynamic` |
| `decimal` / `bigint` | 18 | `Dynamic` |
| `char` | 6 | `Dynamic` |
| `class` | 113 declaraciones | `Class(name)` |

### 2.6 Autoridad duplicada de layout

Cuatro sitios calculan la disposición de una clase, y producen **dos
resultados distintos**:

* `ClassLayout::from_fields` (`varn-types`) descarta `field_repr()` y usa
  `SLOT_SIZE = 16` para todo: `offset(i) = i * 16`.
* `checker_annotations/exprs.rs:215-240` sí usa `tag.field_repr()` y
  empaqueta con alineación real.
* El JIT recalcula `slot * 16` en `clif/fields.rs:191`.
* `set_property` busca el campo por nombre, con recorrido lineal.

`fixed_field_offset`, `fixed_field_tag` y `gc_mask` no tienen ningún
consumidor. La divergencia es inocua sólo porque el dato empaquetado está
muerto.

---

## 3. Decisiones tomadas

| # | Decisión | Elegido |
|---|---|---|
| 1 | Alcance del bloque | El contrato de salida del checker. No se rediseña la inferencia. |
| 2 | Cuántos sistemas de tipos quedan | Uno: `BackendTy`, compartido por todas las fases. |
| 3 | Corpus durante la migración | Rama larga, corpus rojo entre las etapas 3 y 4. |
| 4 | Qué entra en el contrato | Tipo **y** resolución, en el mismo nodo. |
| 5 | Dónde vive el nodo tipado | Un IR tipado único; `hir/` deja de existir como capa. |

Sobre la decisión 5: se evaluaron cuatro opciones.

* **A** — TIR entre checker y HIR. Descartada: deja dos IRs tipados
  consecutivos (`BackendTy` y `HirType`); desplaza la duplicación en vez de
  eliminarla.
* **B** — el checker produce HIR directamente. Descartada: conserva la forma
  de HIR, diseñada para alimentar un backend que trata todo como opaco.
* **C** — tipar el AST existente. Descartada: el parser no puede rellenar
  `ty`, así que exige o un placeholder (que reintroduce la ausencia
  silenciosa) o hacer el AST genérico sobre su estado, lo que es un refactor
  transversal de parser, LSP y debug antes de empezar.
* **D** — un solo IR tipado, `hir/` borrado. **Elegida.**

D se ejecuta como **transformación de HIR, no como reescritura desde cero**.
HIR ya empezó a migrar hacia esta tesis: `HirBinding::Global(Rc<str>,
HirType)` lleva el tipo como campo de la variante, con el comentario *"as a
field of the variant, 'no type' is not expressible by omission"*. Y
`inline/`, `ctor_summary` y `module_locals` son trabajo válido. El destino es
idéntico; la ruta ahorra unas 5 000 líneas de reescritura ciega en una rama
sin corpus verde.

---

## 4. El TIR

### 4.1 Forma del nodo

```rust
pub struct TirExpr {
    pub kind: TirExprKind,   // qué operación
    pub ty:   BackendTy,     // qué produce  — obligatorio, sin Option
    pub res:  Resolution,    // contra qué resuelve — obligatorio
    pub span: Span,
}
```

`ty` y `res` son campos del **nodo**, no de la variante. Un constructor no
puede omitirlos: no compila. `Dynamic` sigue existiendo, pero hay que
escribirlo, y por tanto se puede contar, atribuir y prohibir por sitio.

### 4.2 Las resoluciones colapsan las variantes duplicadas

Hoy `Member` y `GetFixedField` son variantes distintas de `HirExpr`, y esa
bifurcación se repite en cada capa hasta llegar a los pares de opcodes. En el
TIR hay una operación y una resolución:

```
TirExprKind::Field { object, name }
  res: Resolution::FieldSlot(3)          → GetFixedField
  res: Resolution::ByName { .. }         → GetProperty

TirExprKind::MethodCall { recv, name, args }
  res: Resolution::VtableSlot(7)         → InvokeVirtual con índice
  res: Resolution::Intrinsic(ARRAY_PUSH) → CallIntrinsic
  res: Resolution::DirectFn(f)           → llamada directa
  res: Resolution::ByName { .. }         → CallMethod + inline cache

TirExprKind::Var { .. }
  res: Resolution::GlobalSlot(12)        → LoadGlobalIdx, sin pase de reescritura
```

`InvokeVirtual` sin vtable (§2.3), globales por nombre (§2.3), los `strcmp`
del dispatch (§2.3) y la puerta todo-o-nada (§2.3) dejan de ser casos
especiales del backend y pasan a ser un campo que el checker rellena.

---

## 5. `BackendTy`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]   // Copy: es campo de cada nodo
pub enum BackendTy {
    // escalares — viajan en registro, sin tag
    Int, Float, Bool, Char,
    // referencias con tipo conocido
    Str, Bytes, Decimal, BigInt,
    Array(TyId), Map(TyId, TyId), Set(TyId), Tuple(TyListId),
    Class(ClassId),
    Enum(EnumId),
    Fn(SigId),
    Nullable(TyId),
    Void, Never,
    Dynamic(DynReason),
}
```

`BackendTy` es `Copy`; los constructores estructurados guardan handles a una
tabla de interning por módulo, como hace hoy `TyTable`. `TyId` indexa un
`BackendTy`; `TyListId` indexa una secuencia de ellos, para tuplas y firmas.
`ClassId`, `EnumId`, `SigId` y `FnId` indexan las tablas del módulo descritas
en §5.4 y §6.

**Escalares numéricos: solo `Int` (i64) y `Float` (f64).** No hay
`BackendTy` para anchos angostos (spec §11–§12, ADR-0015): un almacenamiento
más estrecho es decisión del optimizador sobre un `Int`/`Float` probado, no un
tipo. `Decimal` y `BigInt` son referencias.

**`BackendTy` no implementa `Default`.** Deliberadamente: no debe existir "el
tipo que sale cuando no pusiste ninguno". Es la contrapartida directa de
`impl Default for Type` (§1).

### 5.1 `Nullable` deja de ser `Dynamic`

`CgTy::to_type_tag()` colapsa hoy `Nullable(_)` a `TypeTag::Dynamic`: un
`int?` no es "un int que a veces falta", es opaco. Es la unión más común del
lenguaje — 64 usos en el corpus.

El tipo conserva la nulabilidad; la **representación** la elige el backend:

* sobre referencia, el patrón nulo — gratis;
* sobre escalar, el par `(valor, bit)` (pendiente: `Nullable` hoy es `Dynamic`;
  ver `plans/2026-09-20-PLAN-PENDIENTE.md` §5.3), que Cranelift pasa en dos
  registros.

### 5.2 `Dynamic` lleva su razón

```rust
pub enum DynReason {
    HostBoundary,     // frontera con el host, JSON.parse
    Union,            // unión no discriminada
    IndexSignature,   // { [key: str]: T }
    Unannotated,      // el autor no anotó
    NotYetSupported,  // el TIR aún no lo expresa
}
```

Cambia la pregunta de "cuánto `dynamic` queda" a "por qué queda", que es la
que sirve para priorizar. `NotYetSupported` es la lista de pendientes del
rediseño, contable con un comando.

### 5.3 Uniones no discriminadas → `Dynamic(Union)`

27 usos no justifican representación propia en este bloque. Queda visible y
contable para decidirlo con datos más adelante.

### 5.4 `Class(ClassId)` lleva identidad, no layout

El layout vive en **una** tabla por módulo, producida por el checker e
indexada por `ClassId`. Los cuatro sitios de §2.6 pasan a leer de ahí. Es la
precondición de empaquetar campos: mientras haya dos algoritmos con
resultados distintos, conectar el offset del checker al runtime produce
lecturas desalineadas.

---

## 6. `Resolution`

```rust
pub enum Resolution {
    None,                                       // literales, aritmética
    Local(LocalId), Param(u32), Upvalue(u32),
    GlobalSlot(u32),
    ModuleSlot { module: ModuleId, slot: u32 },
    FieldSlot(u16), StaticField(u16),
    VtableSlot(u16),
    DirectFn(FnId),
    Intrinsic(IntrinsicId),
    NativeOp(NativeOpId),
    EnumVariant { enum_id: EnumId, tag: u16 },
    ByName { name: Rc<str>, why: DynReason },
}
```

`ByName` lleva razón por el mismo motivo que `Dynamic`: la diferencia entre
"quedan 1037 accesos por nombre" y "quedan 1037, de los cuales tantos son
índices de string, tantos son host y tantos son bugs".

### 6.1 Vtables

El checker no calcula vtables hoy — `vtable` no aparece en `varn-checker`.
La materia prima ya está: `ClassMemberInfo` trae `kind`
(Method/Getter/Setter/Property), `is_static`, `is_override` y `ty`, y
`class_parents` da la jerarquía. La construcción es un recorrido base→derivada
asignando un índice por nombre de método, donde `override` reutiliza el índice
del padre.

### 6.2 Clases nativas en la jerarquía

La puerta todo-o-nada de `checker_annotations/exprs.rs:211` desaparece. Las
clases nativas (`Error` y compañía) publican layout y vtable en una tabla
fija: son conocidas en compilación y no hay razón para no describirlas. Una
subclase reserva el prefijo del ancestro y numera sus miembros propios a
continuación, de modo que `class E extends Error` recupera slot en todos sus
campos.

---

## 7. El verificador

Corre en **cada compilación**, con `panic!`, igual que el verificador SSA
actual (`ssa/mod.rs:21`), que no está tras `cfg(debug_assertions)`.

### 7.1 Bien formado — error

Todo nodo tiene `ty` (garantizado por el tipo Rust, no comprobado en tiempo
de ejecución). Toda referencia — `ClassId`, `TyId`, `FnId`, `EnumId`,
`SigId` — apunta a una entrada existente en las tablas del módulo. Todo slot
está en rango del layout o la vtable que dice indexar.

### 7.2 Coherencia tipo ↔ operación — error

Es lo que hoy no existe en ninguna parte. `ssa/verify.rs` comprueba forma SSA
—definición única, dominancia— y no toca los tipos.

```
Binary{Add} con lhs:Int rhs:Int      → ty debe ser Int
Binary{Add} con lhs:Int rhs:Float    → error: falta un `as` explícito (Convert)
Binary{Div} con lhs:Int rhs:Int      → ty debe ser Int (la división no cambia de dominio)
Field{res:FieldSlot(3)}              → receptor Class(id);
                                       layout[id][3] existe;
                                       ty del nodo == tipo declarado del campo
MethodCall{res:VtableSlot(7)}        → receptor Class(id) con método en slot 7;
                                       aridad y tipos contra la firma
Call{res:DirectFn(f)}                → aridad y tipos contra sig(f)
Index sobre Array(el)                → ty del nodo == el
```

**`Cast` vs `Convert`.** `Cast` es neutral en representación (clase →
interfaz, `T?` → `T` tras un guard); nunca cambia bits. Todo `as` entre
dominios numéricos baja a `InstKind::Convert { conv: NumConv }` →
`OpCode::Convert`, con la tabla única de `varn_core::numeric_conv`
(`IntToFloat`, `FloatToInt`, `BigIntToInt`, `DecimalToInt`, `IntToBigInt`,
`IntToDecimal`, `DynToInt`, `DynToFloat`). Las que pueden fallar lanzan
`IntegerOverflow` y no son puras para DCE.

**Tipo del lenguaje ≠ clase de valor runtime (Fase C).** Tres vocabularios,
cada uno con un solo dueño:

* `varn_core::LangPrimitive` (`null bool int float bigint decimal char str`
  + `void never dynamic`) y `varn_core::BuiltinType` (`Array Map Set Range
  Bytes Task TaskHandle Generator` nombrados sin argumentos): el checker los
  usa como `TypeKind::Primitive` / `TypeKind::Builtin`. `Symbol`, `Error` y
  demás son clases de plataforma nombradas (`TypeKind::Named`).
* `BackendTy`: el tipo que el backend recibe (este documento).
* `varn_core::RuntimeKind` (antes `TypeTag`): solo clasifica valores en
  runtime; no tiene `Void`, `Never` ni `Dynamic`. El layout de un campo de
  clase es `Option<RuntimeKind>` — `None` es un `VmValue` en caja — derivado
  de `BackendTy` en un único sitio (`from_tir::field_kind`), y
  `varn_types::class_layout::class_field_repr` es la única tabla de tamaños.
  El operando de `GetFixedField`/`SetFixedField` es un `FieldAccess { Slot,
  Compact(kind) }` codificado (`0` = slot, `0xFF` = en caja).

Un `FieldSlot` sobre receptor `Dynamic` no compila. Un `AddInt` con un
operando `Str` no compila. La clase entera de miscompiles deja de ser posible
en vez de ser improbable.

### 7.3 Cobertura — informe, no error

Un receptor `Class(id)` resuelto `ByName` es legal: no es un miscompile, es
una oportunidad perdida. Se cuenta y se reporta, desglosado por razón,
función y archivo.

Esa asimetría entre error y desoptimización es lo que sustituye al corpus
verde durante la rama: el verificador dice si rompiste algo, el contador dice
si avanzas.

### 7.4 El instrumento que funciona con el corpus rojo

`vn debug -p bytecode` compila sin ejecutar. Con el verificador activo,
compilar los 191 módulos del corpus es un instrumento válido mientras el
runtime está a medias: no ejecuta nada, pero rechaza toda incoherencia de
tipo o resolución sobre código real y produce el informe de cobertura.

Dos comandos nuevos:

```
vn debug -p tir       --fn F     vuelca el IR tipado
vn debug -p tir:check            verifica y reporta cobertura
```

No sustituye a ejecutar el corpus: no ve divergencias de runtime ni
miscompiles del JIT. Sustituye a no tener nada.

---

## 8. La frontera

`varn-checker` no depende de `varn-compiler` ni al revés; se hablan a través
de `varn-core`. El TIR vive en un crate propio, `varn-tir`, que hace
estructuralmente imposible que el checker importe backend o que el compiler
importe checker.

```
varn-parser → varn-checker ──emite──> varn-tir <──consume── varn-compiler
                                          ↑
                              varn-debug, varn-lsp (leen, no producen)
```

`varn-core` sigue siendo el crate base; meterle un IR de 850 líneas lo
engorda para los diecinueve miembros del workspace.

**Quién emite:** una fase `checker/emit/` que ocupa el sitio de
`checker_annotations/`. Emitir en vez de anotar es la diferencia entre
recorrer el AST apuntando en un mapa y construir un nodo cuyo constructor
exige tipo y resolución.

**`register_meta` se sustituye por `BackendTy`.** El JIT pasa de leer
`Vec<RegisterMeta{SlotKind}>` a leer `Vec<BackendTy>`. Con eso `K` deja de ser
un sistema de tipos paralelo y pasa a ser la representación **física** —
`I64`, `F64`, `I8`, `Ptr`, `Pair(valor,bit)`, `Boxed` — derivada de
`BackendTy`, no inventada por el JIT. Es donde `Nullable` se convierte en el
par de §5.1.

---

## 9. Qué se borra y qué se rompe

### 9.1 Se borra

| ruta | líneas |
|---|---|
| `varn-checker/src/checker_annotations/` | 1 245 |
| `varn-compiler/src/hir/lower/` | 4 506 |
| `varn-core/src/typed_ir.rs` | 212 |
| `varn-core/src/cg_ty.rs` | 66 |
| `varn-types/src/register_meta.rs` | 17 |
| **total** | **6 046** |

### 9.2 Se transforma o se porta

| ruta | líneas | qué pasa |
|---|---|---|
| `hir/mod.rs` | 852 | enum plano → `struct{kind,ty,res}`; `HirType` → `BackendTy` |
| `hir/inline/` | 930 | port |
| `ctor_summary` + `module_locals` | ~700 | port |
| `hir/dump.rs` → `tir/dump.rs` | 727 | reescritura |

### 9.3 Se rompe

* **`varn-jit`** — más de diez archivos leen `SlotKind`. Superficie de
  ruptura mayor.
* **`varn-lsp/features/compiler_inspect.rs`** — 965 líneas contra HIR.
* **`varn-debug/hir.rs`, `ssa.rs`** — los volcados.
* **`varn-pipeline/compile.rs`** — el orquestador.
* **La caché de bytecode en disco se invalida entera.** `FunctionProto`
  cambia de forma al cambiar `register_meta`. Se sube la versión del formato;
  no se intenta migrar.

### 9.4 Fuera de alcance

`varn-compiler/src/passes/` **no se toca**. Los once pases —`algebraic`,
`cfg`, `const_fold`, `cse`, `dce`, `escape`, `fixed_fields`, `licm`,
`monomorphize` y compañía— operan sobre SSA, no sobre HIR: ninguno menciona
`HirExpr` ni `HirFunction`. Reciben mejor información de entrada y no cambian
de forma.

`fixed_fields` es la excepción a vigilar: existe *porque* HIR no traía el
slot. Cuando el TIR lo traiga, el pase puede quedar sin trabajo que hacer.
Eso se comprueba en la etapa 4, no se asume aquí.

Explícitamente **no** se toca en este bloque: `VmValue` sigue siendo dos
palabras, el heap sigue siendo tabla de índices, `ObjData` sigue con slots de
16 bytes, el GC sigue con raíces enumeradas a mano.

Todo eso es el bloque siguiente al TIR (frame por clases, GC por clase,
layout compacto — hecho; ver `plans/2026-09-20-PLAN-PENDIENTE.md` §2).
El TIR fue su precondición: empaquetar campos requiere una sola autoridad de
layout y que el tipo del campo llegue al JIT.

---

## 10. Plan por etapas

### Etapa 0 — corpus verde, antes de abrir la rama

Arreglar los dos bugs de corrección independientes del rediseño:

* **D-1**, el guard de overflow del JIT. `AddInt`, `SubInt`, `MulInt`,
  `AddImm` y `SubImm` se saltan la comprobación razonando sobre payloads i48
  que dejaron de existir con la migración a `VmValue` de dos palabras
  (`clif/body/op_dispatch.rs:149-152` y `:271`). Reproducido: la misma
  función lanza `integer overflow` en frío y devuelve
  `-9223372036854775808` tras 500 000 iteraciones.
* **D-3**, `-9223372036854775808` no compila como literal.

No entran en la rama roja: si lo hicieran, se confundirían con las
regresiones del propio rediseño.

**D-2 no va aquí.** "Qué vale un campo sin inicializar" — hoy el checker lo
acepta, el intérprete devuelve `null` y el JIT devuelve `0` — es un análisis
de asignación definitiva, y por tanto trabajo del checker nuevo.

*Control:* corpus verde en los tres tiers.

### Etapa 1 — `varn-tir` sin consumidores

`BackendTy`, los nodos, `Resolution`, las tablas de layout, vtable y firmas,
y el verificador. Nadie lo importa todavía.

*Control:* `cargo test -p varn-tir`. Único punto del plan donde `cargo test`
es el instrumento correcto, porque aún no hay lenguaje que ejecutar: se
comprueba que el verificador **rechaza** TIR mal formado construido a mano.

### Etapa 2 — el checker emite TIR en paralelo

`checker/emit/` produce TIR; el pipeline sigue compilando por el camino
antiguo y el TIR se verifica y se descarta. Aquí entra el análisis de
asignación definitiva (D-2).

*Control:* los 191 módulos emiten TIR y pasan el verificador. Primer informe
de cobertura.

*Punto de salida barato:* si el informe muestra que el TIR no puede expresar
buena parte del corpus sin `Dynamic(NotYetSupported)`, el diseño falló y no
se ha borrado una sola línea.

El corpus sigue verde durante las etapas 0, 1 y 2.

### Etapa 3 — el corte

`ssa/build` se reengancha a TIR. Se borra lo de §9.1. `register_meta` pasa a
`BackendTy`.

*Control:* los 191 módulos compilan a bytecode sin ejecutar; `-p tir:check`
limpio.

*Segundo punto de decisión:* si SSA no se puede construir desde TIR sin
reintroducir una capa intermedia, D falló y hay que caer al enfoque A.

### Etapa 4 — el intérprete vuelve

Los opcodes que las resoluciones habilitan: `LoadGlobalIdx` emitido
directamente sin pase de reescritura, `InvokeVirtual` con índice de vtable,
`CallIntrinsic`.

*Control:* el corpus vuelve a ejecutar. `run --compare-tiers`.

### Etapa 5 — el JIT vuelve

`K` derivado de `BackendTy`; `SlotKind` fuera del JIT.

*Control:* los tres tiers de acuerdo; `cargo xtask compare`.

### Etapa 6 — consumidores secundarios

`compiler_inspect` del LSP y los volcados de `varn-debug`.

El rojo dura de la etapa 3 a la 4. Todo lo anterior y todo lo posterior tiene
instrumento.

---

## 11. Riesgos

**El TIR sale azucarado.** Es la condición que decide si D funciona: si el
TIR acaba siendo "AST con tipos pegados", `hir/` no se puede borrar y el
resultado es el enfoque A más el trabajo de D. Se detecta en la etapa 2, al
comprobar si `match` con patrones, `for…of` y los genéricos se expresan sin
residuo sintáctico.

**El TIR sale incompleto.** El riesgo opuesto, encontrado al revisar la
etapa 1 y no previsto al escribir esto. El conjunto de nodos de §4 no
expresaba todavía cuatro construcciones. Resueltas contra el AST real
(`ast/expr.rs`, `ast/stmt.rs`, `ast/pattern.rs`) como condición de entrada de
la etapa 2:

* **generadores y `async`/`await`** — `TirFunction` gana `is_async` e
  `is_generator`; se añaden `TirExprKind::Await{future}` y
  `TirExprKind::Yield{value,delegate}`. Sin estado de suspensión en el IR: el
  backend construye la máquina de estados igual que hoy desde HIR, donde
  `Await`/`Yield` son también instrucciones de un operando. El verificador
  exige que `Await` viva en función `is_async` y `Yield` en `is_generator`.
* **`?.` y `??`** — un único primitivo, `TirUnOp::IsNull` (→ `Bool`). El
  cortocircuito y la evaluación única del receptor los desazucara el emisor a
  `Let` temporal + `Select{IsNull(Var), …}`; no hay nodo dedicado.
* **`match` con patrones** — `Discriminant{value}` (→ `Int`, el tag),
  `VariantPayload{value,tag,field}` (con `res: EnumVariant`) y
  `TypeTest{value,class}` (→ `Bool`, compartido con `expr is T`). El `match`
  baja a cadena de `If`; las guardas entran como `&&` en la condición. El
  verificador comprueba que el tipo del nodo `VariantPayload` es exactamente
  el declarado para ese campo de la variante.
* **spread** — se introduce `enum TirArg { Expr | Spread | Named }` en
  `Call`/`MethodCall`/`New`/`MakeVariant`; `enum TirArrayEl { Expr | Spread |
  Hole }` en `ArrayLit`; `enum TirObjectEntry { Field | Spread }` en
  `ObjectLit`. Con `Spread` o `Named` presente, la regla de aridad/tipos de
  argumento no aplica y queda para una regla posterior.

El emisor de la etapa 2 aún debe construir estos nodos; el verificador ya los
rechaza mal formados (`tests/verify_new_nodes.rs`).

**La rama no vuelve.** Entre las etapas 3 y 4 no hay ejecución. El verificador
y el compilador de los 191 módulos son el único instrumento, y no ven
divergencias de runtime. Es el precio aceptado de la decisión 3.

**Ninguna medición de rendimiento respalda este diseño.** Un recuento de
opcodes dice dónde el lenguaje se comporta como dinámico, no dónde se va el
tiempo: en este repositorio hay registro de un cambio que eliminó 72
`GetProperty` del corpus y midió peor. Cada etapa que altere el código
generado se cierra con `cargo xtask compare`, que además verifica integridad.
Los bugs de la etapa 0 son de corrección y no necesitan justificación de
rendimiento.
