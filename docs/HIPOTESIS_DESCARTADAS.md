# Hipótesis descartadas

Cada entrada murió por una **medición**, no por un razonamiento. Están aquí para
que nadie vuelva a gastar tiempo en ellas: si vas a reabrir una, hazlo con un
dato nuevo que contradiga el que la cerró, y actualiza la entrada.

Formato: qué se creía, por qué era creíble, y qué medición la mató.

---

## 1. Rendimiento de ejecución

### El acceso a campos de objeto es el cuello — NO

El IR lo hace parecer devastador: un `GetFixedField` con clase probada baja a
~15 instrucciones y **3 saltos condicionales** (tag NaN-box, discriminante del
`HeapObj`, bounds check del número de campos), más una cadena de resolución
índice→puntero que el propio `fields.rs` marca como *"a future lever"*.

**Medición**: micro dirigido, 2M lecturas de campo en bucle sobre un array de
objetos. **Varn 40 ms, Bun 39 ms.** Empate. El coste existe y no domina.

Corolario: contar cadenas de resolución en el IR **no** predice nada. `matrix`
tiene 67 y gana a Bun; `collection_pipeline` tiene 24 y perdía. Lo que importa
es si caen dentro de un bucle caliente, y el conteo estático no lo dice.

### Un arena para objetos daría una mejora grande — NO

Parece la jugada obvia cuando ves que cada objeto es una asignación del
allocator.

**Medición**: sonda directa sobre las piezas, 5M iteraciones, con el mismo
mimalloc que instala `vn`:

```
Rc<Shape>::clone + drop   :   0.5 ns
ObjData::alloc (3 campos) :   5.5 ns   <- el allocator
ObjRef::with_shape_slice  :   5.4 ns   <- lo que llama la VM
```

**5.5 ns de los ~60 ns que cuesta un objeto de punta a punta: el 9 %.**
Saltarse el allocator por completo no puede recuperar más que eso, y el
refactor es grande (romper `Rc<ObjData>`, que garantiza que los objetos no se
mueven, y reimplementar el conteo de referencias).

Con el allocator del sistema esa cifra era 25,1 ns, así que **mimalloc ya se
llevó la mayor parte de lo que un arena podría dar**.

Antes de reabrirlo: `ObjRef::with_shape_slice` (5,4 ns) ≈ `ObjData::alloc`
(5,5 ns) — copiar los campos es gratis, toda esa función era el `malloc`.

### El GC es lo que hace lenta la alocación — NO

**Medición**: micro donde todos los objetos mueren jóvenes. El perfil dice
`2 000 020` allocs, **41 minor gc, 41 promovidos** — uno por ciclo: el
colector no hace trabajo. Y el tiempo sigue ahí. Los 174 ms son el **acto de
alocar**.

### La llamada al constructor no inlineada es el coste — NO

`new P(...)` compila a una llamada indirecta que el JIT no inlinea, lo cual
invita a culparla.

**Medición**: el objeto **literal**, que no llama a ningún constructor, es
**más lento** (125 ns) que el de clase (87 ns). La ruta de `BuildObject` es
peor que la de clase.

### El coste de alocar escala con el número de campos — NO

**Medición**: 1 campo 57 ns, 8 campos 64 ns. **+1 ns por campo**: el coste es
fijo por objeto. Por eso un arena (que ataca lo que escala) no es la palanca, y
por eso el sospechoso es el camino: cruce JIT→helper, `store_home`, `try_alloc`
del nursery, boxing del resultado.

### `resolved_shape` con búsqueda lineal es un cuello — NO

Hace una búsqueda lineal con `RefCell::borrow()` y `Rc::clone` **una vez por
objeto construido**, lo que suena caro.

**Medición**: A/B pareado, cinco pares alternados, medianas — lineal 93,22 ms
contra indexado 96,19 ms. Diferencia del 1-3 %, con una dispersión del **mismo
binario** de ±23 %. No resoluble, y el razonamiento coincide: con 1-2 shapes por
función la búsqueda recorre 1-2 elementos contiguos.

El cambio a `Vec` indexado por slot del pool se **revirtió**: añade `resize` y
un vector disperso a cambio de nada demostrable.

---

## 2. El fallo de `vn bench` en `bench_http_routing`

Síntoma: `vn bench tests/benchmarks/bench_http_routing.vn` aborta en el warmup
con `OpGetFixedField: slot 3 out of range`; `vn run` va bien. Es **del JIT**
(`VARN_NO_JIT=1` funciona) y **preexistente**. Depende del volumen: 90 000
peticiones pasa, 95 000 falla.

Causa inmediata conocida: una **referencia colgante al nursery** — `evacuate`
recibe un índice sin objeto. El índice es **49152**, exactamente
`Nursery::FULL_THRESHOLD`.

### `clif_link::adopt_if_inherited` reutiliza código del heap ancestro — NO

Su justificación (*"a deep_clone duplicates the object table wholesale, so
those baked handles name the same things here"*) deja de valer en cuanto el
clon aloca o recolecta, así que encajaba perfecto.

**Medición**: forzada a devolver `false` con una sonda. **Falla igual.**

### `proto_constants` no lo escanea el GC menor — NO

Guarda `Rc<Vec<VmValue>>` (índices de heap) y no aparece en las raíces, al
contrario que `static_closures`, que está justo debajo en el struct.

**Medición**: limpiado antes de cada colección menor. **Falla igual.**

### `proto_constants` no lo escanea el GC mayor — NO

El mayor escanea las constantes de los **frames activos**, no el caché global,
así que un proto sin frame activo tendría sus constantes sin rootear.

**Medición**: añadido a las raíces del mayor. **Falla igual.**

### Use-after-free del GC mayor (slot liberado y reutilizado) — NO

El índice tiene el bit alto puesto (old gen) y el objeto leído es un `Str`
donde debía haber otra cosa: el perfil clásico de un slot reciclado.

**Medición**: sonda que traza ese slot concreto en `sweep_phase` y en
`alloc_raw`. **Nunca se libera ni se reutiliza.** Ese objeto siempre fue ese
`Str`.

### Alguna raíz del GC menor trae un índice de nursery fuera de rango — NO

**Medición**: auditoría por secciones dentro de `run_minor_gc` (stack, globals,
modules, module_exports, static_closures, pending_ctors, pending_setters,
vm_suspend, metadata), comprobando cada `VmValue` contra la longitud viva del
nursery. **Ninguna sección lo trae.**

### Hacer que `evacuate` falle pronto con un índice inalcanzable — NO, ROMPE

`evacuate` responde a "no hay objeto aquí" con `pack_old_idx(0)`, que es un
índice **válido**: el programa sigue con un objeto ajeno y el fallo aparece
lejos. La reacción natural es devolver algo imposible (`u32::MAX`) para que
reviente en el acto.

**Medición**: **segfault**. El código compilado resuelve handles del heap **sin
comprobar límites**, así que un índice fuera de rango mata el proceso en vez de
dar un error de VM. El centinela `0` es seguro justamente por apuntar a memoria
válida.

Lo que sí se hizo: dejar el valor y **avisar una vez por proceso** con el
índice. Eso es lo que reveló el `49152`.

### Pista viva (no descartada)

`vn debug -p roots:diff` sobre ese benchmark: 149 safepoints, sólo 52 cubiertos
por home slots, **109 raíces en registro**. En código compilado los valores
vivos están en registros de CPU, no en `ctx.stack`, que es lo que el GC escanea;
de volcarlos responde `flush_boxed`/`store_home`. El informe avisa: *"Hoy es
sano porque nada bajo un raw sin frame puede alocar; deja de serlo en cuanto
`has_alloc` salga de `frame_aware`"*.

`emit_nursery_alloc` se cita en `jit_layout.rs` y `nursery.rs` pero **no
existe**: la alocación inline del JIT está preparada y sin implementar, así que
por ahí no es.

---

## 3. Los `OUTPUT MISMATCH` de `compare.ps1`

### Son errores de cálculo — NO

Cuatro benchmarks producen salida distinta a JS. **Todos los conteos y sumas
coinciden exactamente**, incluido `3583239.999999999` byte a byte. Sólo difieren
longitudes de strings serializados:

| bench | varn | js | delta | causa |
|---|---|---|---|---|
| json_native | 3033781 | 3008781 | +25000 | `JSON.stringify` de un float entero |
| csv_etl | 329181 | 328660 | +521 | ídem, más quoting CSV |
| csv_pipeline | 2298999 | 2298998 | +1 | ídem |
| http_routing | 3080014 | 3080000 | +14 | ídem |

Dos causas, ninguna un fallo de cálculo:

* `JSON.stringify` de un `float` con valor entero da `{"a":3.0}`; JS da
  `{"a":3}`. 25 000 registros × 1 byte = los 25 000 exactos. Es JSON válido
  (Python y `serde_json` hacen lo mismo), pero **es inconsistente con el propio
  Varn**: `(3.0).toString()` da `"3"`.
* En `csv_etl`, `CSV.stringify` cita `"con, coma"` según RFC 4180 mientras el
  JS del benchmark concatena sin escapar y produce CSV inválido. **Varn es el
  correcto**; el benchmark compara una nativa contra un bucle naive.

### El bug de `try/catch` explicaba el mismatch de `http_routing` — NO

Era razonable: si los errores del runtime no se capturaban, un servidor podía
divergir.

**Medición**: arreglado el bug (05912c8), los cuatro mismatches **siguen
idénticos**.

---

## 4. Método de medición

### Wall-clock absoluto entre corridas separadas — INVÁLIDO en esta máquina

El mismo binario, mismo programa, medido con minutos de diferencia:
`gc_alloc` 55,9 → 67,3 ms; `lit` 129 → 176 ms. **Deriva de hasta el 36 %.**

Una "regresión" del 20 % medida así es ruido. Todo A/B tiene que ser **pareado
y alternado** (A,B,A,B…), con medianas, y purgando la caché antes de cada
corrida. Y aun así, con ±23 % de dispersión, **una diferencia menor del 10 % no
es resoluble aquí**.

### `compare.ps1 -Baseline` era fiable — NO LO ERA (ya arreglado)

Los dos binarios compartían directorio de caché y se leían el bytecode
mutuamente. Reportó `matrix` como **+69 % de regresión** cuando el A/B aislado
daba **−8 % de mejora**. Corregido: la clave de caché incluye ahora la identidad
del productor, y el harness da a cada runtime su propio `VARN_CACHE_DIR`.

### Los comentarios del repo como evidencia — NO

Cuatro comentarios afirmaban garantías que el código no daba:

* `artifact.rs`: *"hash of the compiler crates' sources… stale artifacts
  invalidate automatically"* — no cubría el compilador.
* `proto.rs`: *"cubre varn-types y varn-compiler"* — nunca cubrió
  varn-compiler.
* `arena.rs`: *"bump-allocated contiguously, 0.00 ms GC pauses"* — no tenía
  método para alocar. Era código muerto; borrado.
* `VM_ARCHITECTURE.md`: *"Nursery de 4096 ranuras"* — son 65 536.

Y el caso caro: `dce.rs` dice *"Trap behaviour was measured, not assumed"*. El
trap existía; lo que nadie verificó es que fuera **alcanzable por un `catch`**.
No lo era — `try/catch` sólo capturaba `throw` del usuario, y una división por
cero o un `JSON.parse` inválido terminaban el proceso. Sobrevivió porque
`tests/11-errors.vn` **evitaba** dividir por cero y lanzaba a mano.

Verifica ejecutando, no leyendo.
