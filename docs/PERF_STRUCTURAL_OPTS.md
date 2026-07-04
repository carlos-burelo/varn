# Optimizaciones estructurales de rendimiento — especificación

Estado: **Proyecto A implementado** (2026-07-03) · Proyecto B diseño, no implementado.

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

array_sum 6x → ~3x (proyección original, **no confirmada empíricamente** —
ver "Implementación" abajo: el bench pareado mide a la par de baseline en
esta máquina). Cualquier loop `for x of arr` / `while i<arr.length` sobre
array invariante. Real-code: iteración de arrays es ubicua.

### Validación

- `tests/main.vn` 670/670.
- Bench pareado array_sum antes/después (canary entre corridas).
- Test específico: array que promueve a old-gen a mitad de loop (forzar GC)
  — debe seguir correcto (o tomar fallback).
- `VARN_NO_JIT=1` idéntico bit-a-bit.

### Implementación (2026-07-03)

Implementado con desviaciones respecto al diseño original, todas
encontradas durante la propia implementación/validación, no anticipadas
arriba:

1. **La regla estática "sin llamadas ni allocs" no cierra el hueco de GC —
   pero la solución dinámica inicial (re-resolver en la rama "GC corrió"
   del safepoint) resultó tener un costo oculto severo, descartada tras
   medirla.** El back-edge `Loop` ya corre un safepoint en CADA iteración
   (chequeo de nursery-fill) independientemente de si el cuerpo asigna algo
   — puede disparar GC en la iteración 1 por presión de nursery generada
   por código *anterior* al loop; un cuerpo sin llamadas ni mutación de
   array no garantiza por sí solo "sin GC durante el loop". El primer
   intento de cierre re-resolvía el puntero cacheado en la rama "GC corrió"
   del safepoint del back-edge (`codegen/jumps.rs::OpCode::Loop`). Un bench
   pareado a N grande (`arraySum` sobre 10k elementos, miles de llamadas)
   mostró que esa rama, cuando se toma, es catastróficamente cara —
   crecimiento **superlineal** en el número de llamadas (9.4x más iteraciones
   → hasta 14x más tiempo, con varianza run-a-run de hasta 3.4x) — sin
   causa raíz identificada a nivel de instrucción (una versión con un
   placeholder trivial en la misma rama no mostraba el problema; solo la
   cadena real de lectura de memoria del resolve lo disparaba). No se
   invirtió más tiempo en diagnosticar el mecanismo exacto: el riesgo de
   dejar semántica de GC no explicada en un hot path no es aceptable.

   **Diseño final, más simple y sin ese hot path:** en vez de re-resolver
   condicionalmente dentro del loop, el preheader fuerza el MISMO chequeo
   de safepoint (`codegen/jumps::emit_gc_safepoint_check`, factorizado y
   reusado desde el back-edge) una sola vez, antes de cachear. La
   elegibilidad se endureció de "sin llamadas ni mutación de array" a
   **allowlist explícita de opcodes probadamente libres de allocación**
   (`loop_hoist::is_alloc_free_op` — aritmética, comparaciones, control de
   flujo, lectura/escritura de array en rango; cualquier opcode fuera de la
   lista descalifica el loop). Con nursery ya fresco al momento de cachear
   y con el cuerpo del loop garantizado libre de allocación, ninguna
   iteración puede disparar GC — el back-edge del loop no necesita volver a
   chequear nada. Sin hot path nuevo, sin la patología: el mismo bench
   pareado (N=9000 y N=15000 llamadas) mide a la par de baseline (variación
   de ±5%, dentro del ruido de esta máquina), no la degradación superlineal
   observada en el primer intento.

2. **`OpCode::Loop` no es exclusivamente un back-edge real.** El backend SSA
   (`varn-opt`) reutiliza `Loop` como "goto" genérico hacia atrás para
   colapsar `break` seguido de `return` en un salto directo a un `Return`
   compartido más temprano en el layout lineal — satisface la forma
   numérica "target < esta instrucción" sin ser un loop. Inofensivo para el
   uso original de `collect_back_edges` en el backend (solo ensancha
   live-ranges, conservador por diseño), pero fatal para hoisting: el
   preheader asume que el header se alcanza por fall-through en la primera
   entrada, y ese "header" falso solo se alcanza por saltos explícitos — el
   preheader queda como código muerto y el registro de cache llega sin
   inicializar al sitio cacheado. Corrección: `loop_hoist::
   header_reachable_by_fallthrough` exige que la instrucción previa al
   header no sea `Jump`/`Loop`/`Return`/`Throw`/`Yield`; un "loop" falso
   tampoco cuenta como anidado al decidir si otro loop es innermost.

Además, un bug de alineación de stack encontrado en la primera validación
(segfault inmediato): el registro de cache no puede empujarse/sacarse por
separado del resto de `regmap.used_phys` — ~30 sitios en todo `codegen/`
calculan alineación de 16 bytes antes de llamadas FFI/GC leyendo
`regmap.used_phys.len()`; un push adicional fuera de esa cuenta desalinea
la pila en cualquier función con hoisting activo. Fix: el registro de
cache se añade a `used_phys` mismo (nunca a `map`, así que la lógica de
spill de virtuales no lo toca), y viaja gratis por toda esa lógica.

Elegibilidad restringida a **`ArrayGetIndex`/`ArrayLength` únicamente**
(nunca `GetIndex` genérico): son las únicas formas con garantía estática de
tipo Array, vía el flag `is_array` que el checker registra en
`checker_annotations.rs`. Esa garantía es solo de *tipo*, no de
no-nulidad (`is_array` se computa sobre `obj_ty.non_nullified()`) — un
receptor `Array<T>?` con valor `null` sí puede llegar al guard. Por eso el
resolve cachea `0` como sentinel de "no cacheable esta ejecución" en vez de
asumir éxito incondicional; cada sitio hace `test`/`jz` antes de confiar en
el cache (predicho perfectamente tras la primera iteración, ya que la
invariancia del registro implica que el resultado del guard es idéntico en
cada iteración de una misma ejecución del loop).

Archivos: `varn-types/src/loop_analysis.rs` (análisis compartido con el
backend), `varn-jit/src/loop_hoist.rs` (planificación de hoists +
allowlist de opcodes libres de allocación), `codegen/array_fast.rs`
(`emit_resolve_into_cache`, `emit_cached_or_fallthrough`),
`codegen/indexing.rs` + `codegen/arrays.rs` (sitios cacheados),
`codegen/jumps.rs` (`emit_gc_safepoint_check`, factorizado y compartido
entre el back-edge y el preheader), `compiler.rs` (emisión del preheader:
safepoint forzado + resolve), `regalloc.rs` (reserva condicional de
`LOOP_ARRAY_CACHE_REG` = R14, plegado en `used_phys`).

Validado: `tests/main.vn` 670/670 con JIT activo (también tras el
descarte del re-resolve en el safepoint). Test dirigido de GC forzado
antes de cada una de 5000 llamadas a una función con loop elegible —
5000/5000 resultados correctos, confirma que el safepoint forzado del
preheader deja el puntero cacheado consistente incluso cuando el heap
tenía presión de nursery justo antes de entrar al loop. `VARN_NO_JIT=1`
tiene una falla preexistente no relacionada en
`42-stdlib-comprehensive-test.vn` (confirmada idéntica en baseline sin
estos cambios) — el resto del intérprete coincide con el comportamiento
del JIT.

**Bench honesto (2026-07-03, esta máquina, `vn bench --runs 9`,
`arraySum` sobre array de 10k elementos, 2500 llamadas, checksum con
módulo para evitar DCE):** fase `execute`, mínimo de 9 corridas —
baseline 8.74s vs hoisted 8.49s (~3% más rápido en el mejor caso; p50
9.24s vs 9.72s, dentro del ruido — σ de esta máquina en corridas
comparables va de 225ms a 1s sobre bases de 8-10s). **No se confirma la
mejora ~2x que proyectaba el diseño original** (array_sum 6x→~3x); el
costo de la cadena de guardas evitada (~8 instrucciones x86 por acceso)
es una fracción demasiado pequeña del costo total observado por acceso
(~350ns en este bench) para distinguirse del ruido de esta máquina con
esta metodología. La ganancia teórica sigue siendo válida (menos
instrucciones ejecutadas, confirmado por inspección del código emitido),
pero no está confirmada empíricamente a un múltiplo claro — un
micro-benchmark más aislado (sin la llamada de función ni el checksum
envolvente, idealmente con el conteo de instrucciones/IC del propio
`vn bench --profile` en vez de wall-clock) queda pendiente para medirla
limpiamente.

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
