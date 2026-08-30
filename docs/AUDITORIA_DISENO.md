# Auditoría de diseño: qué se pudo hacer de otra forma

Escrita tras una sesión larga midiendo y depurando el runtime. Cada crítica va
con la evidencia que la sostiene; donde no la hay, se dice.

No es una lista de errores. Tres de las cuatro decisiones discutibles son
elecciones defendibles cuyo precio ahora está medido, y una es un cambio que el
proyecto ya tiene a medio camino.

---

## 1. Lo que está bien y no tocaría

Antes de lo demás, porque condiciona el resto:

* **VM de registros con tipos del checker llegando al backend.** Es la mejor
  decisión del proyecto. `GetFixedField` con la forma probada, `ArrayGetIndex`
  sobre arrays tipados, `register_meta` con `Int`/`Float` desboxados: el
  frontend no tira su trabajo a la basura. Es lo que hace que leer un campo
  **empate con Bun** (40 ms contra 39 en 2M lecturas) pese a todo lo que se
  critica abajo.
* **Arranque de ~10 ms** contra los 42 de Bun y 55 de Node. Cuatro veces mejor,
  y es una ventaja estructural para scripts y CLI. Cualquier cambio que la
  erosione paga un precio que hay que justificar aparte.
* **Objeto DST en una sola asignación** (`ObjData`): cabecera y campos
  contiguos. Correcto, y medido: copiar los campos es gratis frente al `malloc`.
* **Tiering con OSR.** Sin OSR, una función que entra una vez y itera corre
  interpretada de principio a fin — medido en el propio código: 20x.

---

## 2. Referencia de heap: índice a una tabla, no puntero

**Lo que hay.** `VmValue` es NaN-boxing de 64 bits: 48 bits de payload, de los
que se usan **32 para un índice** en `Vec<Option<HeapObj>>`.

**Lo que se pudo hacer.** Un puntero canónico de x86-64 usa 47 bits. **Cabía en
el payload**, que es lo que hace JSC. La tabla no era una necesidad del formato,
fue una elección.

**El precio, medido en esta sesión:**

* Leer un campo atraviesa **cuatro indirecciones**:
  `VmValue → índice → base+idx*48 → HeapObj (48 B) → Rc<ObjData> → campo`.
  Con puntero serían una o dos.
* **Dos asignaciones por objeto**: la entrada de 48 bytes en la tabla y el
  `Rc<ObjData>`. La tabla existe sólo para que los índices sean estables.
* Una familia entera de fallos que con punteros no existe: un índice obsoleto es
  un número perfectamente válido que apunta a otro objeto. Tres rondas de
  depuración esta sesión persiguiendo uno, y el centinela de error de `evacuate`
  (`pack_old_idx(0)`) es literalmente un índice válido usado como «fallo».

**Por qué se eligió, casi seguro.** Un GC de copia con punteros crudos en Rust
exige `unsafe` en cada acceso y una disciplina que el compilador no verifica.
Los índices son la salida idiomática: sin UB, comprobables, y el `Vec` da al
mark-sweep una forma trivial de recorrer todo lo vivo.

**Veredicto.** Defendible, y hoy es la restricción de fondo del rendimiento con
objetos. Si se rehiciera, la pregunta correcta no es «índice o puntero» sino
**dónde** hace falta la estabilidad: sólo la frontera del host la necesita. Un
puntero directo dentro de la VM más un handle indirecto para lo que cruza al
host da lo mejor de ambos, a cambio de un `unsafe` acotado y auditado.

## 3. Las raíces del GC se enumeran a mano

**Lo que hay.** `ExecCtx::run_minor_gc` construye la lista de raíces nombrando
nueve estructuras (`stack`, `globals`, `modules`, `module_exports`,
`static_closures`, `pending_constructors`, `pending_setters`, `vm_suspend`,
`metadata`), las copia a un vector plano, colecta y las escribe de vuelta por
rangos.

**El problema.** Cualquier campo nuevo del `ExecCtx` que guarde un `VmValue` es
una fuga silenciosa hasta que alguien se acuerde de añadirlo. No es hipotético:
`proto_constants` guarda `Rc<Vec<VmValue>>` y **no está en esa lista** — resultó
no ser la causa del bug que investigaba, pero la omisión es real.

**Lo que se pudo hacer.** Que ser una raíz sea una propiedad del tipo, no de una
lista: un `Rooted<T>` que se registre al construirse y se dé de baja al
destruirse (shadow stack), o un único registro central por el que pase todo
`VmValue` que viva fuera de la pila. Cuesta algo de ergonomía y elimina la clase
entera de fallos.

**Veredicto.** Es la deuda más barata de pagar de las cuatro, y la que más
fallos futuros evita.

## 4. Los safepoints vuelcan registros a mano, teniendo stack maps al lado

**Lo que hay.** En cada safepoint, `live_boxed` decide qué registros están vivos
—con una liveness propia, alimentada por `bytecode::decode`— y `flush_boxed` los
escribe a los home slots del frame para que el GC los vea.

**Lo que ya está a medio camino.** Cranelift **emite stack maps**, y el proyecto
**ya los captura**… sólo para compararlos consigo mismo en
`vn debug -p roots:diff`. El informe dice, con todas las letras: *"109 raíces en
registro — lo que los stack maps tendrían que rootear tras el cutover"*.

**El precio.** El bug abierto de `bench_http_routing` es exactamente esto: un
registro que el flush no vuelca conserva su valor pre-colección y acaba metiendo
un índice muerto en un objeto vivo. Y la corrección depende de que la liveness
propia sea perfecta: dos bugs de esta sesión nacen de `decode`.

**Veredicto.** No es un rediseño, es terminar un cutover que ya está planificado
y con la instrumentación de verificación construida. Es el cambio con mejor
relación coste/beneficio del documento.

## 5. `decode` como fuente única de `def`/`uses`

**Lo que hay.** Un decodificador que describe cada instrucción, y del que beben
el register allocator, la liveness del JIT, el escáner de raíces y el
disassembler.

**Que sea única es lo correcto** — la alternativa (cada consumidor con su tabla
de anchuras) ya causó bugs que el propio archivo documenta.

**El problema es que no se verifica contra nadie.** Un `use` que falte ahí no da
error: da un registro dado por muerto, y el fallo aparece a miles de
instrucciones de distancia. Dos bugs de esta sesión salen de ahí — el de
register allocation (arreglado) y, muy probablemente, el de raíces (abierto).

**Lo que se pudo hacer.** Una comprobación diferencial: ejecutar el intérprete
con instrumentación que registre qué registros lee y escribe de verdad cada
opcode, y contrastarlo con lo que `decode` declara. Es una prueba que se escribe
una vez y cubre toda la clase.

---

## 6. Lo que NO cambiaría aunque tiente

* **Cranelift en vez de un backend propio.** Compilar cuesta 746 µs por función
  y el 100 % de las funciones rutean. El problema medido nunca fue la calidad
  del código generado.
* **El GC generacional.** Se le culpó del coste de alocar y se midió: en un
  micro donde todo muere joven, 41 promociones en 41 ciclos — el colector no
  hace trabajo y el tiempo sigue ahí.
* **La caché de bytecode en disco.** Bien concebida; sus fallos eran de
  invalidación, ya corregidos.
* **`Rc<ObjData>` para la generación vieja.** Ahí la estabilidad de puntero sí
  se necesita, y el coste sólo lo pagan los objetos que sobreviven.

---

## 7. Si sólo se pudiera cambiar una cosa

**Terminar el cutover a stack maps** (§4). No es un rediseño, cierra el bug
abierto, elimina la dependencia de que la liveness propia sea perfecta, y la
instrumentación para verificarlo ya existe.

La representación del heap (§2) es la que más rendimiento tiene detrás, pero es
un rediseño con breaking changes y su beneficio **no se puede estimar sin
construirlo** — ver `PLAN_ALOCACION.md` y `HIPOTESIS_DESCARTADAS.md`.
