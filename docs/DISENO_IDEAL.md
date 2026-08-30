# El deber ser: runtime para un lenguaje tipado sobre CPUs modernas

Diseño de referencia, sin concesiones a lo que hay hoy. Sirve como norte: cada
decisión del runtime actual puede contrastarse con esto y decidir si su
diferencia está justificada.

---

## 0. El error de origen

**Varn copió la arquitectura de un motor de JavaScript, y Varn no es
JavaScript.**

NaN-boxing, shapes, inline caches polimórficos, resolución de propiedades por
nombre: todo eso existe porque un motor de JS **no sabe los tipos hasta que
ejecuta**. Son soluciones brillantes a un problema que Varn no tiene — el
checker ya probó el tipo de cada expresión antes de emitir un solo opcode.

La consecuencia se paga en cada operación. Hoy:

* Un campo declarado `int` se guarda **NaN-boxed** (`ObjData.values` es
  `[Cell<VmValue>]`), así que leerlo exige desempaquetar un entero cuyo tipo era
  conocido en compilación.
* Leer un campo con la clase probada baja a **~15 instrucciones y 3 saltos
  condicionales**: comprobar el tag NaN-box, comprobar el discriminante del
  `HeapObj`, comprobar el número de campos. Tres guardas de una información que
  el checker ya tenía.
* Un valor de heap es un **índice de 32 bits a una tabla**, así que llegar al
  campo son cuatro indirecciones.

El referente correcto no es V8 ni JSC. Es **Go, C# o Swift**: runtimes con
recolección de basura para lenguajes donde el tipo se conoce y la representación
lo aprovecha.

**Principio rector: el tipo estático es la información más cara de obtener y la
más barata de tirar. No tirarla en ningún punto entre el checker y el
registro de la CPU.**

---

## 1. Representación de valores

### Regla

**Cada valor viaja en su forma nativa. El boxing existe sólo donde el tipo es
`dynamic`, y no se propaga fuera de ahí.**

| Tipo del checker | Representación | En Cranelift |
|---|---|---|
| `int` | i64 nativo, sin tag | `types::I64` |
| `float` | f64 nativo | `types::F64` |
| `bool` | 1 byte | `types::I8` |
| `char` | u32 | `types::I32` |
| clase / array / str | **puntero directo** | `types::I64` |
| `T?` | puntero nulable, o (valor, bit) para escalares | par de valores |
| `dynamic` | valor etiquetado de 128 bits: (tag, payload) | dos `I64` |

Dos consecuencias que importan:

**No hay NaN-boxing.** Con tipos estáticos no compra nada y cobra en cada
operación. Y para `dynamic`, **128 bits (tag + payload) es mejor que 64
NaN-boxed**: no hay máscaras ni desplazamientos, el payload puede ser un puntero
de 64 bits íntegro, y en x86-64 dos registros son tan baratos como uno. NaN-box
optimiza el tamaño de una representación que ya no es el caso común.

**Los enteros no pierden bits.** Los i48 actuales fuerzan un sign-extend en cada
lectura y obligan a un camino aparte para desbordamiento. Un i64 nativo es lo
que la CPU hace de todos modos.

### Nulabilidad

`T?` sobre un puntero usa el patrón nulo — gratis. Sobre un escalar, un par
`(valor, bool)` que Cranelift pasa en dos registros; el checker ya sabe cuál es
cuál, así que nunca hay que preguntarlo en runtime.

---

## 2. Layout de objetos

### Regla

**Una clase con campos declarados es un struct de C, no un diccionario.**

```
class P { a: int; b: float; c: bool }

  offset 0   : header (tipo + marca de GC, 8 bytes)
  offset 8   : a   i64
  offset 16  : b   f64
  offset 24  : c   i8   (+7 de padding, o empaquetado con otros bools)
```

Leer `p.a` es **una carga con desplazamiento constante**. Sin tag que
comprobar, sin discriminante de `HeapObj`, sin bounds check contra el número de
campos, sin desempaquetar: el checker probó la clase, el offset es una constante
de compilación y el tipo del campo es `I64`.

De 15 instrucciones y 3 saltos a **una instrucción y cero saltos**.

### Corolarios

* **Campos ordenados por tamaño y alineación**, no por orden de declaración.
  Menos padding, menos líneas de caché por objeto.
* **`bool` empaquetados en un mapa de bits** cuando hay varios.
* **Sin shape en el objeto.** La clase es el tipo; el descriptor vive una vez
  por clase, no una referencia por instancia. Un objeto de tres campos ocupa
  32 bytes, no 32 + cabecera de shape + entrada de tabla.
* **Herencia por prefijo**: los campos de la base primero, así un puntero a la
  derivada vale como puntero a la base sin conversión.
* **Objetos literales sin tipo nominal** (`{ a: 1 }`) siguen necesitando
  descriptor: son un tipo estructural anónimo que el checker sintetiza, y su
  layout se calcula igual en compilación.

### Lo dinámico, aparte

`dynamic` y los objetos con claves calculadas necesitan búsqueda por nombre.
Ahí sí: tabla hash por objeto, con inline caches. **Pero es un camino separado
que no contamina la representación de todo lo demás** — que es exactamente el
error de hoy.

---

## 3. Arrays y colecciones

**`T[]` es un buffer de `T`, no de valores etiquetados.**

```
int[]    →  header + len + cap + [i64; n]
float[]  →  header + len + cap + [f64; n]
P[]      →  header + len + cap + [*P; n]
```

Sumar un `int[]` pasa a ser un bucle sobre i64 contiguos: **vectorizable**.
Cranelift no auto-vectoriza hoy, pero el layout es la precondición — sin él, ni
ese backend ni ninguno futuro puede hacerlo, y hoy es imposible porque cada
elemento es un `VmValue` que hay que destaguear.

Y el efecto de caché es mayor que el de instrucciones: un `float[]` denso mete
8 elementos por línea de caché en vez de 8 valores etiquetados que hay que
desempaquetar uno a uno.

---

## 4. Memoria y recolección

### Punteros, no índices

Los valores de heap son **punteros directos**. La estabilidad que hoy da la
tabla de índices sólo hace falta en un sitio —la frontera del host— y ahí se
resuelve con handles explícitos, no gravando cada acceso del programa.

### Trazado preciso por tipo

Con tipos estáticos, **el GC no necesita inspeccionar valores**. Cada clase
publica su mapa de punteros —qué offsets son referencias— calculado en
compilación. Trazar un objeto es recorrer una lista corta de offsets, no
comprobar el tag de cada campo.

Eso es lo que hace un GC de Go o C# rápido, y hoy es imposible porque cualquier
campo puede ser cualquier cosa.

### Raíces por stack maps, no por enumeración

**Cranelift emite stack maps.** El GC pregunta al stack map qué ranuras del
frame son referencias vivas en ese punto exacto.

Se acaban dos clases de fallo de golpe: la lista de raíces enumerada a mano
—donde cualquier estructura nueva es una fuga silenciosa— y el volcado manual de
registros con liveness propia, que es el bug abierto de `bench_http_routing`.

### Generacional, de copia, con arena

Nursery como arena de bump. Alocar es comparar un puntero contra un límite,
escribir la cabecera y avanzar — **emitible inline por Cranelift**, sin cruzar a
Rust. Promover copia al espacio viejo y reescribe punteros usando los stack maps
y los mapas de tipo.

Con el layout nativo, el objeto de tres campos que hoy cuesta 60 ns debería
costar el bump (≈2 ns) más escribir los campos.

---

## 5. Qué permite Cranelift que hoy no se usa

* **Tipos nativos.** Hoy casi todo cruza como `I64` porque todo es un `VmValue`.
  Con representación nativa, `F64` viaja en registro XMM sin bitcast y `I8` no
  ocupa una palabra.
* **Stack maps para GC.** Ya se capturan… sólo para compararse consigo mismos en
  `roots:diff`.
* **Llamadas directas.** Con tipos conocidos, una llamada a método no virtual es
  un `call` directo; hoy pasa por resolución dinámica.
* **Alocación inline.** Con arena de bump, sin helper.
* **Menos saltos.** Las tres guardas por acceso a campo desaparecen. En un
  núcleo con predictor, un salto que siempre acierta cuesta poco, pero ocupa
  entradas del BTB y rompe bloques básicos que el planificador podría fusionar.

---

## 6. Lo que este diseño cuesta

Honestamente:

* **`dynamic` se vuelve el ciudadano de segunda.** Hoy todo es uniforme y
  `dynamic` no cuesta más que lo demás. En el diseño ideal, cruzar a `dynamic`
  cuesta boxing explícito. Es el intercambio correcto para un lenguaje donde
  `dynamic` es la excepción, y desastroso si es la regla.
* **La frontera del host se complica.** Las nativas dejan de recibir un
  `VmValue` uniforme y pasan a recibir tipos concretos, o handles. Más superficie
  de API, más generación de código.
* **`unsafe` acotado.** Punteros directos con GC de copia en Rust exigen
  disciplina que el compilador no verifica. La mitigación no es evitarlo, es
  **concentrarlo**: un módulo de acceso al heap, auditado, y nada de punteros
  crudos fuera de él.
* **Es un rediseño, no un refactor.** Toca `VmValue`, el layout de objetos, el
  GC, la frontera del host y todo el backend del JIT. La caché de bytecode se
  invalida entera.

---

## 7. Distancia desde donde está hoy

Ordenado por (beneficio / riesgo), y cada paso tiene sentido por sí solo:

1. **Stack maps para las raíces del GC.** Independiente del resto, cierra el bug
   abierto y elimina la lista enumerada a mano. Ya está a medio construir.
2. **Campos de escalares sin box.** Un campo `int`/`float`/`bool` en una clase
   con forma probada guarda el valor nativo. No toca `VmValue` en registros ni
   el GC: sólo el layout de `ObjData` y los dos opcodes de acceso. **Es el
   primer paso que rompe el techo medido**, porque quita las guardas y el
   desempaquetado del camino caliente.
3. **Arrays tipados con buffer nativo.** Misma idea sobre `T[]`. Habilita la
   vectorización futura.
4. **Punteros en vez de índices**, con handles sólo en la frontera del host.
5. **Arena de bump con alocación inline.**

Los pasos 2 y 3 son los que más rendimiento devuelven por unidad de riesgo, y
ninguno exige tocar `VmValue`. El paso 4 es el que convierte esto en otro
runtime.

---

## 8. La frase corta

**Hoy Varn paga el precio de un lenguaje dinámico y sólo cobra las ventajas de
uno tipado en el frontend.** El deber ser es que el tipo llegue intacto hasta el
registro de la CPU: representación nativa, offsets constantes, trazado preciso y
raíces del propio compilador.
