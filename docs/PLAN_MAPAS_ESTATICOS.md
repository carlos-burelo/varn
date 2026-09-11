# Plan: Mapas Estáticos de Alto Rendimiento y Unificación Semántica

> **Directiva Primaria:** *Sintaxis de alto nivel, performance de bajo nivel.*  
> Varn es un lenguaje estáticamente tipado. La tipificación estática debe explotarse al máximo en lugar de aplicar estrategias dinámicas heredadas de V8/JSC (shapes, transiciones, inline caches polimórficos) que introducen sobrecomplejidad y degradan el rendimiento.

---

## 0. Definición Semántica Canónica: `Object` vs `Record` vs `Map` (y eliminación de `Dict`)

Antes de optimizar el runtime, la semántica del lenguaje debe ser unívoca. En el codebase de Varn han coexistido términos y sintaxis con significados solapados (`Map`, `Dict`, `Record`, `Object`, `new Map()`, `{}`, `#{}`). 

Esta sección establece la fuente de verdad definitiva:

### 0.1 Matriz Comparativa Canónica

| Concepto | Naturaleza Semántica | Tipo Canónico en Varn | Sintaxis de Creación | Sintaxis de Acceso | Semántica de Igualdad | Representación Interna en VM |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`Object`** | Estructura / Instancia nominal o anónima con **propiedades fijas conocidas en compilación**. | `{ name: str, age: int }` o `class User` | `{ name: "Alice", age: 30 }` o `new User(...)` | `obj.name` (offset de slot fijo resuelto en compilación). | Referencia (`===`). Mutable por defecto (o `readonly`). | `HeapObj::Object` (slots continuos, layout fijo o shape monomórfico estático, **cero transiciones**). |
| **`Record`** | Tipo producto compuesto **profundamente inmutable con igualdad estructural**. Par de `Tuple`. | `#{ name: str, age: int }` | `#{ name: "Alice", age: 30 }` | `rec.name` | Estructural profunda (`==`). **Inmutable**. | `HeapObj::Record` (slots inmutables, comparación por valor). |
| **`Map<K, V>`** | **Contenedor asociativo dinámico** para pares clave-valor arbitrarios determinados en runtime. | `Map<K, V>` (y `{ [key: K]: V }` como azúcar sintáctico de firma de índice). | Target-typed `{}` / `{"a": 1}` o `new Map<K, V>()` | Corchetes `m[k]`, `m[k] = v` y métodos `.get(k)`, `.set(k, v)`, etc. | Referencia (`===`). Mutable. | `HeapObj::Map(MapRef)` (`ValueMap` / tabla hash o vector lineal para $N$ pequeño). **Cero Shapes.** |
| **`Dict`** | **ELIMINADO / PROHIBIDO**. | *Ninguno* (Violación estricta de Regla 1: No alias). | N/A | N/A | N/A | El nombre canónico en Varn es **`Map`**. `std:collections/dict` se descontinúa. No se introduce ningún `HeapObj::Dict`. |

---

### 0.2 Reglas Sintácticas y Desambiguación en el Compilador

1. **Desambiguación de `{}` mediante Target-Typing (Tipado guiado por contexto)**:
   - **Contexto `Map`**: Si la expresión literal `{ ... }` tiene como tipo esperado `Map<K, V>` o una firma de índice `{ [key: K]: V }`:
     ```varn
     let params: Map<str, str> = {};
     let headers: { [key: str]: str } = { "content-type": "application/json" };
     ```
     El compilador emite directamente `BuildMap`. Se construye un `HeapObj::Map` en $O(1)$ sin crear ni consultar `Shape`.
   - **Contexto `Object`**: Si `{ ... }` no tiene anotación de mapa o se infiere como un objeto estructural anónimo:
     ```varn
     let user = { name: "Alice", age: 30 };
     ```
     El compilador emite `BuildObjectWithShape`. Los campos son fijos y conocidos desde el análisis léxico.

2. **`#{ ... }` pertenece exclusivamente a `Record`**:
   - `#{}` crea únicamente registros inmutables por valor. Mantiene simetría con `#[ ... ]` (Tuple):
     - Posicional inmutable: `#[ 1, "hello" ]` $\rightarrow$ `Tuple`
     - Nominal inmutable: `#{ id: 1, name: "hello" }` $\rightarrow$ `Record`
   - `#{}` **nunca** crea un `Map`.

3. **Unificación de `Map<K, V>` y `{ [key: K]: V }`**:
   - En el type checker, `{ [key: K]: V }` se normaliza semánticamente a `Map<K, V>`. Ambos bajan en el TIR a `BackendTy::Map(K, V)`.
   - Ambas sintaxis representan el mismo tipo nativo y son mutuamente asignables.

4. **Sintaxis Dual de `Map<K, V>` (Indexación + Métodos)**:
   - Un `Map<K, V>` admite tanto acceso por corchetes como API de métodos orientada a objetos sobre la misma instancia:
     ```varn
     let m = new Map<str, int>();
     m["alice"] = 100;           // Compila a opcode SetIndexMap
     let v = m["alice"];         // Compila a opcode GetIndexMap
     let exists = m.has("alice");// Método nativo
     ```

5. **Erradicación de `Record<K, V>` como Tipo Mapa**:
   - En TypeScript, `Record<K, V>` era un tipo utilitario sustituto de mapas ante la ausencia de registros reales en JS. En Varn, esto provocó una colisión destructiva con `#{}`.
   - **Regla Estricta:** La palabra `Record` queda reservada **exclusivamente** para `#{}` (registros inmutables por valor).
   - Queda terminantemente prohibido usar `Record<str, T>` para mapas. Se elimina de `types.vn` y se elimina el magic-string `if name == "Record"` en `crates/varn-checker/src/binder/type_resolution/aliases.rs` (cumpliendo con la **Regla 2: Prohibición de Magic Strings**).
   - Si se requiere construir objetos a partir de uniones literales (`type Keys = "a" | "b"`), se utiliza la sintaxis directa de mapped type `{ [P in Keys]: T }` que genera un `Object` ordinario, sin usurpar el nombre `Record`.

---

## 1. Lo que está medido (Diagnóstico del Problema)

Al perfilar `tests/benchmarks/bench_http_routing.vn` con `vn bench -v` (3.85x más lento que Bun):

| Métrica | Valor Actual |
| :--- | :--- |
| Objetos alocados (100k requests) | **800 026** (~8 por request) |
| Heap allocs | 510 420 |
| GCs menores | 33 |
| `split` en `bench_http_routing` | 504 ns/llamada |
| `split` aislado (mismo string, mismo tamaño) | **173 ns/llamada** |
| Tiempo nativo total | 218 ms de ~581 ms totales (37,5 %) |

El factor 3x de degradación en `split` se debe a la presión de GC provocada por los 800k objetos. Esos objetos son mapas temporales construidos por request (`headers`, `query`, `params`), cada uno tipado `{[key: str]: str}`.

### El tipo estático ya existe en el frontend pero se descarta en el backend:

```
varn-core::CgTy::Map(K, V)          -- crates/varn-core/src/cg_ty.rs
        ↓ (el checker ya lo resuelve para Map<K,V> y {[key: str]: V})
HirType::Map(TyId, TyId)            -- crates/varn-compiler/src/hir/mod.rs
        ↓ (lower_type en tir::emit::ty)
BackendTy::Map(TyId, TyId)          -- crates/varn-tir/src/ty.rs
        ↓ (from_tir/build.rs NO distingue Map de Object dinámico)
InstKind::GetIndex / SetIndex / BuildObjectWithShape
        ↓
HeapObj::Object(ObjRef)
Shape (HashMap<Rc<str>, slot>) + Vec<VmValue> de slots
```

Consecuencia: Cada vez que se ejecuta `params[pName] = pathSegments[i]`, una clave variable (`"id"`, `"orderId"`, `"userId"`) dispara una **TRANSICIÓN DE SHAPE** global. Se paga la maquinaria diseñada para clases con campos estables en un mapa que nunca debió tener shape.

---

## 2. La Tesis

**El costo no es decidir el tipo en runtime; el checker YA lo decidió.**  
El costo radica en que el backend obliga a un valor `Map<K, V>` a pasar por la maquinaria de `Shape` (hidden classes) de V8/JSC. 

Un motor dinámico necesita shapes porque no sabe en compilación si `{}` será un objeto de 3 campos fijos o un mapa abierto. Varn ya tiene la respuesta. Pagar la penalización de transiciones dinámicas teniendo tipos estáticos es sobreingeniería innecesaria.

---

## 3. Plan de Implementación por Fases

### Fase 0 — Confirmar con Micro-Benchmark en Rust Puro (sin tocar la VM)

Antes de alterar la VM, medir en Rust puro el patrón exacto de `Route.match` / `ReqContext` (insertar 1-3 claves string cortas en un mapa vacío, leerlas, descartar):

1. **Camino actual:** `Shape` + transición + slot array.
2. **`ValueMap` actual:** `rustc_hash::FxHashMap<MapKey, VmValue>`.
3. **Vector lineal:** `SmallVec<[(MapKey, VmValue); 4]>` o vector plano sin hash para $N \le 4$. Para 2-3 claves, un scan lineal es típicamente más rápido que calcular el hash.

**Criterio:** La Fase 0 determina la representación de `ValueMap` en la Fase 1 con números duros.

---

### Fase 1 — Representación Canónica: Optimización de `HeapObj::Map`

Consolidar sobre la variante ya existente en la VM: **`HeapObj::Map(MapRef)`**:
- **Sin `Shape`. Sin transiciones.**
- Almacenamiento interno respaldado por `ValueMap` (con optimización híbrida lineal/hash según los resultados de la Fase 0).
- `set(key, val)`: Si la clave existe, sobrescribe in place; si no, inserta. Aislamiento total: no toca estructuras globales compartidas.
- Interoperabilidad total: no se introduce ningún `HeapObj::Dict` superfluo.

---

### Fase 2 — Opcodes Typed y Lowering en Compilador / VM

1. **Nuevos Opcodes Dedicados:**
   - `OpCode::BuildMap`: Construye directamente un `HeapObj::Map` a partir de pares en la pila, omitiendo shapes.
   - `OpCode::GetIndexMap`: Búsqueda directa en la tabla/vector del mapa sin branching dinámico ni shape lookup.
   - `OpCode::SetIndexMap`: Inserción/actualización directa en el mapa con su correspondiente barrera de escritura de GC.

2. **Lowering desde TIR (`crates/varn-compiler/src/from_tir/build.rs`):**
   - Cuando `TirExprKind::ObjectLit` tiene como tipo destino `BackendTy::Map`, emitir `InstKind::BuildMap`.
   - Cuando `TirExprKind::Index` opera sobre un receptor con `BackendTy::Map`, emitir `InstKind::GetIndexMap`.
   - Cuando `TirExprKind::Assign` apunta a un índice sobre `BackendTy::Map`, emitir `InstKind::SetIndexMap`.

3. **Compatibilidad con Builtins:**
   - `new Map()` y los literales `{}` tipados como mapa producen la misma representación `HeapObj::Map`, permitiendo usar tanto indexación `m[k]` como métodos `.get()`, `.set()`, `.has()`, `.delete()`, `.clear()`, `.size()`.

---

### Fase 3 — Fast-Path JIT en Cranelift (CLIF)

- Exponer el layout de memoria de `HeapObj::Map` en `JitHelpers`.
- Generar camino inline en `emit_get_index` y `emit_set_index` para `GetIndexMap` / `SetIndexMap`.
- Para accesos con claves constantes repetidas (`headers["authorization"]`), implementar un cache de un solo slot por callsite (índice directo/bucket).

---

### Fase 4 — Limpieza Arquitectónica y Descontinuación de `Dict` y `Record<K, T>`
 
1. **Descontinuar `std/collections/dict.vn`**: Reemplazar sus usos en tests por `Map<K, V>`.
2. **Erradicar `Record<K, T>` como tipo mapa**:
   - Remover `type Record<K, T>` de `crates/varn-builtins/src/modules/types/types.vn`.
   - Eliminar el bloque de magic-string `if name == "Record"` en `crates/varn-checker/src/binder/type_resolution/aliases.rs`.
   - Actualizar `tests/24-record.vn` para que pruebe `Map<str, int>` o renombrarlo a `tests/24-map-literal.vn`.
3. **Actualizar la documentación en `docs/lang/types.md`**:
   - Clarificar la frontera estricta: `Record` es únicamente `#{}` (inmutable por valor).
   - Documentar `Map<K, V>` como el único contenedor asociativo canónico.
4. **Asegurar coherencia en runtime**:
   - Asegurar que `typeof` sobre instancias de `Map` responda canónicamente `"Map"`.

---

## 4. Matriz de Compatibilidad y Sitios Críticos a Auditar

* **Módulos y Serializadores (`JSON`, `CSV`):**  
  Auditar puntos donde se asume que un contenedor es `HeapObj::Object` (por ejemplo `ctx_csv.rs` o serialización de JSON). `HeapObj::Map` debe ser serializable directamente como diccionario/objeto JSON sin requerir shapes.
* **Reflexión y Type Inspection:**  
  `typeof` e `instanceof` deben retornar de forma coherente con `TypeTag::Map`.

---

## 5. Protocolo de Verificación y Criterios de Aceptación

1. **Paridad de Tiers:**
   - Cero discrepancias en `cargo run -p varn-cli -- tests/*.vn --compare-tiers`.
   - Paso limpio de `tests/main.vn` en los 4 tiers (Interp/JIT x Dev/Embedded).
2. **Suite Específica:**
   - `tests/17-map-set.vn` (funcionalidad de Map/Set).
   - `tests/24-record.vn` (actualizado a Map o probado como Record).
   - `tests/69-tuples-records.vn` (integridad de Tuple/Record inmutables).
3. **Calidad de Código:**
   - `cargo test --workspace` sin fallos.
   - `cargo clippy --workspace` limpio.
4. **Métricas de Rendimiento (End-to-End):**
   - Ejecutar `bench_http_routing.vn`:
     - Reducción drástica de objetos alocados (eliminando las ~800k transiciones de shape).
     - Acercamiento sustancial al tiempo de referencia de Bun/Node.
