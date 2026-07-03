# Optimizaciones estructurales de rendimiento — especificación

Estado: **diseño, no implementado**. Fecha: 2026-07-02.

Este documento especifica los dos proyectos de optimización que cierran los
gaps restantes >4x contra Node/V8. Ambos son estructurales (no peephole) y
multi-sesión. Las mediciones base son pareadas Varn-vs-Node v24.4.1, en
frío (canary `no_mod ≈ 305ms`), checksums idénticos.

Gaps actuales (2026-07-02, tras array-inline + BuildStr):

| bench       | Varn  | Node | ratio | causa raíz dominante                    |
|-------------|-------|------|-------|-----------------------------------------|
| map_set     | 331ms | 40ms | 8.3x  | hash real + build de string-key         |
| array_sum   | 268ms | 45ms | 6.0x  | **guards invariantes no hoisteados**    |
| prop_mono   | 93ms  | 22ms | 4.2x  | **NaN-box tax** (objeto ya eliminado)   |
| call_closure| 291ms | 70ms | 4.2x  | overhead de entrada de closure          |
| alu_int     | 732ms | 294ms| 2.5x  | **NaN-box tax** + idiv por constante    |

Los dos proyectos aquí atacan las causas marcadas: **guard hoisting**
(array_sum) y **typed unboxed ints** (prop_mono, alu_int, fib, aritmética
de array_sum).

---

## Proyecto A — Hoisting de guards loop-invariantes en el JIT

### Problema

El fast path de acceso a arrays (`codegen/array_fast.rs::
emit_resolve_array_payload`) verifica en CADA iteración, para `arr[i]`:

1. `arr` es heap-ref (mask + cmp del tag)
2. selección de generación (bit 31: nursery vs old-gen) — un branch
3. el slot contiene un `Array` (tag-byte cmp)
4. bounds check `i < len` (recarga `len` de memoria)
5. load `data[i]`

Cuando `arr` es **invariante del loop** (no se escribe en el cuerpo), los
pasos 1-3 producen el mismo puntero de element-vec en toda iteración, y la
longitud (paso 4) es constante mientras no haya push/pop. V8 prueba el
"shape" del array una vez en el preheader y el cuerpo del loop queda como
bounds-check + load. Nosotros repetimos ~8 instrucciones invariantes por
iteración.

### Estado actual del JIT

- Codegen **lineal por-instrucción** (`compiler.rs::compile_proto`): un
  solo pase sobre el bytecode, cada opcode emite su código sin conocer
  fronteras de loop.
- La detección de back-edges YA existe pero solo en el backend
  (`varn-backend/regalloc_post.rs::collect_back_edges`, vía
  `varn_types::bytecode::decode`). Devuelve `Vec<(target_instr, loop_instr)>`.
- `register_meta`/`SlotKind` da el tipo estático de cada registro
  (Int/Float/Bool/Str/Ref/Dynamic).

### Diseño

Introducir una fase de **análisis de loops** antes del codegen, que produzca
para cada loop natural:

- `preheader_ip`: punto de inserción justo antes de la entrada al loop.
- `invariant_regs`: registros nunca escritos dentro del cuerpo del loop
  (def-set del loop = unión de `InstrInfo.def` de sus instrucciones; un reg
  es invariante si no está en ese set).
- `hoistable_guards`: por cada `GetIndex`/`ArrayGetIndex`/`ArrayLength`
  cuyo receptor (`obj_reg`) es invariante, el guard de tipo/generación es
  hoisteable.

El codegen emite, en el preheader:

- Resolver `obj_reg` → puntero de element-vec una vez, en un **registro
  callee-saved reservado** (extender `ALLOC_REGS`, hoy `[Rbx,R12,R13,R14]`,
  reservando uno para "array payload cacheado" del loop más interno).
- Guardar la longitud en otro registro/slot si el loop no hace push/pop
  (invalidar si el cuerpo contiene `ArrayPush`/`ArrayPop`/`ArrayExtend`
  sobre ese array).

El cuerpo del loop, para un `arr[i]` con `arr` invariante:

- Salta los pasos 1-3 (usa el puntero cacheado).
- Mantiene el bounds check (paso 4) usando la longitud cacheada.
- Load (paso 5).

Fallback: si el análisis no puede probar invarianza (obj_reg escrito,
receptor no-array estático, loop con push), emite el fast path actual sin
cambios.

### Pasos de implementación

1. **Exponer el análisis de loops.** Mover/compartir `collect_back_edges`
   (ya en `varn_types::bytecode` idealmente) y construir loops naturales:
   para cada back-edge `(header, latch)`, el cuerpo = instrucciones en
   `[header, latch]` (los loops de Varn son reducibles, un solo back-edge
   por header — verificar contra el emisor SSA).

2. **Def-set por loop.** Recorrer el cuerpo con `decode`, unir `info.def`.
   Un registro es invariante si ∉ def-set. Registrar también si el cuerpo
   contiene ops que mutan longitud de array.

3. **Reservar registro de cache.** Añadir un slot en `RegMap` para el
   "array payload del loop activo". Solo el loop más interno lo usa
   (loops anidados: el externo pierde el cache — aceptable, el interno
   domina). Ajustar `ALLOC_REGS` o usar un callee-saved dedicado.

4. **Emitir preheader.** En `compile_proto`, al llegar al header de un loop
   con guards hoisteables, emitir la resolución una vez antes del cuerpo.

5. **Fast-path condicional en `array_fast`.** `emit_resolve_array_payload`
   toma un flag "receptor ya resuelto en reg cacheado" → usa el cache.

### Invariantes de correctness

- **Invalidación por GC/realloc:** el puntero de element-vec cacheado se
  invalida si el array crece (realloc de `Vec`) o si un minor-GC mueve el
  objeto. **Crítico:** un push dentro del loop, o una llamada que dispare
  GC, invalida el cache. Regla segura: hoistear SOLO si el cuerpo del loop
  no contiene llamadas (`InstrInfo.call_args.is_none()` en todo el cuerpo)
  NI ops de mutación de array. El safepoint de back-edge corre GC — así
  que el cache debe recomputarse tras cada iteración O el loop debe estar
  libre de GC. Empezar conservador: solo loops sin llamadas ni allocs.
- **Bit 31 de generación:** un array en nursery puede promover a old-gen
  en un GC intermedio, cambiando su índice. Sin GC en el cuerpo, el índice
  es estable → cache válido.
- **Bounds:** la longitud cacheada solo es válida sin push/pop/extend.

### Riesgos

- El puntero cacheado sobrevive incorrectamente a un GC → use-after-free.
  Mitigación: restricción "loop sin llamadas ni allocs" en v1.
- Registro callee-saved menos para el resto del cuerpo → posible spill
  extra. Medir en benches con muchos locales.

### Impacto esperado

array_sum 6x → ~3x. Cualquier loop `for x of arr` / `while i<arr.length`
sobre array invariante. Real-code: iteración de arrays es ubicua.

### Validación

- `tests/main.vn` 670/670.
- Bench pareado array_sum antes/después (canary entre corridas).
- Test específico: array que promueve a old-gen a mitad de loop (forzar GC)
  — debe seguir correcto (o tomar fallback).
- `VARN_NO_JIT=1` idéntico bit-a-bit.

---

## Proyecto B — Locals enteros sin NaN-box en loops tipados

### Problema

Cada operación entera en el JIT des-empaqueta y re-empaqueta el NaN-box.
`AddInt` (both-int, `codegen/arith.rs`):

```
add_reg_reg(Rax, R11)          ; suma los dos ints boxed
sub_reg_reg(Rax, REG_INT_TAG)  ; quita una copia del tag
shl_reg_imm8(Rax, 16)          ; \ enmascara a 48 bits
shr_reg_imm8(Rax, 16)          ; / (carry del bit 47 no debe tocar el tag)
or_reg_reg(Rax, REG_INT_TAG)   ; re-aplica el tag
```

5 instrucciones por suma. V8 sobre un Smi untagged: 1 `add` + un `jo`
(jump-on-overflow). El `Mul` es peor (8 instrucciones). Para un loop que
encadena ops enteras sobre el mismo valor, cada op re-boxea aunque el
siguiente uso inmediatamente vuelve a operar sobre bits boxed.

`REG_INT_TAG` = `QNAN|TAG_INT` = `0x7FFC_0000_0000_0000`, en R15.
`SlotKind::Int` ya marca qué registros son estáticamente enteros
(`slot_kinds.rs`), y la semántica i48 con wrap ya está unificada
(`varn-core/numeric.rs`, ver [[varn-int-semantics-i48]]).

### Diseño

Mantener los valores enteros de un loop tipado en forma **untagged** (i64
crudo, sign-extendido) en registros callee-saved a lo largo de una cadena
de ops enteras, re-empaquetando solo en las fronteras:

- **Frontera de entrada:** al cargar un int boxed a un reg untagged, hacer
  `shl 16; sar 16` una vez (sign-extend del payload de 48 bits).
- **Cadena de ops:** `AddInt`/`SubInt`/`MulInt` sobre regs untagged son
  1 instrucción (`add`/`sub`/`imul`), sin mask/tag. Overflow: wrap i48 vía
  un `shl 16; sar 16` diferido, o un check `jo` → deopt/rebox. La semántica
  es wrap (varn-core), así que basta con enmascarar al re-empaquetar.
- **Frontera de salida:** al almacenar a un slot que espera boxed, o pasar
  a un op que no es int-untagged (llamada, comparación que necesita el
  valor boxed, store a memoria heap), re-empaquetar: `shl 16; shr 16; or
  REG_INT_TAG`.

El resultado: en `acc = acc + i*3 - (i%7)`, `acc` e `i` viven untagged en
registros durante todo el cuerpo; solo se re-boxean al final del loop (o
nunca, si el loop-carry se mantiene untagged entre iteraciones).

### Estado actual reutilizable

- `SlotKind::Int` por registro (ya derivado, `slot_kinds.rs`).
- `register_meta` ya guía el flush-skipping del JIT
  (ver [[varn-jit-register-meta-contract]]).
- Semántica de wrap i48 unificada — el re-empaque es `shl/shr/or`.

### Diseño del ABI de registros untagged

Introducir un segundo "banco" lógico: para un registro virtual con
`SlotKind::Int` que es local de un loop tipado, su representación física
en el cuerpo del loop es **untagged i64** en un callee-saved. Necesita:

- Un `RegState` por registro físico: `Boxed` | `UntaggedInt`.
- En la entrada al loop, promover los int-locals del loop-carry a
  untagged (un `shl/sar` por cada uno en el preheader).
- En la salida del loop / antes de cualquier uso boxed, re-empaquetar.
- Las comparaciones (`LtInt` etc.) sobre untagged son `cmp` directo
  (sin necesidad de tag) — de hecho MÁS simples.

### Pasos de implementación

1. **Modelo de estado de registro.** Extender el codegen con un mapa
   `phys_reg → RegState`. Inicialmente todo `Boxed`.

2. **Análisis de "int-chains" tipadas.** Usando `register_meta`, identificar
   secuencias de ops enteras (`AddInt/SubInt/MulInt/ModInt/DivInt/LtInt...`)
   cuyos operandos y resultados son `SlotKind::Int`, dentro de un loop.

3. **Emit untagged.** Para esas ops, emitir la variante de 1 instrucción
   sobre regs marcados `UntaggedInt`. Insertar promoción (`shl/sar`) al
   materializar un int boxed como untagged, y re-empaque (`shl/shr/or`) al
   cruzar a un contexto boxed.

4. **Loop-carry untagged.** Si una variable int cruza el back-edge y solo
   participa en ops enteras, mantenerla untagged entre iteraciones (promover
   en preheader, re-boxear tras el loop). Esto elimina el re-empaque
   per-iteración por completo.

5. **Overflow / wrap.** La semántica es wrap i48. Sobre untagged i64, el
   wrap solo importa al re-empaquetar (el `shl 16; shr 16` lo hace). Entre
   ops untagged, dejar que el i64 desborde libremente y enmascarar al final
   es correcto SOLO si ninguna comparación intermedia depende del valor
   fuera de rango i48. **Verificar:** `LtInt` sobre untagged debe comparar
   los valores i48 correctos → hacer el sign-extend en la promoción y
   mantener el rango. Alternativa segura: `shl 16; sar 16` tras cada op
   (2 instr en vez de 5 — aún gana), enmascarando siempre a i48.

### Invariantes de correctness

- **Un registro nunca se lee boxed mientras está en estado `UntaggedInt`**
  (y viceversa). El modelo de estado debe re-empaquetar antes de cualquier
  uso que espere boxed (llamada, store heap, ToString, comparación con
  valor dynamic).
- **GC roots:** un untagged i64 NO es un NaN-box válido — el GC no debe
  escanearlo como valor. Los flushes a memoria (spill, safepoint) deben
  re-empaquetar primero, o marcar el slot como no-root. **Crítico:** el
  safepoint de back-edge escanea el stack; los untagged en registros
  callee-saved no se escanean (no son roots — son ints), pero si se
  spillean a stack deben re-boxearse o el GC los malinterpretará.
- **Tier-identidad:** el resultado observable debe ser idéntico al
  intérprete (semántica wrap i48). Test bit-a-bit `VARN_NO_JIT=1`.

### Riesgos

- El más alto del roadmap. Un error de estado (leer untagged como boxed)
  = corrupción silenciosa de valores.
- Interacción con el register allocator actual (frecuencias) y con el
  flush/reload alrededor de llamadas.
- El GC malinterpretando un untagged spillado como heap-ref = crash o
  corrupción de heap.

### Impacto esperado

alu_int 2.5x → ~1.5x; prop_mono 4.2x → ~2x; fib y toda aritmética de loop
mejoran. Es la causa raíz compartida de casi todos los gaps numéricos.
Real-code: cualquier loop con cómputo entero.

### Validación

- `tests/main.vn` 670/670 tras cada incremento.
- Test de overflow/wrap: loop que desborda i48 — JIT e intérprete
  bit-idénticos (ver el test de tier-identidad de [[varn-int-semantics-i48]]).
- Test de spill: forzar presión de registros en un loop entero para
  ejercer el spill de untagged (debe re-boxear).
- Bench pareado alu_int/prop_mono/fib antes/después, canary entre corridas.

---

## Orden recomendado

1. **Proyecto A (guard hoisting)** primero: menor riesgo (correctness de
   loop preheaders es acotable con la restricción "loop sin llamadas"),
   impacto concreto en código real (iteración de arrays), y establece la
   infraestructura de análisis de loops que el Proyecto B también aprovecha.

2. **Proyecto B (typed unboxed ints)** después: máximo impacto pero máximo
   riesgo; construir sobre el análisis de loops de A.

Ambos comparten prerequisito: **análisis de loops naturales en el JIT**
(hoy solo existe en el backend). Implementarlo una vez, reusar en ambos.

Relacionado: `docs/VM_ARCHITECTURE.md`, `docs/COMPILER_ARCHITECTURE.md`,
memorias [[varn-int-semantics-i48]] [[varn-jit-register-meta-contract]]
[[varn-jit-inline-heap-layout]].
