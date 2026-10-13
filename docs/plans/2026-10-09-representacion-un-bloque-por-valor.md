# Representación de valores: un bloque por valor
> Histórico. No spec vigente. Código manda. Revalidar antes de usar.
> Validación: `.\scripts\verify.ps1 -Fast` (matriz 4 cuadrantes, release); iteración `.\scripts\verify.ps1 -Quick`; sin JIT `VARN_NO_JIT=1 vn run tests/main.vn`. Cifras aquí = sesión indicada, no cita: re-medir antes de usar.

Plan de implementación. Estado al 2026-10-09.

**Objetivo.** Que cada valor compuesto (array, Map, Set, vista de string, objeto literal) viva en **una** celda del heap, sin `Vec`, `Rc` ni `Arc` colgando, y que el JIT pueda alocarlo en línea como ya hace con las instancias de clase.

**Reglas de este documento.**

- Ninguna cifra viene de otro documento. Cada número lleva el comando que lo reproduce; si el comando no da ese número, manda el comando.
- No hay código en el plan. Cada tarea nombra el contrato (tipo, función, archivo) y qué se borra; el código se escribe sin comentarios (Ley 11).
- No hay tests unitarios. La única prueba es un archivo `tests/NNN-*.vn` importado desde `tests/main.vn`, que debe **fallar en el binario base** cuando la tarea arregla un comportamiento, y pasar idéntico en ambos tiers cuando solo cambia la representación.
- Toda propuesta declara ganancia, verificación y borrado (Ley 10). Sin las tres, no entra.

---

## 1. Cómo se mide

**Probe.** `tests/benchmarks/probes/repr_rows.vn` y su gemelo `repr_rows.ts`. Cada fila crea 1 000 000 de valores retenidos en un array y reporta el mínimo de 5 corridas en proceso, en ms. Filas: `loop_only` (control), `instance3`, `objlit3`, `arraylit3`, `map3`, `map_get_heapkey` (2 M lecturas), `strview`, `split5` (500 k splits).

**Comando base.**

```
VARN_CACHE_DIR=$(mktemp -d) target/release/vn.exe run tests/benchmarks/probes/repr_rows.vn
VARN_NO_JIT=1 VARN_CACHE_DIR=$(mktemp -d) target/release/vn.exe run tests/benchmarks/probes/repr_rows.vn
bun tests/benchmarks/probes/repr_rows.ts
```

**Comparación entre dos binarios** (base contra cambio, mismo perfil): bloques alternados, se descarta la primera corrida de cada bloque, mínimo de los bloques, un `VARN_CACHE_DIR` por binario. Una diferencia menor al 10 % no se resuelve en esta máquina y se reporta como «sin efecto medible». La fila `loop_only` es el control: si se mueve, la tanda se repite.

**Tamaños.** Se leen del binario, no de la memoria de nadie: `size_of` de `VmValue`, `HeapObj`, `ArrayRepr`, `ValueMap` y la cabecera de `ObjData`, impresos por una corrida de `vn debug` o por un `const _: () = assert!(size_of::<T>() == N)` en el crate dueño del tipo.

**Validación de cada commit.** Matriz de 4 cuadrantes de `tests/main.vn` (std `dev-checkout`/`@embedded` × JIT/`VARN_NO_JIT=1`) en `release`, con `VARN_CACHE_DIR` nuevo, más el corpus `tests/errors/*.vn` con `vn check`. Criterio: `FAILED: 0` en los cuatro y 0 fallos del corpus.

---

## 2. Línea base

Medida el 2026-10-09 (i7-1355U, Windows, `release`) con el comando base, **antes** de los commits `678580d4` y `1263145b`. Las filas `objlit3` y `map3` cambiaron después; el paso 1 de cada fase es volver a medir.

| Fila | Varn JIT | Varn intérprete | Bun | Qué había por valor |
|---|---|---|---|---|
| `loop_only` | 4 | 30 | 6 | control |
| `instance3` | 24 | 89 | 10–30 | 1 celda de 40 B, alocada en línea por el JIT |
| `objlit3` | 45 | 105 | 9–11 | 1 celda de 128 B vía helper + `Vec` temporal + `Rc<Shape>` |
| `arraylit3` | 49–54 | 110 | 11–15 | celda de 96 B + `Vec` de 48 B vía helper + `Vec` temporal |
| `map3` | 87 | 156 | 10–11 | celda + `Rc<RefCell<ValueMap>>` + `Vec` de entradas |
| `map_get_heapkey` | 39 | 314 | 3–4 | cada lectura con clave en el heap re-hashea el contenido |
| `strview` | 29–31 | 153 | 21–28 | celda + `Arc<str>` clonado |
| `split5` | 124–139 | 194 | 7–8 | 3 temporales + 3 vistas con `Arc` + array (celda + `Vec`) |

Lo que la tabla dice:

1. **La alocación en línea pesa.** Con todo por Rust (intérprete) la instancia cuesta 59 ns netos y el objeto literal 75; con JIT, 20 contra 41. La estructura explica poco; el helper explica casi todo.
2. **La estructura pesa en Map.** Un Map costaba ~2x un objeto en los dos tiers.
3. **`split5` contra Bun no es comparable 1:1.** 14 ns por split con un array de 7 elementos solo se explica con una caché de splits constantes en JSC. La meta se fija contra nuestro propio costo, no contra Bun.

---

## 3. Hecho

| Commit | Qué | Prueba en la suite |
|---|---|---|
| `678580d4` | `BuildObject` deja de cerrar upvalues en el JIT (una closure en un objeto literal no veía escrituras posteriores) | `tests/159-closure-in-object-literal.vn` |
| `1263145b` | Map y Set viven en la celda (`CellTable<T>`); desaparece el `Rc<RefCell<…>>` | `tests/160-map-set-representation.vn` |
| `0b963319` | `length` de un string SSO cuenta caracteres, no bytes | `tests/161-string-length-chars.vn` |
| `4e08159d` | Un literal de tupla en código compilado es una tupla (el JIT lo construía como array) | `tests/166-tuple-literal-in-compiled-code.vn` |

## 4. Descartado con medición

| Idea | Resultado | Cómo se midió |
|---|---|---|
| Entradas de Map inline (`SmallVec<[_; 4]>`) | Peor que el `Vec`; revertido | fila `map3`, dos binarios pareados |
| `startsWith` con SSO en línea | ~1 ns, sin efecto en ninguna fila | fila propia en el probe, dos binarios pareados |
| `VmValue` de 8 B (NaN-boxing) | Techo de 2–9 % por fila | simulación del tamaño de slot sobre las filas del probe |
| `Rc<Shape>` como causa de lentitud | 0,5 ns por clone + drop | micro en proceso; solo se toca como prerrequisito de la Fase C |

---

## 5. Fases pendientes

El orden lo decide la medición del paso 1 de cada fase, no este documento. Cada fase es independiente y se commitea por tareas.

### Fase A — Clave de Map en O(1)

- **Ganancia.** `map_get_heapkey` deja de re-hashear el contenido del string en cada lectura. Meta: acercar la fila a `map3` por lectura.
- **Verificación.** Fila `map_get_heapkey`, dos binarios pareados, ambos tiers.
- **Se borra.** El camino que canonicaliza la clave buscándola en el interner en cada acceso.
- **Contrato.** `HeapStr` lleva un bit «internado»; `canonical_map_key` devuelve el mismo valor si el bit está puesto. Archivos: `crates/varn-vm/src/heap/str.rs`, `heap/strings.rs`, `heap/map_keys.rs`.
- **Prueba.** Amplía `tests/160-map-set-representation.vn`: misma clave construida por concatenación, por `slice` y por literal encuentra la misma entrada; clave no internada sigue funcionando.

### Fase B — Arrays y split en un bloque

- **Ganancia.** `arraylit3` pierde el `Vec` y el `Vec` temporal del helper; `split5` pasa de 3 temporales a una pasada.
- **Verificación.** Filas `arraylit3` y `split5`, pareado, ambos tiers.
- **Se borra.** `BoxedElems` como `Vec` separado; el sondeo de layout de `Vec` desde el JIT (pasa a constantes de `varn-jit-abi`); las dos copias del algoritmo de split.
- **Contrato.** Los elementos nacen en la cola de la celda y derraman a un buffer solo al crecer más allá de la capacidad inline; `ArrayRepr` conserva sus tres representaciones (`Boxed`, `I64`, `F64`) sobre ese buffer; `split_ranges` en `crates/varn-types/src/str_util.rs` es la única fuente del algoritmo. Archivos: `crates/varn-types/src/vm_value/{array,vm_array,vm_array_access}.rs`, `crates/varn-vm/src/heap/cells/typed.rs`, `crates/varn-vm/src/heap/jit.rs`.
- **Prueba.** Archivo nuevo en la suite: literal + `push` más allá de la capacidad inline, `I64` que pasa a `Boxed` al recibir un string, `F64` que recibe un int, `pop`/`shift` hasta vaciar y volver a crecer, `split` con separador vacío, de varios caracteres y ausente; todo dentro de un bucle caliente para cubrir el JIT.
- **Riesgo.** Identidad: el array no se realoca nunca (la identidad de un objeto es su dirección). El derrame mueve los elementos, no la celda.

### Fase C — Vistas de string sin `Arc`

- **Ganancia.** `strview` y la parte de vistas de `split5` pierden el `Arc` atómico.
- **Verificación.** Filas `strview` y `split5`, pareado.
- **Se borra.** `HeapStr::View` con `Arc<str>`; el clon atómico por vista.
- **Contrato.** La vista guarda la referencia a la celda padre y el GC la traza como hijo. Archivos: `crates/varn-vm/src/heap/str.rs`, `heap/children.rs`.
- **Prueba.** Archivo nuevo en la suite: vista que sobrevive a su padre, padre con buffer de append que se realoca, GC forzado entre creación y lectura, vista y Map literal como argumentos de `spawnIsolate`.

### Fase D — Objetos literales alocados en línea

- **Compuerta.** Solo se empieza si, tras A–C, `objlit3` sigue a más de 1,5x de `instance3` en JIT con el comando base.
- **Ganancia.** `objlit3` se acerca a `instance3`: el JIT toma la celda y escribe los campos sin helper.
- **Verificación.** Fila `objlit3`, pareado; `instance3` como control de que la toma de celda compartida no regresa.
- **Se borra.** Los helpers `jit_build_object_with_shape` y `jit_build_record_with_shape`; la copia de la toma de celda que hoy vive solo en `classops.rs`.
- **Contrato.** Las shapes son inmortales (el JIT no puede subir un refcount) y se referencian por puntero estable; la toma de celda en línea es una sola función compartida por instancias y objetos. Archivos: `crates/varn-types/src/value/shape.rs`, `value/object/*`, `crates/varn-jit/src/clif/from_ssa/{classops,heap}.rs`.
- **Prueba.** Amplía `tests/159-closure-in-object-literal.vn` y agrega: objeto que recibe un campo después de creado (transición de shape), objetos con shape de clase creados por el host (`IsolateHandle`, `Channel`, errores reconstruidos), y un bucle que crea 100 000 shapes iguales sin crecer la tabla de shapes.

### Fase E — Tipos objeto exactos como instancias

- **Compuerta.** Depende de D. Hoy `{ id: int }` se baja a `BackendTy::Dynamic(DynReason::Shape)`: el backend conoce sus campos y la frontera con `dynamic` ya valida la forma (`tests/171-dynamic-boundary-object-types.vn`), pero el valor sigue siendo un objeto de shape dinámica con acceso por nombre.
- **Ganancia.** Un valor de tipo objeto exacto se representa como una instancia de layout fijo (24 ms contra 45 en `instance3`/`objlit3` de la línea base).
- **Verificación.** Fila nueva `typedobj3` en el probe.
- **Se borra.** El acceso por nombre con caché de shape para valores cuyo tipo estático ya fija el layout.
- **Contrato.** `DynReason::Shape` se gradúa a una variante propia de `BackendTy` con layout. El único flujo que impide el layout fijo es «tipado → `dynamic` → agregar campo»: se decide en esta fase si ese flujo se prohíbe en el checker o si el objeto se desoptimiza a shape dinámica.
- **Prueba.** `tests/171-dynamic-boundary-object-types.vn` debe pasar sin cambios; se agrega lectura y escritura de campos en bucle caliente sobre el layout fijo.

---

## 6. Modos de falla que cada fase debe cubrir en la suite

1. Callback que muta el mismo Map/Set durante `forEach` (semántica de instantánea, sin corrupción).
2. Crecimiento más allá de la capacidad inline, conservando valores y orden.
3. Vista de string que sobrevive a su padre, con GC en medio.
4. Valores que cruzan isolates.
5. Objetos con shape de clase creados por el host tras volver inmortales las shapes.

## 7. Fuera de alcance

- `http_routing`: el despacho no lo domina la representación. Se mide aparte con `cargo run --release -p xtask -- compare`.
- `VmValue` de 8 B: descartado en la sección 4.

## 8. Bitácora

Cada fase agrega aquí: fecha, commit, comando y las filas medidas antes y después (base y cambio, ambos tiers). Una fase sin su fila en la bitácora no está cerrada.
