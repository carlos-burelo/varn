# Auditoría: qué prueba el sistema de tipos y qué hace el runtime con ello

Complementa `AUDITORIA_DISENO.md` (que audita el runtime) y `DISENO_IDEAL.md`
(que describe el norte). Este documento audita **el canal**: cuánta de la
información que el checker prueba llega viva al código máquina, dónde muere y
qué línea exacta la mata.

Todo lo que sigue está medido sobre `target/release/vn.exe` y el corpus de
`tests/` y `tests/benchmarks/`. Donde hay un número, hay un comando que lo
produce.


---

## Estado (actualizado tras aplicar los pasos 0 y 1)

Este documento se escribió como diagnóstico. Parte de lo que describe ya está
arreglado; los números "antes" se conservan porque son la evidencia, no el
estado.

| Sección | Estado |
|---|---|
| §1 `int` da números falsos | **CORREGIDO.** `VmValue` es ahora `(tag, payload)` de 128 bits e `int` es un `i64` nativo. Todos los casos de la tabla dan el resultado correcto. i48 borrado de la semántica, del checker, del plegador, de `int.MAX_VALUE` y de los tests. |
| §2.1 `global` sin tipo | **CORREGIDO.** `HirBinding::Global` lleva su `HirType`. De 2 470 valores `Dynamic` a 182. |
| §2.2 `new C(...)` sin tipo | **CORREGIDO.** El checker anota la expresión `New`; el lowering la propaga. |
| §2.3 `CgTy::Fn` → `Dynamic` | **CORREGIDO.** Proyecta a `HirType::Ref`: un closure es una referencia de heap, y eso basta para sacar cada lectura de función de un registro dinámico. |
| **Cobertura de tipos del corpus** | **41 % `Dynamic` → 22 %.** Medido igual: `vn debug -p ssa` sobre `tests/*.vn`. |
| §2.3 `Nullable(T)` → `Dynamic` | Pendiente (paso 6). |
| §3 shapes/overflow en objetos tipados | Pendiente (paso 7). |
| §3.1 `InvokeVirtual` sin productor | Pendiente (paso 4). |
| §4 las 20 instrucciones por lectura de campo | Pendiente (paso 3). |
| §5 tipos sin verificar en el SSA | Pendiente (paso 2). |

**Coste presente del cambio de representación:** el backend CLIF está apagado
(`PAIR_MIGRATION_PENDING` en `varn-jit/src/lib.rs`). Modela cada registro de la
VM como UNA palabra máquina, que es lo que el NaN-box permitía; con un valor de
dos palabras hay que migrar `use_boxed`, los primitivos de box/unbox, el
direccionamiento de los home slots y la firma de cada helper de runtime — y en
Windows x64 un struct de 16 bytes se devuelve por puntero oculto, así que la
convención de retorno difiere de SysV y hay que declararla por plataforma. Todo
se interpreta mientras tanto: correcto, y ~40x más lento en código caliente
(`bench_fib`: 1,83 s contra 44 ms). Los primitivos que faltan están marcados con
`unimplemented!("… awaits the two-word value migration")`; son la lista de
tareas exacta.

La matriz de 4 cuadrantes pasa 1 167/1 167 en los cuatro.

---

## 0. La conclusión

La intuición de partida —«traté a Varn como JS en las optimizaciones»— es
correcta, pero se queda corta en un punto y se pasa en otro.

**Se queda corta:** el problema no es sólo que el runtime tenga maquinaria de
motor dinámico. Es que **el lenguaje prohíbe explícitamente el dinamismo que esa
maquinaria existe para soportar**. Las shapes, las transiciones de shape, el
slot de overflow y las inline caches sólo son alcanzables a través de
`dynamic` — el checker rechaza añadir un campo tanto a una instancia de clase
como a un literal de objeto (§3). El 100 % de los objetos paga por una capacidad
que el 100 % del código bien tipado no puede usar.

**Se pasa:** los cimientos del frontend están bien. El checker es estricto de
verdad, la inferencia funciona (una función sin tipo de retorno declarado
igualmente produce un valor tipado), los genéricos se instancian para el tipo de
retorno, y `CgTy` es una proyección honesta que nunca adivina. El problema no es
que los cimientos estén mal puestos. Es que **entre el checker y el registro de
la CPU hay cuatro puntos donde la información se tira, y tres de ellos son una
línea de código**.

Y hay un quinto problema que no es de rendimiento: la representación de valores
y la semántica del lenguaje se han separado, y hoy `int` da respuestas
silenciosamente incorrectas (§1).

---

## 1. El bug que demuestra la tesis: `int` da números falsos

Es el hallazgo más grave del documento y es de corrección, no de rendimiento.
Va primero porque además **es la tesis convertida en fallo**: una decisión de
representación heredada de los motores de JS (NaN-boxing) dicta el ancho de un
entero en un lenguaje tipado, y cuando la semántica se actualizó sin actualizar
la representación, el resultado fue silencio y números equivocados.

### Lo observado

```
$ cat /tmp/edge.vn
let m: int = 140737488355327   // int.MAX_VALUE
print(m + 1)
let big: int = 9223372036854775807
print(big)

$ vn run /tmp/edge.vn
-140737488355328
-1
```

* `int.MAX_VALUE + 1` **envuelve en silencio**. No lanza.
* El literal `9223372036854775807` —que el checker acepta— **imprime `-1`**.

Más casos, idénticos con JIT y con `VARN_NO_JIT=1`, y también cuando el plegador
de constantes resuelve la expresión en compilación:

| Expresión | Correcto | Varn imprime |
|---|---|---|
| `100000000000000 + 100000000000000` | `200000000000000` | `-81474976710656` |
| `100000000000000 * 3` | `300000000000000` | `18525023289344` |
| `1000000007 * 1000000007` | `1000000014000000049` | `-80578252960719` |

El último es literalmente el ejemplo que `tests/53-int48-overflow.vn` cita en su
cabecera como aquello que el diseño **no** debe hacer:

> *«Un `int` de 48 bits es un diseño legítimo; uno que responde
> `1000000007 * 1000000007` con un número equivocado y sin señal, no.»*

### La causa

Tres capas afirman tres cosas distintas:

| Capa | Qué dice | Dónde |
|---|---|---|
| Semántica | `int` es i64, desborda en `[-2^63, 2^63-1]` | `varn-core/src/numeric.rs:39` — `INT_MAX = i64::MAX` |
| Representación | `int` son 48 bits, el resto se trunca | `varn-types/src/vm_value.rs` — `from_int` enmascara con `MASK_INT48`; `as_int` extiende el signo desde el bit 47 |
| Biblioteca | `int.MAX_VALUE == 2^47-1` | `varn-builtins/src/modules/primitives/int/int.rs:6` |

`add_int` es `a.checked_add(b)` sobre i64: para `2^47` devuelve `Some`, y
`VmValue::from_int` trunca ese `Some` a 48 bits sin decir nada. La comprobación
de rango existe, pero comprueba el rango equivocado, así que nunca dispara en la
franja donde la representación sí pierde bits.

Es una regresión. El commit `bbd8ae3 fix(core)!: raise on int overflow instead
of wrapping` estableció el contrato de lanzar; `917dcdd refactor(core): change
form i48 representation to i64` movió la semántica a i64 sin mover la
representación, y con eso deshizo el commit anterior en todo el rango
`[2^47, 2^63)`.

### Las dos salidas

1. **Volver a i48 de verdad**: `INT_MAX = (1<<47)-1`, `checked_int` comprueba
   ese rango, `from_int` deja de enmascarar (o asevera). Es media tarde y
   restaura el contrato que los tests ya describen.
2. **Ir a i64 de verdad**, que es lo que dice el nombre del commit y lo que
   quiere `DISENO_IDEAL.md`: un i64 no cabe en un payload NaN-box, así que
   exige que un registro tipado `Int` viaje desboxado. Es el paso 3 del plan de
   §7 y resuelve esto como efecto secundario.

**Ninguna de las dos es opcional.** Hoy el lenguaje miente sobre la aritmética
de enteros, y ese es el tipo de fallo que hace que nadie confíe en un runtime.

---

## 2. Medición: dónde muere el tipo

`vn debug -p ssa` imprime el tipo de cada valor SSA (omite los `Dynamic`). Sobre
todo el corpus de `tests/*.vn`:

```
$ for f in tests/*.vn; do vn debug -p ssa "$f" 2>&1; done | ...
corpus: typed=7929 dynamic=5559  ->  41% Dynamic
```

**41 % de los valores SSA son `Dynamic`** en un corpus escrito en un lenguaje
donde el checker rechaza asignar `str` a `int`. Por benchmark:

| Benchmark | % Dynamic |
|---|---|
| `bench_collection_pipeline` | 12 % |
| `bench_dto` | 18 % |
| `bench_matrix` | 18 % |
| `bench_http_routing` | 25 % |
| `bench_json` | 31 % |
| `bench_fib` | 33 % |
| `bench_csv_etl` | 36 % |

### Qué instrucción produce cada valor Dynamic

Agrupando los 5 559 del corpus por la instrucción que los define:

| Instrucción | Cantidad | % |
|---|---:|---:|
| `global` (leer un global) | 2 470 | 44 % |
| `call` | 1 799 | 32 % |
| `moduleslot` (leer un import) | 275 | 5 % |
| `getprop` | 148 | 3 % |
| `callmethod` | 147 | 3 % |
| `add.dyn` | 137 | 2 % |
| `nativeop#<id>` | 118 | 2 % |
| resto (`await`, `arraygetindex`, `try`, `yield`…) | 465 | 8 % |

**Los dos primeros son el 76 %.** Y los dos son plomería, no una limitación del
checker.

### 2.1 `global` — 44 % de toda la pérdida, dos líneas

```rust
// varn-compiler/src/ssa/build/mod.rs:241
HirBinding::Global(name) => {
    return Ok(self.emit(InstKind::LoadGlobal(name.clone()), HirType::Dynamic));
}
HirBinding::Upvalue(uv) => {
    return Ok(self.emit(InstKind::LoadUpvalue(*uv), HirType::Dynamic));
}
```

`HirType::Dynamic` está **escrito a mano**. Y no podría ser otra cosa, porque
`HirBinding::Global(Rc<str>)` sólo lleva el nombre: el tipo no viaja en el
binding. El checker sí lo sabe — anota `cg_ty` en cada `Identifier`.

Se ve en cualquier programa:

```
const s = "n="          // el checker prueba: str
...
v7 = global ...::s      // Dynamic
v8 = add.dyn v7, v2     // y por eso la suma es dinámica
```

El efecto es en cascada. Es el origen de buena parte de los `add.dyn` y, sobre
todo, hace que **toda llamada a una función de nivel superior sea una llamada
indirecta a un valor sin tipo**:

```
0027 │ LoadGlobal │ r1 = global[2]  ; "...::withRet"
0029 │ LoadNull   │ r14
0030 │ Move       │ r15 = r2
0032 │ Call       │ r7 = call r1(2 args @ r14)
```

`withRet` es una función de módulo y no es reasignable — el checker lo prueba:
`g = 5` sobre `let g = f` da `error[VN3001]`. El destino está resuelto en
compilación y aun así se busca en la tabla de globales en cada llamada. Existe
`LoadStaticFn`, que se usa **al definir** la función, y ningún opcode para
llamarla directamente.

### 2.2 `call` — 32 %, y el caso más claro es `new`

Las llamadas a función **sí** propagan el tipo de retorno declarado, y también
el inferido:

```
v8:  int = call v6(v1)     // noRet(x)      — retorno inferido
v11: int = call v9(v1)     // withRet(x):int — retorno declarado
```

Pero:

```
const p = new P(1)         →  v2 = call v0(v1)      // Dynamic
const q: P = new P(2)      →  v5 = call v3(v4)      // Dynamic
```

**Construir una instancia produce `Dynamic`**, incluso con anotación explícita
`: P`. El tipo de `new P(...)` es `P` por definición: no requiere inferencia,
requiere no tirarlo. Y como el resultado es `Dynamic`, el registro que lo
contiene es `SlotKind::Dynamic`, se aloja boxeado, y cada acceso a campo tiene
que volver a probar en runtime lo que la palabra `new` ya decía.

### 2.3 El canal se estrecha una vez más antes del JIT

`ssa/emit/mod.rs:210` proyecta `HirType` (10 variantes, con carga) a `SlotKind`
(6, sin carga):

```rust
HirType::Ref | HirType::Array(_) | HirType::Map(_,_)
| HirType::Set(_) | HirType::Class(_)          => SlotKind::Ref,
HirType::Nullable(_) | HirType::Dynamic        => SlotKind::Dynamic,
```

Dos pérdidas concretas:

* **`Class(id)` → `Ref`.** La identidad de clase no llega al asignador de
  registros ni al JIT. Es exactamente el dato que hace falta para un offset
  constante o para una ranura de vtable.
* **`Nullable(T)` → `Dynamic`.** `int?` colapsa a completamente dinámico. La
  seguridad frente a nulos es una característica de cabecera de Varn
  (`tests/08`, `tests/30`, `tests/46`), y usarla desoptimiza el valor a fondo.
  Un `T?` sobre puntero es el patrón nulo (gratis); sobre escalar es un par
  `(valor, bit)`. Ninguno de los dos es «dinámico».

---

## 3. El runtime paga por un dinamismo que el lenguaje prohíbe

Esto es lo que convierte «copié la arquitectura de un motor de JS» de metáfora
en coste medible. Cada objeto de Varn lleva:

```rust
#[repr(C)]
pub struct ObjData<T: ?Sized = [Cell<VmValue>]> {
    shape: UnsafeCell<Rc<Shape>>,                     // 8 B
    inline_len: u32, _pad: u32,                       // 8 B
    overflow: UnsafeCell<Option<Box<Vec<VmValue>>>>,  // 8 B
    values: T,                                        // n × 8 B, NaN-boxed
}
```

Y `Shape` lleva `property_names: HashMap<RuntimeString, usize>` más
`transitions: RefCell<HashMap<RuntimeString, Rc<Shape>>>`.

`shape`, `overflow` y `transitions` existen para una sola cosa: que un objeto
pueda **crecer** en runtime. Comprobado contra el checker:

```
class P { a: int; ... }
const p = new P(1)
p.b = 5
→ error[VN3004]: property 'b' does not exist on type 'P'

const o = { a: 1 }
o.b = 2
→ error[VN3004]: property 'b' does not exist on type '{ a: int }'

let d: dynamic = { a: 1 }
d.b = 2
→ 2      ✔ único camino que llega a Shape::transition()
```

**Ni las instancias de clase ni los literales de objeto pueden crecer.** La
transición de shape sólo es alcanzable a través de `dynamic`.

El coste, para una clase de tres campos `int`:

| Concepto | Bytes |
|---|---:|
| Entrada en la tabla del heap (`HeapObj`; stride 0x30 confirmado en el asm de §4) | 48 |
| Contadores de `Rc` | 16 |
| Cabecera de `ObjData` (shape + len + overflow) | 24 |
| Los tres campos, NaN-boxed | 24 |
| **Total** | **112** |
| Datos reales | **24** |

4,6× de sobrecoste, y de la cabecera de 24 bytes, 16 son para una mutabilidad
que el checker no permite.

Lo mismo con el despacho. El lenguaje **exige el modificador `override`**
(`error[VN4101]` si falta), es decir: la jerarquía de clases es cerrada y
conocida en compilación, que es la precondición exacta de una vtable. Y sin
embargo:

```
function useShape(x: Shape): float { return x.area() }

fn useShape:
  b0(v0: dyn):                      ← un parámetro de tipo interfaz es Dynamic
    v1: float = callmethod v0.area

  0000 │ CallMethod │ r2 = r1.0(0 args @ r5) ; Literal(Str("area"))
```

Búsqueda **por cadena de texto**, con inline cache, para invocar un método que
una interfaz declara y que el checker verificó que existe.

### 3.1 Maquinaria construida y nunca usada

`OpCode::InvokeVirtual` está implementado en el intérprete
(`ops_control_calls.rs:198`), en el JIT (`clif/methods.rs`), en el escáner de
safepoints, en el desensamblador, en el codificador de bytecode y en el helper
de IC (`jit_helpers/ic.rs`).

**El compilador no lo emite nunca.** La única referencia en `varn-compiler` es
`regalloc_post/rewrite.rs:293`, que sólo renumera registros. El camino de
despacho virtual está construido de punta a punta y no tiene productor.

`passes/monomorphize.rs` tiene el problema simétrico: el nombre promete
monomorfización de genéricos, y lo que hace es especializar `GetIndex`/`SetIndex`
a `ArrayGetIndex`/`ArraySetIndex` cuando el objeto es un array. Es una pasada
útil y bien hecha, pero **no existe monomorfización de genéricos**: `class Box<T>`
se compila una vez, con `T` dinámico.

---

## 4. La evidencia en código máquina

Todo lo anterior se lee de una vez en el asm que emite Cranelift. Este bucle:

```
class P { a: int; constructor(a: int) { this.a = a } }
function sum(p: P, n: int): int {
  let t: int = 0; let i: int = 0
  while (i < n) { t = t + p.a; i = i + 1 }
  return t
}
```

`vn debug -p clif:asm --fn sum` produce, antes del bucle, la lectura de `p.a`
(LICM la izó correctamente — la pasada funciona):

```asm
and   r8,[0F0h]          ; comprobar tag NaN-box (¡constante desde memoria!)
cmp   r8,[0F8h]
jne   <helper>           ;   salto condicional 1
mov   r10,[rcx+90h]      ; base de la tabla del heap
mov   r9,[r10+0E8h]      ; base de la nursery
mov   r8,[r10+30h]
bt    rdx,1Fh            ; ¿índice de nursery o de generación vieja?
cmovb r8,r9
mov   r10,rdx
and   r10,7FFFFFFFh      ; extraer el índice
mov   r9,rdx
and   r9,[100h]
bt    rdx,1Fh
cmovb r9,r10
imul  r11,r9,30h         ; índice × 48 B  ← entrada de HeapObj
movzx rsi,byte ptr [r8+r11]
cmp   rsi,3              ; discriminante de HeapObj
jne   <helper>           ;   salto condicional 2
mov   rax,[r8+r11+8]     ; deref al Rc<ObjData>
mov   r8d,[rax+18h]      ; inline_len
test  r8d,r8d
jne   <helper>           ;   salto condicional 3
mov   rax,[rax+28h]      ; ← el campo, por fin
```

**~20 instrucciones, 3 saltos condicionales, un `imul` y 4 cargas** para leer el
campo 0 de un parámetro cuyo tipo está declarado `P`. Con layout nativo es
`mov rax,[rdi+8]`.

Detalle secundario pero revelador: las constantes del NaN-box (`[0F0h]`,
`[0F8h]`, `[100h]`) no son inmediatos, son **cargas del pool de constantes**.
Son de 64 bits, así que Cranelift no puede empotrarlas. Un tag más estrecho las
haría inmediatas.

### 4.1 El bucle desboxa una invariante en cada vuelta

El cuerpo del bucle, ya con `p.a` izado a `r9`:

```asm
add  rsi,1        ; i++
mov  rdi,r9       ; ← copia inútil
mov  rcx,rdi      ; ← copia inútil
shl  rcx,10h      ; ┐ extensión de signo i48
sar  rcx,10h      ; ┘   de un valor invariante del bucle
add  rax,rcx      ; t += a
jmp  <cabecera>
```

`r9` no cambia entre vueltas. Su desempaquetado i48 se repite igualmente:
**4 de las 7 instrucciones del cuerpo son trabajo redundante**, que existe sólo
porque el valor viaja boxeado. Con un registro `Int` desboxado el cuerpo es
`add rax,r9` más el incremento.

### 4.2 Un `float` se pasa por un registro de enteros

`raw_signature` (`varn-jit/src/clif/abi.rs:32`) declara **todos** los parámetros
y el retorno como `types::I64`. Un parámetro `float` llega en GPR con sus bits
boxeados:

```
function fsum(x: float, n: int): float { let t = 0.0; while (i<n) { t = t + x; ... } }
```

```asm
add    r9,1
vmovq  xmm6,rdx      ; ← GPR → XMM en CADA iteración, de un parámetro invariante
vaddsd xmm0,xmm0,xmm6
jmp    <cabecera>
```

`x` está declarado `float`. Cranelift admite `types::F64` en la firma y lo
pasaría en `xmm1`; el cuerpo sería `vaddsd xmm0,xmm0,xmm1`. En su lugar hay un
cruce de dominio entero↔flotante por vuelta.

---

## 5. Los cimientos: el tipo es una tabla lateral, no el IR

Ésta es la crítica estructural, y es la respuesta directa a «¿hice bien los
cimientos?».

El tipo no viaja **en** el programa. Viaja **al lado**:

```rust
pub struct TypeAnnotations {
    inner: HashMap<AnnKey, ExprAnnotation>,   // clave: AstId de la expresión
    ...
}
pub struct ExprAnnotation {
    pub cg_ty: Option<CgTy>,    // «ausente significa Dynamic»
    ...
}
```

Tres consecuencias, en orden de gravedad:

1. **La ausencia es indistinguible del fallo.** Si nadie llamó a
   `record_cg_ty_at` en una rama del anotador, el resultado es `Dynamic`: código
   correcto y lento, sin diagnóstico. Hay exactamente **nueve** sitios que anotan
   `cg_ty` en todo el checker (seis en `checker_annotations/exprs.rs`, tres en
   `stmts.rs`). Cada forma sintáctica que no pase por uno de esos nueve es una
   desoptimización silenciosa. Así se llega al 41 %.

2. **Los tipos del SSA no se verifican.** `ssa/verify.rs` comprueba forma SSA
   —definición única, dominancia— y no toca los tipos. Nada comprueba que un
   `add.int` reciba operandos `Int`, ni que el `HirType` declarado de un valor
   coincida con lo que la instrucción produce. Un tipo que **falta** cuesta
   rendimiento; un tipo **equivocado** sería un miscompile, y no hay ningún
   cable trampa para él.

   Es la misma clase de deuda que `AUDITORIA_DISENO.md` §5 señala sobre
   `decode`: una fuente única de verdad sin nada que la contraste. Ahí ya costó
   dos bugs.

3. **El canal se estrecha tres veces sin que nadie lleve la cuenta.**
   `TypeKind` (rico) → `CgTy` (12 variantes) → `HirType` (10) → `SlotKind` (6).
   Cada estrechamiento es defendible por separado; el efecto compuesto no lo
   mide nadie. Un contador de cobertura en `-p ssa` —el volcado ya distingue
   tipado de `Dynamic`, sólo falta el agregado— convierte esto en una métrica de
   regresión.

**Cómo se ve un cimiento correcto aquí:** el tipo es un campo obligatorio del
nodo, no una entrada opcional de un mapa. `HirBinding::Global(Rc<str>)` pasa a
`HirBinding::Global(Rc<str>, HirType)` y el problema de §2.1 deja de poder
existir, porque no hay dónde escribir «Dynamic» por omisión. El verificador
rechaza un `add.int` con operandos no-`Int`. Y `Dynamic` se convierte en una
elección explícita, contable y visible, en vez de en el silencio por defecto.

---

## 6. Lo que está bien y conviene no tocar

Para que la lista de arriba se lea en escala:

* **El checker es estricto de verdad.** Instancias selladas, literales sellados,
  asignación invariante, `override` obligatorio. Es una base más fuerte que la
  de la mayoría de lenguajes con esta ergonomía, y es lo que hace que todo lo
  demás sea *aprovechable* en vez de aspiracional.
* **La inferencia funciona.** Un retorno no declarado se infiere y se propaga;
  `Box<int>.get()` devuelve `int`; `Task<T>` se desenvuelve a `T` en la
  proyección.
* **`CgTy` no adivina.** Lo que la proyección no puede expresar es `Dynamic`,
  nunca una suposición. Es la política correcta.
* **Las pasadas de optimización funcionan.** LICM izó la lectura de campo del
  bucle de §4. El plegador de constantes coincide con el intérprete. A las
  pasadas no les falta calidad; les falta información de entrada.
* **El tiering con OSR, el arranque de ~10 ms y el objeto DST en una sola
  asignación** siguen siendo aciertos (ver `AUDITORIA_DISENO.md` §1).
* **Los números de hoy son buenos**: `fib` 2,02×, `gc_alloc` 1,82×, `dto` 1,71×
  frente a Bun. El argumento de este documento no es que el rendimiento sea
  malo. Es que se consiguió **a pesar** del canal de tipos, no gracias a él, y
  que la parte no cobrada es grande.

---

## 7. Plan, por coste creciente

Cada paso vale por sí solo y ninguno depende del siguiente.

### 0. Arreglar `int` (§1) — corrección, no rendimiento

No es negociable contra rendimiento. Decidir i48 o i64 y hacer que las tres
capas digan lo mismo. Si se elige i48, es media tarde. Si se elige i64, se funde
con el paso 3, y hay que hacerlo antes de publicar nada.

### 1. Que el tipo sobreviva a `global`, `new` y `moduleslot`

Tres cambios pequeños, el mayor rendimiento por línea del documento:

* `HirBinding::Global` lleva su `HirType`; `load_binding` deja de escribir
  `Dynamic`. — **44 % de la pérdida.**
* La construcción `new C(...)` produce `HirType::Class(C)`. — parte del 32 %.
* `moduleslot` toma el tipo del sitio de exportación. — 5 %.

Estimación: del 41 % de `Dynamic` a algo cercano al 10-15 %, sin tocar
`VmValue`, ni el GC, ni la frontera del host. Es plomería.

### 2. Contador de cobertura y verificador de tipos en el SSA

Barato, y es lo que impide que el paso 1 se erosione. `vn debug -p ssa` ya
distingue tipado de `Dynamic`; falta el agregado por archivo y una aserción en
`ssa/verify.rs` de que cada instrucción tipada recibe operandos del tipo que
declara. Convierte «se me olvidó anotar esta forma sintáctica» de invisible en
un fallo de CI.

### 3. Campos escalares sin boxear + `SlotKind` con carga

Es el paso 2 de `DISENO_IDEAL.md` §7, y lo que rompe el techo medido en §4.
Requiere que `SlotKind` lleve `Class(id)` y `Array(elem)`: sin identidad de
clase no hay offset constante. Aquí desaparece la cadena de 20 instrucciones.

### 4. Despacho estático y virtual

Con el paso 1 hecho, el destino de una llamada a función de módulo es conocido:
llamada directa en vez de `LoadGlobal` + `Call` genérico. Y con la jerarquía
cerrada por `override`, **emitir el `InvokeVirtual` que ya está implementado**
para métodos de clase e interfaz, en vez de buscar por cadena de texto.
Probablemente el mejor rendimiento por riesgo de la lista, porque el consumidor
ya existe y está probado.

### 5. Firmas nativas en Cranelift

`raw_signature` deja de declarar todo `I64`: un parámetro `float` es `F64` y
viaja en XMM. Acotado a `abi.rs` y al lowering de parámetros.

### 6. `T?` como par `(valor, bit)` en vez de `Dynamic`

Deja de castigar la característica que el lenguaje anuncia.

### 7. Sacar shapes y overflow del camino de los objetos tipados

Ya con offsets constantes (paso 3), una instancia de clase no necesita
`Rc<Shape>` ni `overflow`. Se convierten en el camino de `dynamic`, separado,
que es lo que `DISENO_IDEAL.md` §2 llama «lo dinámico, aparte». De 112 bytes a
~40 para el objeto de tres campos.

---

## 8. La frase corta

`AUDITORIA_DISENO.md` cierra con «Varn paga el precio de un lenguaje dinámico y
sólo cobra las ventajas de uno tipado en el frontend». La medición de este
documento le pone número y le corrige el matiz:

**El frontend hace su trabajo. El 41 % de esa prueba se tira, y la mitad de lo
que se tira se pierde en dos líneas que escriben `HirType::Dynamic` a mano.**

Los cimientos no están mal puestos. Está mal puesta la tubería que sale de
ellos, y la tubería es mucho más barata de arreglar que los cimientos.
