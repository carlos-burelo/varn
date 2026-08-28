# Varn Language — Comportamiento en Tiempo de Ejecución

> Fuentes: `tests/53-gc-class-vtable.vn`, `tests/62-jit-osr.vn`, `tests/63-escape-analysis.vn`, `tests/65-safepoint-roots.vn`, `tests/66-inline-strings.vn`, `tests/67-math-intrinsic-fastpath.vn`, `tests/74-regalloc-interference.vn`, `docs/VM_ARCHITECTURE.md`.

---

## 1. Modelo de Ejecución

Varn ejecuta código a través de dos backends:

| Modo | Descripción |
|------|-------------|
| **Intérprete** | Bytecode register-based ejecutado directamente. Activo por defecto para código frío o cuando `VARN_NO_JIT=1`. |
| **JIT (Cranelift x86-64)** | Compilación JIT de funciones calientes. Produce código nativo x86-64. Cobertura ~96.7% de la suite de pruebas. |

La variable de entorno `VARN_NO_JIT=1` deshabilita el JIT y fuerza el intérprete para validar parity entre ambos backends.

---

## 2. Garbage Collector Generacional

El GC de Varn es **generacional**:

- **Generación joven (nursery)**: objetos recién creados. GC frecuente y rápido.
- **Generación mayor (tenured)**: objetos que sobreviven múltiples GCs.
- **Soporte de safepoints**: el GC puede pausar hilos en puntos seguros de ejecución para recolectar sin corrupción de estado.

El GC cubre todos los tipos de heap:
- Objetos de clase (`HeapObj::Object`).
- Arrays.
- Strings.
- Closures.
- Generadores.

---

## 3. Análisis de Escape y Strings Inline

El compilador analiza si objetos pueden vivir en el stack en lugar del heap (**análisis de escape**). Para strings pequeñas, usa representación **inline** sin allocación heap:

```varn
// tests/66-inline-strings.vn
// Strings cortas se almacenan inline sin pasar por el heap
const s = "hi"         // inline: <16 bytes tipicamente
const t = "hello world this is a longer string"  // heap
```

---

## 4. Optimizaciones del Compilador

| Optimización | Descripción |
|-------------|-------------|
| **DCE (Dead Code Elimination)** | Elimina código muerto basándose en pureza de funciones. |
| **Algebraic Identities** | Simplifica expresiones algebraicas (`x * 1 → x`, `x + 0 → x`). |
| **Inlining** | Inline de funciones pequeñas en el sitio de llamada. |
| **Math Intrinsics** | Fast-path para operaciones matemáticas comunes. |
| **OSR (On-Stack Replacement)** | Compilación JIT de funciones que ya están en ejecución (bucles calientes). |
| **Register Allocation (Regalloc)** | Asignación de registros con interferencia mínima. |

---

## 5. Pipeline de Compilación

```
Fuente (.vn)
    ↓
Lexer
    ↓
Parser → AST
    ↓
Checker (type checking, binding, inference)
    ↓
HIR (High-Level IR)
    ↓
SSA (Static Single Assignment)
    ↓
Passes (DCE, algebraic, CFG simplification)
    ↓
Regalloc → Bytecode
    ↓
[Interpreter] o [JIT → x86-64 nativo]
```

---

## 6. Modelo de Memoria

- **Heap compartido por módulo principal**: el hilo principal y sus tareas comparten heap.
- **Heap aislado por isolate**: cada `spawnIsolate` crea un heap completamente separado. La comunicación solo ocurre vía canales (copia de valores serializables).
- **Tipos de heap**: `HeapObj::Object`, `HeapObj::Array`, `HeapObj::String`, `HeapObj::Closure`, `HeapObj::Generator`.

---

## 7. Stdlib: Dos Procedencias

| Procedencia | Descripción |
|------------|-------------|
| **Dev tree** (`std/`) | Archivos `.vn` del checkout; prioridad en desarrollo. |
| **Bundle embebido** | Bundle `.vnb` compilado en el binario; usado en distribución. |

Para forzar el bundle embebido: `VARN_STD=@embedded vn run file.vn`.

---

## 8. Garantías Numéricas

- `int`: aritmética de 48 bits, rango `-140737488355328 ..= 140737488355327`.
  Salir de ese rango **lanza `integer overflow`**: no envuelve, no satura y no
  promociona a float. Los desplazamientos (`<<`, `>>`, `>>>`) sí truncan al
  ancho del tipo, porque ese es su resultado definido y no un desbordamiento.
  Reglas en `varn-core/src/numeric.rs`; contrato fijado en
  `tests/53-int48-overflow.vn` y `tests/errors/int-overflow-*.vn`.
- `float`: IEEE 754 doble precisión. División `int/int` produce `float`.
- `decimal`: precisión exacta para operaciones financieras, sin errores de punto flotante.
- `bigint`: sin overflow, precisión arbitraria.

---

## 9. Closures y Captura de Variables

Las closures capturan variables por referencia del stack frame padre (mecanismo de upvalue). Si la variable muta después de crear la closure, la closure ve el valor actualizado.

```varn
function makeCounter(): () => int {
    let c = 0
    return () => {   // captura 'c' por upvalue
        c = c + 1
        return c
    }
}
const cnt = makeCounter()
assert("counter increments", cnt() === 1 && cnt() === 2)
```

---

## 10. Top-Level `await`

Los módulos de Varn soportan `await` a nivel de módulo. El runtime espera la completación de la expresión awaited antes de continuar la ejecución del módulo:

```varn
await runAsync()              // bloquea hasta que la función termina
if (!isIsolate) {
    await testIsolates()
}
```

---

## 11. `isIsolate` — Variable Global del Runtime

Variable booleana predefinida disponible en todos los módulos. Es `true` cuando el módulo se está ejecutando dentro de un isolate hijo:

```varn
if (!isIsolate) {
    await main()   // solo corre en el hilo principal
}
```

Esto evita que el módulo hijo re-ejecute la suite de pruebas cuando se carga para acceder a funciones exportadas.
