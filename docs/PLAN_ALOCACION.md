# Plan: reducir el coste de alocar un objeto

Objetivo: **de 60 ns a ≤ 20 ns por objeto**, que pondría a Varn por delante de
Bun (24 ns) en la operación que hoy decide todos los benchmarks que perdemos.

Es un plan con breaking changes deliberados. Las garantías que rompe están
nombradas, cada una con qué la sustituye.

---

## 1. Lo que está medido

| | |
|---|---|
| Coste de un objeto, punta a punta | **~60 ns** (Bun: 24 ns) |
| De eso, el allocator (`ObjData::alloc`) | **5,5 ns — 9 %** |
| El resto | **~54 ns — 91 %** |
| ¿Escala con el número de campos? | **No**: 57 ns con 1 campo, 64 ns con 8 |
| ¿Es el GC? | **No**: 41 promociones en 41 ciclos y el tiempo sigue |
| ¿Es el constructor? | **No**: el literal, sin constructor, es peor (125 ns) |

El detalle y las mediciones están en
[HIPOTESIS_DESCARTADAS.md](HIPOTESIS_DESCARTADAS.md). **Lee eso antes de
proponer una causa alternativa.**

### Reparto por tramo (Fase 0, hecha)

Contadores de ciclos sobre 2M objetos, con el overhead de la propia medición
descontado y calibrado. Micro: `{ a: i, b: i as float, c: true }` en bucle.

| tramo | ns | % | ¿evitable? |
|---|---|---|---|
| **Fuera del helper** — cruce, `store_home` de los campos, lectura | **24,5** | **59 %** | sí, Fase 1 |
| `ObjData::alloc` (asignación DST + copia) | 6,6 | 16 % | sólo Fase 2 |
| `Heap::alloc` (mover 48 B, dos `push`) | 4,2 | 10 % | sólo Fase 2 |
| barrido de closures entre los campos | 3,2 | 8 % | **sí, y es desperdicio** |
| `resolved_shape` en runtime | 2,7 | 7 % | **sí, y es desperdicio** |
| **total por objeto** | **41,5** | | |

Cómo se obtuvo el 41,5: el mismo bucle sin crear objeto cuesta 46 ms y creando
objeto 129 ms; la diferencia son 83 ms para 2M objetos.

Dos avisos sobre el método, por si alguien repite la medición:

* **El desglose anidado se mide a sí mismo.** Con los sub-tramos activos el
  helper "cuesta" 218 ciclos; sin ellos, 57. Los 161 de diferencia son las ocho
  lecturas de `rdtsc`. Por eso hay dos niveles (`VARN_ALLOC_PROFILE=1` para
  totales, `=2` para desglose) y el total sólo se lee del nivel 1.
* Con el nivel correcto, los sub-tramos **suman el total**. No hay coste sin
  explicar dentro del helper: el "resto" que aparecía era el artefacto.

### Lo que esto cambia respecto a la intuición inicial

**El cruce vale casi tres veces más que la representación del heap.** La Fase 1
ataca 24,5 ns de cruce más 5,9 ns de desperdicio puro (shape y closures, ambos
decidibles al compilar) = **30,4 ns**. La Fase 2 entera ataca 10,8 ns.

El plan mantiene el orden, pero la Fase 2 deja de ser el objetivo y pasa a ser
el remate: si la Fase 1 rinde lo que el reparto dice, el objetivo de ≤20 ns se
alcanza sin tocar la representación del heap ni romper ninguna garantía.

## 2. La tesis

El coste es **estructural, no algorítmico**. Un objeto joven hoy es:

```
VmValue → índice → base + idx*48 → HeapObj (enum, 48 B) → Rc<ObjData> → header (3 words) → campo
```

**Cuatro indirecciones para leer un campo, y dos asignaciones por objeto**: la
entrada de 48 bytes en el `Vec<Option<HeapObj>>` del heap, más el `Rc<ObjData>`
que sale del allocator. El `Vec` existe sólo para que los índices sean estables.

Y el camino de creación, por objeto: volcar los campos al frame (`store_home`),
cruzar al helper con `call_indirect`, resolver la shape (`RefCell` + búsqueda +
`Rc::clone`), recorrer los campos buscando closures, alocar el `ObjData`, copiar
los campos dentro, mover un `HeapObj` de 48 bytes, empujarlo a **dos** `Vec`
(`objects` y `forwarding`), y volver.

Ninguna constante de ese camino es el problema. La forma lo es.

**Tesis: la generación joven debe ser una arena de bump, con los objetos
inline, sin tabla de indirección y sin `Rc`.** Alocar pasa a ser comparar un
puntero contra un límite, escribir la cabecera y avanzar — emitible inline por
el JIT, sin cruzar a Rust.

---

## Fase 0 — Atribución — HECHA

Implementada en `varn-vm/src/alloc_profile.rs`, apagada por defecto y sin coste
medible apagada (la bandera es un atómico global; como `thread_local` costaba un
6 % con el perfilado apagado). Resultados arriba.

**Sin esto el resto era adivinar.** Esta máquina no resuelve diferencias menores
del 10 % (deriva del 36 % entre corridas, ±23 % entre repeticiones), y las
piezas que quedan valen 10-15 ns cada una sobre 60.

Contadores de ciclos (`rdtsc`, no `Instant` — su overhead de ~25 ns ahogaría lo
medido) alrededor de los tramos de `jit_build_object_with_shape` y del camino de
`construct`: cruce, resolución de shape, barrido de closures, `ObjData::alloc`,
copia de campos, `try_alloc`, retorno. Detrás de una env var, coste cero
apagado.

**Criterio de salida**: una tabla que sume ~60 ns y diga cuánto vale cada tramo.
Si el reparto contradice la tesis —por ejemplo, si el cruce al helper resulta
ser 5 ns y no 20— **el plan cambia aquí**, no después.

**Coste**: bajo. **Riesgo**: ninguno.

## Fase 1 — Alocación inline en el código compilado

Sin tocar todavía la representación. El JIT emite la alocación en vez de llamar
al helper: comprobar espacio, escribir, avanzar; salto al helper sólo cuando el
nursery está lleno.

Esto ya estaba previsto y abandonado a medias: `jit_layout.rs` y `nursery.rs`
exponen `objects_vec_byte_offset`, `forwarding_vec_byte_offset`,
`nursery_alloc_count_byte_offset_from_rcbox` y citan un `emit_nursery_alloc`
**que no existe**. Los offsets están probados contra un heap vivo en
`ExecCtx::new`.

Elimina: el `call_indirect`, el `store_home` de cada campo al frame, la
resolución de shape en runtime (el JIT la resuelve al compilar y hornea el
puntero), y el barrido de closures (decidible al compilar por los tipos).

**Criterio de aceptación**: −15 ns o más en el micro de alocación, con A/B
pareado. Si da menos de 10, la Fase 0 mintió: parar y revisar.

**Riesgo**: medio. El código inline debe respetar el safepoint de back-edge y
no dejar objetos a medio inicializar visibles para el GC. **Reversible**: es
aditivo, el helper se queda como camino lento.

## Fase 2 — La generación joven es una arena, no una tabla

El cambio de fondo.

`Nursery` deja de ser `Vec<Option<HeapObj>>` + `Vec<Option<u32>>` y pasa a ser
un bloque de bytes con un puntero de bump. Un objeto joven se escribe inline:

```
[ shape_ptr | len | tag | campo0 | campo1 | ... ]
```

Un `VmValue` de heap joven codifica un **offset dentro de la arena**, no un
índice de tabla. Leer un campo pasa de cuatro indirecciones a **una suma y una
carga**.

Al promover, el GC menor copia el objeto a la generación vieja, que conserva la
representación actual (`Rc<ObjData>`): sólo el 11-20 % de los objetos llega ahí
en cargas reales, y ahí la estabilidad de puntero sí importa.

Elimina de un golpe: el `malloc` por objeto (5,5 ns), el `HeapObj` de 48 bytes y
su movimiento, los dos `push`, y dos niveles de indirección.

**Criterio de aceptación**: ≤ 20 ns por objeto en el micro, y `tests/main.vn`
1159/0 en la matriz de 4.

**Riesgo**: alto. Es el paso que justifica el plan y el que puede romper cosas
sutiles.

### Lo que esta fase rompe, y con qué se sustituye

* **«El objeto nunca se mueve en memoria»** (`VM_ARCHITECTURE.md` §4). Deja de
  valer para la generación joven: promover copia. Lo sustituye la regla de que
  **el host nunca recibe punteros crudos a objetos jóvenes**: una nativa que
  guarde una referencia fuerza la promoción del objeto (pin), y el
  `HOST_BOUNDARY_SPEC` pasa a decirlo explícitamente. Hay que auditar
  `varn-builtins` en busca de punteros retenidos.
* **El formato de `VmValue` para referencias de heap**: offset+generación en vez
  de índice. Toca NaN-boxing, el JIT y todo lo serializado que contenga
  handles. Invalida artefactos: lo cubre `BUILD_FINGERPRINT`, que ya observa
  `varn-types`.
* **`HeapObj` deja de ser el tipo de la generación joven.** Los `HeapObj`
  restantes (Map, Set, Task, Closure…) siguen en la tabla del old gen; sólo
  objetos, records y arrays pequeños entran en la arena.

### Lo que hay que resolver antes de escribir código

* **Punteros horneados por el JIT.** Ya existe invalidación por epoch
  (`jit_epoch`, `jit_ancestry`); hay que comprobar que cubre offsets de arena, y
  el bug abierto de `bench_http_routing` sugiere que la cobertura actual **no es
  completa** (ver `bench-jit-snapshot-corruption` en las notas del proyecto).
  **Ese bug se arregla antes de la Fase 2, no después**: entrar aquí con una
  fuga de raíces conocida es garantizar semanas de depuración.
* **Barrera de escritura old→young** con offsets en vez de índices.
* **Objetos que nacen en old gen** con la nursery llena: hoy hay un camino
  (`holds_nursery_ref`), tiene que seguir existiendo.

## Fase 3 — Escritura única en los constructores

`new P(a,b,c)` hoy escribe cada campo **dos veces**: `ObjData::alloc` inicializa
los N campos a `null` y el constructor los sobreescribe. Con la arena, la
alocación inline puede escribir los valores definitivos de una vez cuando el
constructor es trivial (sólo asignaciones `this.x = arg`), que es la forma
dominante.

También cae aquí la basura del camino de `construct.rs`: `RefCell::borrow` del
`ctor_rt_cache`, `downcast` de un `Rc<dyn Any>` y tres `Rc::clone` por objeto.
Cachear el `Rc<VmClosure>` ya tipado en vez de `Rc<dyn Any>` quita el downcast.

**Criterio de aceptación**: el caso de clase iguala al literal.
**Riesgo**: bajo, y es independiente de la Fase 2.

---

## 3. Orden, y por qué

```
0 (atribución)  →  1 (inline)  →  [arreglar bench_http_routing]  →  2 (arena)  →  3 (constructores)
```

La Fase 1 es reversible y da la mayor parte del beneficio si el cruce al helper
resulta ser el tramo gordo. La Fase 2 sólo se aborda si la Fase 0 la respalda y
el bug de raíces del JIT está cerrado.

La Fase 3 puede adelantarse: no depende de las otras.

## 4. Cómo se verifica cada paso

* **Corrección**: `tests/main.vn` 1159/0 en la matriz de 4 —{árbol `std/`,
  `VARN_STD=@embedded`} × {JIT, `VARN_NO_JIT=1`}— con `vn cache clean` entre
  corridas. No negociable en ninguna fase.
* **Rendimiento**: A/B **pareado y alternado**, medianas de 5, caché purgada
  antes de cada corrida. Nunca wall-clock entre corridas separadas: en esta
  máquina eso miente en un 36 %.
* **Micro de referencia**: 2M objetos, con constructor y como literal, con y sin
  retención. Es lo que produjo todas las cifras de este documento.
* **Regresión de arranque**: el arranque en frío (~10 ms) es una ventaja de
  Varn frente a Bun (42 ms). Cualquier fase que lo empeore paga un precio que
  hay que justificar aparte.

## 5. Lo que este plan NO hace

* **No toca el allocator.** mimalloc ya se llevó lo que había ahí: `ObjData::alloc`
  son 5,5 ns de 60. Un arena que sólo sustituya al `malloc` no puede recuperar
  más del 9 %.
* **No persigue el acceso a campos.** Empata con Bun hoy (40 ms contra 39 en 2M
  lecturas). La Fase 2 lo mejora de rebote al quitar indirecciones, pero no es
  el objetivo.
* **No toca el tiering del JIT.** Es un frente distinto y medido aparte: 46 ms
  de los 324 en `tests/main.vn`, con los benchmarks calientes indiferentes.
