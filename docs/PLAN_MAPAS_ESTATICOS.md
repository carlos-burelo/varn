# Plan: usar `HirType::Map` en vez de descartarlo

Objetivo: los valores tipados `{[key: K]: V}` (index signature / `Record<K,V>`)
dejan de pasar por la representación pensada para objetos de POCAS propiedades
ESTABLES (`Shape` + array de slots, con transición de shape en cada clave
nueva) y pasan a una representación de mapa real — sin perder la garantía que
`Shape` sí necesita (formas de clase con IC monomórfico/polimórfico).

No es un plan de "más magia de runtime". Es lo contrario: el checker YA
resuelve estáticamente cuándo un valor es un mapa homogéneo — el fix es dejar
de tirar ese dato en el camino checker → backend y empezar a usarlo, el mismo
patrón que `AddInt`/`AddFloat` ya usan para aritmética typed. Ningún paso de
este plan agrega inferencia nueva; todos consumen algo que `varn-checker` ya
calcula hoy.

---

## 1. Lo que está medido

Perfilando `tests/benchmarks/bench_http_routing.vn` con `vn bench -v`
(3.85x más lento que Bun, el único outlier del benchmark suite — el resto cae
en 1.2x-2x, brecha esperable contra V8/JSC):

| | |
|---|---|
| Objetos alocados (100k requests) | **800 026** (~8 por request) |
| Heap allocs | 510 420 |
| GCs menores | 33 |
| `split` en `bench_http_routing` | 504 ns/llamada |
| `split` aislado (mismo string, mismo tamaño) | **173 ns/llamada** |
| Tiempo nativo total | 218 ms de ~581 ms totales (37,5 %) |

`split` no está roto — 173 ns aislado es razonable. El 3x de diferencia entre
aislado y en contexto es presión de alocación/GC agregada, no un hotspot
puntual. Los 800k objetos son mayormente mapas dinámicos reconstruidos por
request: `headers`, `query`, `params`, headers de respuesta — cada uno
`{[key: str]: str}`.

### Confirmado en el código: el tipo estático existe y se descarta

```
varn-core::CgTy::Map(K, V)          -- crates/varn-core/src/cg_ty.rs:23
        ↓ (el checker ya lo infiere para {[key: str]: V})
HirType::Map(TyId, TyId)            -- crates/varn-compiler/src/hir/mod.rs:26-27
        ↓ (from_tir/build.rs lo transporta hasta la instrucción SSA)
InstKind::GetIndex / SetIndex / BuildObject / BuildObjectWithShape
        ↓ (NINGUNO mira el tipo del objeto — mismo camino que `dynamic`)
HeapObj::Record(ObjRef) — LA MISMA representación que HeapObj::Object:
Shape (HashMap<Rc<str>, slot: usize>) + Vec<VmValue> de slots
```

`grep HirType::Map` en `varn-compiler/src` da 4 resultados: dos son el nombre
para dumps de depuración (`"map"`), uno es la conversión `CgTy → HirType`. Cero
son un `match` que elija un opcode o una representación distinta por ello.
`HeapObj::Record` y `HeapObj::Object` son el mismo variante estructural
(`Record(ObjRef)`, `Object(ObjRef)`) — la única diferencia es el tag para
`typeof`/`instanceof`, no el layout.

Consecuencia concreta: `params[pName] = pathSegments[i]` — con `pName` una
clave que cambia según la ruta (`"id"`, `"orderId"`, `"userId"`,
`"itemId"`) — dispara, cada vez que aparece una clave que ese `Shape`
concreto no tenía, una TRANSICIÓN DE SHAPE (nueva entrada en el árbol de
shapes global, nuevo `Rc<Shape>`) — maquinaria construida para que una
CLASE con campos fijos publique shapes estables una sola vez, pagada aquí
por un mapa que semánticamente nunca debería tener "forma" en absoluto.

---

## 2. La tesis

**El costo no es tener que decidir el tipo en runtime — eso ya no hace
falta, el checker lo decidió.** El costo es que el backend, al no consumir
esa decisión, obliga a un valor `Map<str,str>` a pasar por la MISMA
maquinaria (`Shape` + transición) que existe específicamente para el caso
que NO es este: propiedades fijas, conocidas en la clase, con pocos shapes
distintos en todo el programa.

Un motor sin tipos estáticos (V8, JSC) necesita esa maquinaria SIEMPRE,
porque en tiempo de compilación no sabe si `obj` va a comportarse como una
clase de 3 campos fijos o como un diccionario abierto — de ahí "hidden
classes" e "IC polimórficos" con guardas de forma en cada acceso. Varn ya
sabe la respuesta en la mayoría de los casos reales (anotación `{[key:str]:
V}`, o inferencia desde uso). Pagar la misma maquinaria de todos modos es
exactamente la "magia" que no debería hacer falta.

---

## Fase 0 — Confirmar alcance y número objetivo (no tocar código)

Antes de escribir la representación nueva, medir el techo real con un
micro-benchmark en Rust puro (fuera de la VM): comparar, para el patrón
exacto de `Route.match`/`ReqContext` (insertar 1-3 claves string cortas en
un mapa vacío, leerlas de vuelta, descartar), tres implementaciones:

1. El camino actual (`Shape` + transición + slot array), aislado de la VM.
2. `rustc_hash::FxHashMap<Rc<str>, VmValue>` directo (la representación que
   propone la Fase 1).
3. Un vector lineal de `(Rc<str>, VmValue)` sin hash — plausible mejor para
   los tamaños reales (`headers` tiene 3 claves, `params` 1-2): un
   `HashMap` paga hashing que un scan de 2-3 elementos no necesita.

**No se empieza la Fase 1 sin este número.** `PLAN_ALOCACION.md` ya dejó la
advertencia aplicable acá: "los tramos no son aditivos... ninguna fase se da
por buena con el desglose, hay que demostrar la ganancia end-to-end". El
razonamiento de este documento (shape-transition es más caro que un insert
directo) es sólido pero no está medido en ns todavía — la Fase 0 es la que
lo convierte en un número, incluida la posibilidad de que el vector lineal
le gane al hash para los tamaños reales de este benchmark.

---

## Fase 1 — Representación dedicada: `HeapObj::Dict`

Nuevo variante de `HeapObj`, distinto de `Object`/`Record`, elegido por el
compilador cuando el tipo estático de un `BuildObject`/literal es
`HirType::Map(Str, _)` (la forma con más cobertura real: casi todo uso de
index-signature en este benchmark y en general tiene clave `str`).
Representación decidida por el resultado de la Fase 0 — `FxHashMap<Rc<str>,
VmValue>` si el hashing no domina, vector lineal si domina para los tamaños
típicos (probablemente ambos: lineal hasta N pequeño, hash después — el
mismo patrón que `ObjData` ya usa con `inline_len` para spill).

Sin `Shape`. Sin transición. `set(key, val)`: si la clave existe, sobrescribe
in place; si no, inserta — sin tocar nada global ni compartido entre
instancias distintas del mismo tipo `Map<K,V>` (a diferencia de `Shape`, que
SÍ es compartido y por eso paga por mantenerlo coherente entre instancias).

## Fase 2 — Opcodes typed, mismo patrón que `AddInt`/`AddFloat`

`GetIndexMap` / `SetIndexMap`, elegidos en `bin_opcode`-equivalente para
`GetIndex`/`SetIndex` exactamente como ya se elige `AddInt` sobre `Add`
genérico cuando el checker probó el tipo. Intérprete: dispatch directo al
`HeapObj::Dict`, sin el `match` dinámico que hoy prueba
Array/Object/Record/Map/Str en cada `GetIndex`.

`BuildObject` para un literal cuyo tipo de destino es `Map(Str,V)` (la
anotación de `hdrs`/`params`/headers de respuesta en el benchmark) emite
`BuildMap` en vez de `BuildObjectWithShape` — construye el `Dict`
directamente con las entradas del literal, sin pasar por `Shape` ni una
sola vez.

## Fase 3 — Camino rápido en CLIF (mismo patrón de toda la sesión)

Igual que `array_layout`/`object_layout`/`frame_layout` — probar el layout
de `HeapObj::Dict` una vez al arrancar y exponerlo en `JitHelpers`, con un
camino inline en `emit_get_index`/`emit_set_index` para `GetIndexMap`/
`SetIndexMap` que camina la estructura directo (sin cruzar a Rust) en el
caso caliente, cayendo a un helper Rust solo si hace falta crecer/rehashear.

**Esta fase conecta con un hallazgo YA documentado en memoria de sesiones
anteriores** (`getindex-const-key-has-no-inline-cache`): `obj["clave
literal"]` no tiene inline cache hoy — cada llamada rehace el lookup
completo aunque la clave sea la MISMA constante en cada ejecución de ese
sitio (`req.headers["authorization"]`, por ejemplo, siempre la misma
cadena). Con `HeapObj::Dict` ya construido, agregar un IC de un solo slot
por sitio de llamada (guardar el índice/bucket resuelto la primera vez,
igual que el poly-IC de `GetProperty`) es la pieza que más rinde de las
tres fases — pero solo tiene sentido después de la Fase 1, no antes: hoy
haría IC sobre `Shape`, que es exactamente la maquinaria que se está
retirando.

---

## 3. Orden y por qué

Fase 0 → 1 → 2 → 3, sin saltar. Fase 0 decide la representación interna de
la Fase 1 con un número, no con intuición (aprendizaje explícito de
`PLAN_ALOCACION.md`: medir antes de comprometerse a una estructura). Fase 2
es la que cambia comportamiento observable (nuevo opcode, nueva selección
en el compilador) — necesita el ritual completo de validación de esta
sesión (`--compare-tiers` en todo `tests/*.vn`, `main.vn` ×4 tiers,
`cargo test`+`clippy`) antes de tocar la Fase 3. Fase 3 es la de más
rendimiento pero la más delicada (memoria cruda vía CLIF) — mismo patrón de
riesgo ya asumido y validado con `JitFrameLayout` esta sesión, no una
técnica nueva.

## 4. Lo que este plan rompe, y con qué se sustituye

* **`HeapObj::Record` deja de ser sinónimo estructural de `Object`** para
  los casos que el checker tipa `Map(K,V)` — pasan a `Dict`. `typeof`/
  `instanceof` sobre esos valores debe seguir dando lo mismo que hoy
  (verificar explícitamente en la suite — no hay motivo semántico para que
  cambie, pero es el tipo de detalle que un cambio de representación
  interna puede romper sin querer).
* **Un valor puede empezar como literal (`BuildMap`) y mezclarse con un
  `Object` de shape fija en código genérico/`dynamic`** — cualquier sitio
  que hoy asuma "todo lo no-Array es `Object`/`Record` con `Shape`"
  necesita un tercer brazo para `Dict`. `ctx_csv.rs`'s
  `HeapObj::Object(obj) | HeapObj::Record(obj)` (ya visto esta sesión) es
  exactamente el tipo de sitio a auditar — un `Dict` pasado a `CSV.stringify`
  hoy caería al brazo de error ("CSV items must be objects or array rows")
  en vez de serializar sus entradas; hay que decidir si eso es lo correcto
  o si necesita su propio brazo.

## 5. Cómo se verifica cada fase

Mismo ritual de esta sesión, sin atajos: `tests/*.vn --compare-tiers` 0
disagreements, `tests/main.vn` PASSED 1180 ×4 tiers (JIT/NO_JIT ×
dev-std/embedded), `tests/65-safepoint-roots.vn`, `cargo test --workspace`,
`cargo clippy --workspace` limpio en los archivos tocados. Medición de
rendimiento end-to-end (no solo el desglose interno) en `bench_http_routing`
antes/después de cada fase, más `cargo xtask compare` completo al cierre de
la Fase 3 para confirmar que el outlier deja de serlo — o para reportar
honestamente cuánto quedó, si no cierra del todo.

## 6. Lo que este plan NO hace

* **No toca `HeapObj::Object`/`Shape` para clases o literales de propiedades
  fijas.** Esa representación es correcta para lo que fue pensada — el
  problema es que hoy también absorbe lo que no debería.
* **No es el `PLAN_ALOCACION.md`.** Ese plan ataca el costo POR OBJETO
  alocado (60 ns → 20 ns) sin importar la representación; este plan ataca
  qué representación le toca a un valor según lo que el checker YA sabe de
  él. Son ejes distintos y complementarios — la Fase 1 de este plan se
  beneficia de lo que ese otro plan ya redujo, y viceversa.
* **No agrega inferencia de tipos nueva.** Si el checker no llega a `Map(K,V)`
  para un valor (queda `Dynamic`), ese valor sigue exactamente el camino de
  hoy — este plan no intenta ampliar cuándo el checker decide `Map`, solo
  usar la decisión cuando ya existe.
* **No cierra la brecha completa contra Bun/Node en `http_routing`.** El
  35-40% de tiempo en llamadas nativas string (`split`/`slice`/`startsWith`)
  no lo toca este plan — sigue siendo el costo real de motores con 15-20
  años de trabajo en sus builtins de string. Este plan ataca específicamente
  la parte del gap que es estructural y propia (mapas dinámicos pagando
  maquinaria de shape), no la totalidad del 3.85x.
