# TIR · Etapa 2 — el checker emite TIR en paralelo

Continúa `TIR_CONTRATO_TIPADO.md` §10. Etapas 0 y 1 cerradas (`b2a1060`).
Esta etapa produce un `varn_tir::TirModule` por cada módulo del corpus, lo
pasa por el verificador y lo **descarta**. El pipeline sigue compilando por el
camino de HIR. Cuando termina: 191 módulos emiten TIR verde y sale el primer
informe de cobertura.

El corpus sigue verde durante toda la etapa.

---

## 1. La frontera: qué consume el emisor

`checker_annotations::collect_type_annotations` recibe hoy, y produce
`TypeAnnotations`:

```rust
program:    &Program                                  // el AST
bind:       &BindResult                               // símbolos + jerarquía + imports
resolver:   &dyn ImportResolver                       // sigue imports
expr_table: &FxHashMap<AstId, checker::TypeEntry>     // el tipo de cada expr
```

`checker/emit/` toma **exactamente** esas cuatro entradas y devuelve
`TirModule`. Ni una más: si el emisor necesita un dato que el checker no
expone, ese dato es una laguna del checker y se arregla ahí, no se recalcula
en `emit/`.

`TypeEntry` trae dos carriles — `ty` (contra el que se comprobó) y `refined`
(lo más fuerte demostrado, "consumed only by codegen"). **El emisor usa
`refined.unwrap_or(ty)`**: es el consumidor de codegen para el que ese carril
existe.

### Dónde se engancha

Nueva fase paralela a `collect_type_annotations` en `checker/mod.rs`. El
`CheckResult` gana un campo `tir: Option<TirModule>` que el pipeline verifica
(`verify_module`) y, con `-p tir` / `-p tir:check`, vuelca o informa. En
compilación normal el campo se construye, se verifica con `panic!` como el
verificador SSA, y se tira.

`varn-checker` gana dependencia de `varn-tir`. `varn-tir` NO depende de
`varn-checker` — la traducción `Type → BackendTy` vive en `checker/emit/`,
del lado del productor.

---

## 2. `Type → BackendTy`

`Type(SemanticTypeKind, tainted)` donde
`SemanticTypeKind = TypeKind<Box<Type>, Rc<str>, Vec<Type>, FunctionType, Vec<ObjectTypeMember>, ()>`.

| `TypeKind` variante | `BackendTy` |
|---|---|
| `Intrinsic(Int/Float/Bool/Char)` | escalar homónimo |
| `Intrinsic(Str/Decimal/BigInt)` | `Str` / `Decimal` / `BigInt` |
| `Intrinsic(Void/Never)` | `Void` / `Never` |
| `Intrinsic(Null)` | `Nullable` de `Never` — "solo null" |
| `Intrinsic(Dynamic)` | `Dynamic(Unannotated)` salvo que el sitio sepa mejor (§2.1) |
| `Intrinsic(Object)` | `Dynamic(IndexSignature)` |
| `Array(t)` | `Array(intern(lower(t)))` |
| `Tuple(ts)` | `Tuple(intern_list(ts.map(lower)))` |
| `Named(n, _)` clase | `Class(class_id(n))` |
| `Named(n, _)` enum | `Enum(enum_id(n))` |
| `Named(n, _)` alias | `lower` del alias resuelto (`get_alias_node`) |
| `Named("Map"/"Set", args)` vía `Generic` | `Map` / `Set` |
| `Fn(FunctionType)` | `Fn(sig_id(...))` |
| `Union(ts)` con `null` ∈ ts | `Nullable(intern(lower(union sin null)))` |
| `Union(ts)` no discriminada | `Dynamic(Union)` |
| `Object(members)` | `Dynamic(IndexSignature)` |
| `EnumVariant{..}` | `Enum(enum_id(enum_name))` |
| `Generic`, `KeyOf`, `Mapped`, `Conditional`, `Infer`, `IndexedAccess`, `TypePredicate`, `Typeof`, `TemplateLiteral`, `Intersection`, `This` no resuelto | `Dynamic(NotYetSupported)` |

`tainted == true` no cambia el `BackendTy` — es señal de que el checker ya
reportó un error; el emisor no añade ruido.

### 2.1 De dónde sale `DynReason`

* Retorno de función nativa / `JSON.parse` / valor que cruza el host →
  `HostBoundary`. Lo marca el sitio del `Call`, no el mapa de tipos.
* `Intrinsic(Dynamic)` en una posición con anotación explícita del autor que
  dijo `dynamic` → `Unannotated` igual (el autor renunció).
* `Intrinsic(Dynamic)` sin anotación y con inferencia fallida → `Unannotated`.
* Lista de §11 del contrato → `NotYetSupported`.

Es la métrica que decide el punto de salida barato: si el informe sale
dominado por `NotYetSupported` sobre construcciones comunes, el TIR no sirve
y no se ha borrado nada.

---

## 3. Las tablas del módulo

Se construyen **antes** de emitir cuerpos, porque `Class(ClassId)` y
`Enum(EnumId)` necesitan el índice.

* **Clases.** `bind` da la jerarquía (`class_parents`) y los miembros
  (`ClassMemberInfo`: `kind`, `is_static`, `is_override`, `ty`). `ClassInfo`
  vía `ClassInfo::new_with_methods`, recorrido base→derivada. Los campos en
  orden de declaración; `override` reusa el slot del padre (ya lo hace
  `new_with_methods`).
* **Vtables.** El checker no las calcula hoy (§6.1 del contrato). El emisor
  las construye del mismo recorrido: un índice por nombre de método de
  instancia, `override` comparte índice. Getters/setters entran como métodos
  con nombre decorado (`get x` / `set x`) — a decidir en la primera sub-fase
  contra el AST real.
* **Clases nativas** (`Error` y compañía) en una tabla fija, con layout y
  vtable conocidos. Una subclase reserva el prefijo del ancestro. Esto es lo
  que mata la puerta todo-o-nada de `checker_annotations/exprs.rs:211`.
* **Enums.** `EnumInfo` con `VariantInfo{ name, tag, payload }`. El tag es el
  orden de declaración.
* **Firmas.** `Signature{ params, return_ty }` interned; funciones libres y
  entradas de vtable la referencian por `SigId`.

---

## 4. Los cuerpos

Recorrido del AST guiado por `expr_table`. Cada `ExprKind` / `StmtKind` →
nodo TIR con `ty` de `refined.unwrap_or(ty)` y `res` de lo que el checker
probó (`semantic_info`: `CallResolution`, `MemberResolution`).

Orden de las sub-fases (cada una es un commit, verificador verde al final):

1. **Esqueleto** (hecho). `emit_module` produce `TirModule` con tablas
   vacías y el cuerpo = un `TirStmt::Expr` con `Dynamic(NotYetSupported)`.
   `varn-tir` dep en `varn-checker`; `checker/emit/` con `lower_type`
   (escalares, `Array`, `Tuple`, `T|null`→`Nullable`, uniones→`Dynamic(Union)`,
   `Named`→`NotYetSupported` sin tabla). `DebugFlags{tir,tir_check}`,
   `varn_debug::tir`, wiring en `compile.rs`. `-p tir` vuelca, `-p tir:check`
   verifica e informa (línea base: cobertura estática 100 % sobre 1 nodo
   `NotYetSupported`).
1b. **Tablas.** Clases, enums, vtables y firmas reales desde `bind`;
   `NameResolver` deja de ser `NoNames`.
2a. **Literales, `Var`, aritmética, `if`/`while`/`return`** (hecho). Cuerpos
   de módulo y de funciones libres. `Var` resuelve a `Local`/`Param`;
   globales/imports quedan `ByName` hasta la sub-fase 4. `Binary`/`Unary`
   sólo se emiten cuando los operandos son un escalar coherente — un
   `int + float` degrada a placeholder hasta que 2b añada `Cast`. Regla del
   verificador ampliada: null desnudo (`Nullable(Never)`) asigna a cualquier
   `T?` (`return null` en función `T?`).
2b. **`for` C, `do…while`, `for…of`/`for…in`.** Desugar al único `Loop`.
   `for…of` necesita el protocolo de iterador (método `.next()`), así que
   depende de la sub-fase 3. Aquí se prueba el riesgo "TIR azucarado": si
   `for…of` no baja sin residuo, D falló.
3. **Campos y métodos** (hecho). `Member` no computado → `Field` +
   `FieldSlot` (tipo del nodo = tipo declarado del campo, la autoridad) o
   `ByName`; `Member` computado → `Index`. `Call` sobre `Member` →
   `MethodCall` + `VtableSlot` (sólo si aridad casa con la firma) o `ByName`.
   `this` → `Var : Class(this_class)`. `Assign` simple (`=` a identificador o
   campo). Métodos y constructores de clase → `TirFunction` con `has_this`.
   Firmas de método = `Dynamic` en aridad correcta (el tipado preciso de
   firmas de método es sub-fase propia — un retorno sin anotar llega como
   `Void` del binder). `Intrinsic` / `DirectFn` en `MethodCall` → sub-fase 5 /
   sin resolver aún. Coverage con señal real: `url.vn` 44 static / 21 by-name
   / 48 NotYetSupported.
4. **Globales y llamadas libres** (hecho). Símbolos-valor del scope global
   (`Var`/`Let`/`Const`/`Function`/`Class`/`Enum`/`Namespace`/`Struct`) →
   slot por orden de declaración del binder; `Var` no local/param → `GlobalSlot`.
   `Call` sobre identificador → `DirectFn(FnId)` si la función es libre y la
   aridad casa (firma forzada a `f.params.len()` para que el verificador
   concuerde), si no `Call` + `ByName`. Regla del verificador ampliada:
   `int`→`float` en argumento de llamada; subclase→ancestro por la cadena
   `ClassInfo.parent`. Coverage: `url.vn` 166 static / 35 by-name.
5. **Colecciones y `New`** (hecho). `ArrayLit` (+ `Hole`/`Spread`),
   `TupleLit`, `ObjectLit` (spread sí; métodos/getters/setters en literal de
   objeto → sub-fase posterior). `New` sobre identificador de clase local →
   `New{class}`, si no placeholder. `E.V` / `E.V(args)` → `MakeVariant` +
   `res: EnumVariant{enum_id, tag}`. Regla del verificador: `Array`/`Set`
   covariantes en el elemento para asignabilidad. Coverage: `12-classes.vn`
   64 static / 37 by-name.
6a. **`?.`, `??`, `&&`, `||`** (hecho). Desugar a `Select` con receptor puro
   (sin llamadas/asignaciones/construcción — re-bajable, porque `Select`
   nombra un operando dos veces). `a ?? b` → `IsNull(a) ? b : a`; `a?.b` →
   `IsNull(a) ? null : a.b`; `a && b` → `a ? b : false`; `a || b` →
   `a ? true : b`. Receptor no puro → placeholder hasta 6b (temp hoisted).
6b. **`match` + temp hoisted** (hecho). `FnEmitter` gana buffer `pending`;
   `lower_stmt` pasa a `Vec<TirStmt>`. `match` en posición de sentencia,
   `return` y `let x = match` → sujeto hoisted + cadena de `If`. Patrones:
   wildcard, identificador (binding, o test de variante nularia si el sujeto
   es enum), literal (`s == lit`), `T` (`TypeTest` + binding), variante de
   enum (`Discriminant(s) == tag` + `VariantPayload` por binding; el enum
   sale del nombre del patrón o del tipo del sujeto). Guarda pura → `&&`.
   `?.` / `??` con receptor con efectos → hoist a temp. Además: `let`/`const`
   a nivel de módulo ahora entran en `top_level.body` (antes se saltaban
   todos los `Decl`); tabla de enums lee payloads de `sum_variant_fields`;
   `lower_type` resuelve `Generic` sin argumentos como nombre.
   Pendiente: `match` como sub-expresión anidada; patrones Record/Sequence;
   for/for-of/do-while.
7. **`async` / generadores** (hecho). `is_async` / `is_generator` en
   `TirFunction` desde los modifiers (funciones libres y métodos).
   `ExprKind::Await` → `Await{future}`; `ExprKind::Yield` → `Yield{value,
   delegate}` (valor de resume sin tipar todavía). El verificador exige
   `Await` en función `is_async`, `Yield` en `is_generator`.
8. **Spread y named args** (hecho ya en 3/5). `lower_arg` mapea
   `Positional`/`Spread`/`Named` a `TirArg`; `ArrayLit`/`ObjectLit` llevan
   spread. Con spread/named presente, `DirectFn`/`VtableSlot` degradan a
   `ByName` (el verificador no comprueba aridad).

2b (tardío). **`for` C, `do…while`, `for…of` sobre array** (hecho). `for` →
   init + `Loop{true}` con `update` al inicio del body tras flag de primera
   iteración (para que `continue` avance) + `test` como puerta de `break`.
   `do…while` → body + puerta de `break` al final. `for…of` sobre iterable
   `Array(el)` → `let i=0; loop { if !(i<len) break; let x=arr[i]; body; i=i+1 }`.
   `for…in`, `for…of` sobre no-array (protocolo iterador), `switch`, `try`,
   `using` → placeholder.

Cada sub-fase que aún no cubra una forma la emite como
`Dynamic(NotYetSupported)` con un `TirStmt::Expr` placeholder — nunca un
nodo a medias que el verificador no pueda comprobar.

---

## 5. D-2 — asignación definitiva

"Qué vale un campo/local sin inicializar." Hoy el checker lo acepta, el
intérprete devuelve `null`, el JIT `0`. Es análisis de flujo, no de tipos, y
es trabajo del checker nuevo — entra en esta etapa, no en la 0.

Alcance mínimo: un local o campo leído en un camino donde no se le asignó es
error del checker. No se persigue el caso intra-expresión ni el flujo
complejo; lo justo para que el emisor no tenga que inventar un valor.

Va como sub-fase propia entre la 2 y la 3, con sus propios `tests/*.vn` en
la carpeta de errores del checker.

---

## 6. Comandos

```
vn debug -p tir       --fn F     vuelca el TIR de una función
vn debug -p tir:check            verifica los 191 módulos e informa cobertura
```

`tir:check` reporta silencio si todo verifica; el informe de cobertura sale
siempre, desglosado por `DynReason`, función y archivo (ya lo produce
`varn_tir::Coverage::report`). No es parte de `-p all` ni de `-p check`: es
para barrer un corpus, como `clif:check`.

`DebugFlags` gana `tir: bool` y `tir_check: bool`. La construcción del
`TirModule` en `check.rs` se dispara con `debug.tir || debug.tir_check` o en
cada compilación una vez la etapa 3 lo haga obligatorio — en la etapa 2 basta
con los flags para no pagar el coste en cada `run`.

---

## 7. Controles y puntos de salida

* **Control de etapa:** `vn debug -p tir:check` sobre el corpus (191
  módulos) — cero errores del verificador, informe de cobertura publicado.
* **Riesgo "TIR azucarado"** (contrato §11): se decide en la sub-fase 2 y 6.
  Si `for…of`, `match` con patrones o los genéricos no bajan sin residuo
  sintáctico, `hir/` no se puede borrar → caer al enfoque A. Nada borrado
  todavía.
* **Riesgo "TIR incompleto":** resuelto en `b2a1060`; los nodos existen, el
  emisor solo tiene que construirlos.
* **Punto de salida barato:** si tras la sub-fase 3 el informe muestra que
  `NotYetSupported` domina sobre construcciones comunes del corpus, el diseño
  falló y no se ha borrado una sola línea.

---

## 8. Fuera de alcance de la etapa 2

`ssa/build` sigue enganchado a HIR. `register_meta` sigue siendo
`SlotKind`. Nada de §9.1 del contrato se borra todavía — eso es la etapa 3,
el corte, en rama larga.
