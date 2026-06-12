# Plan de Optimización de Rendimiento JIT (Rumbo a Bun/JSC)

Este documento detalla la hoja de ruta técnica y arquitectónica para optimizar el JIT de Varn y reducir la diferencia de rendimiento actual frente a JavaScriptCore (Bun) y V8 (Node.js).

---

## 1. Llamadas JIT-a-JIT Directas en Assembly (Eliminación de Helpers en Rust)

**Cuello de botella actual**: Cada llamada recursiva o directa salta a las funciones de Rust `jit_prepare_call` y `jit_post_call` para empujar y sacar `CallFrame` de `ctx.frames`, y verificar la capacidad de `ctx.stack`. Esto añade millones de saltos de contexto entre código máquina y Rust.

### Solución: Apilar CallFrame directamente en ensamblador
Como `ExecCtx` está decorado con `#[repr(C)]`, sus offsets en memoria son estables y predecibles desde el registro `ARG_EXEC_CTX` (generalmente `R9` en Windows o `Rcx` en SysV):

* **Offsets de `ExecCtx`**:
  * `stack` (Offset 0): `ptr` (Offset 0), `cap` (Offset 8), `len` (Offset 16)
  * `frames` (Offset 24): `ptr` (Offset 24), `cap` (Offset 32), `len` (Offset 40)

* **Offsets de `CallFrame` (32 bytes)**:
  * `closure_ptr` (Offset 0): `*const VmClosure`
  * `base` (Offset 8): `usize`
  * `ip` (Offset 16): `usize`
  * `_owned_closure` (Offset 24): `Option<Rc<VmClosure>>` (Donde `None` es un puntero a `0`).

### Algoritmo en Ensamblador JIT (`codegen/calls.rs`):

#### En la secuencia de llamada (Call):
1. **Comprobar espacio en la pila**:
   * Cargar `callee_base + register_count + 32`.
   * Comparar contra `stack.cap` (Offset 8 de `ExecCtx`).
   * Si es mayor, saltar a un helper de Rust para redimensionar la pila.
2. **Comprobar capacidad de frames**:
   * Cargar `frames.len` (Offset 40) y `frames.cap` (Offset 32).
   * Si `len == cap`, saltar a un helper de Rust para reasignar el vector de frames.
3. **Escribir el `CallFrame` en el buffer de frames**:
   * Cargar el puntero base del buffer `frames.ptr` (Offset 24).
   * Calcular la dirección de destino: `dest = ptr + len * 32`.
   * Escribir:
     * `[dest]` = dirección de la closure destino (`ARG_CLOSURE`).
     * `[dest + 8]` = `callee_base` (`ARG_BASE`).
     * `[dest + 16]` = dirección de retorno (`return_ip`).
     * `[dest + 24]` = `0` (indica `None` para `_owned_closure`).
4. **Incrementar longitud de frames**:
   * Incrementar `frames.len` (Offset 40) en 1.
5. **Ejecutar llamada directa**:
   * Cargar el JIT entry point de la closure (`ARG_CLOSURE + 56`).
   * Ejecutar `call r10`.

#### En la secuencia de retorno (Return / Post-Call):
1. **Sacar el frame**:
   * Decrementar `frames.len` (Offset 40) directamente en ensamblador.
   * *(Nota: Si hay upvalues que cerrar, se puede saltar condicionalmente a un helper de Rust, de lo contrario se evita por completo).*

---

## 2. Call Inline Cache (Call IC) para llamadas dinámicas

**Cuello de botella**: Las funciones cargadas desde variables globales o propiedades de objetos requieren resolver la closure mediante un lookup en el heap dinámico en cada llamada.

### Solución: Caching del target de llamada en el buffer JIT
Asociar una estructura `JitCallSiteIC` en memoria por cada instrucción `Call` compilada:

```rust
#[repr(C)]
pub struct JitCallSiteIC {
    pub cached_callee: VmValue,          // VmValue (NaN-boxed)
    pub cached_closure: *const VmClosure, // Puntero raw de la closure
    pub cached_entry: usize,             // Dirección JIT de entrada
}
```

En la compilación de `OpCode::Call`, inyectar la dirección del struct IC en el código máquina:
1. Cargar el valor de la closure a llamar (`Reg::Rax`).
2. Comparar `Rax` contra `cached_callee` (Offset 0 del IC).
3. **Hit**:
   * Cargar `cached_closure` en `ARG_CLOSURE`.
   * Cargar `cached_entry` en `R10`.
   * Saltar directamente a la secuencia de llamada inline (Paso 1).
4. **Miss**:
   * Saltar a un helper de resolución (`jit_resolve_call_ic`), que actualiza el struct IC con el nuevo target (JIT compilándolo si es necesario) y retorna a la ejecución.

---

## 3. Optimización Extrema de Auto-Recursividad (Self-Recursion Bypass)

Para algoritmos altamente recursivos (como `fib`), el callee de la llamada suele ser la misma función que se está ejecutando.

### Solución: Detección y salto local
Si el JIT detecta al resolver la llamada en la sección IC que `cached_closure == ARG_CLOSURE` (nosotros mismos):
* Evitar todo el despacho de llamadas dinámicas.
* Saltar directamente a la etiqueta de inicio de nuestro propio JIT (saltándose el prólogo si el número de argumentos coincide, actuando casi como un bucle local).

---

## 4. Generación de Código Especializada por Tipo (Type Specialization)

**Cuello de botella**: Operaciones aritméticas simples (`Add`, `Sub`, `Mul`) generan código JIT que comprueba el tipo de los operandos en runtime (`is_int` vs `is_f64`), bifurcando el flujo de ejecución máquina.

### Solución: JIT especializado
Dado que Varn cuenta con un type checker estático en el frontend:
* Si el type checker garantiza que los operandos de una suma son de tipo `int`, el compilador JIT debe emitir directamente la instrucción ensamblador `add` en registros de 64 bits (ej. `add rdi, rsi`).
* Si garantiza que son de tipo `float`, debe emitir directamente instrucciones SSE / AVX (ej. `addsd xmm0, xmm1`).
* Esto elimina por completo los branches de comprobación de tipo y las operaciones de NaN-unboxing en runtime, alcanzando velocidad de ejecución nativa (C/Rust).

---

## 5. Snapshots de Bytecode y Heap para la Librería Estándar (Cold Start)

**Cuello de botella**: En cada ejecución `vn run`, el compilador analiza y ejecuta dinámicamente decenas de archivos `.vn` de la librería estándar (`std:math`, `std:sys`, etc.), lo que añade unos ~100ms fijos de retardo inicial.

### Solución: Serializar estado inicial
* Generar y guardar en disco un archivo de bytecode precompilado de la librería estándar.
* O mejor aún: guardar un snapshot del heap de la VM (`snap_heap`, `snap_globals`, `snap_modules`) ya inicializado (similar a como lo hace `vn bench` en cada iteración).
* Al arrancar `vn run`, mapear directamente en memoria este snapshot en microsegundos, permitiendo arrancar y ejecutar scripts de usuario de manera instantánea (< 5ms).

---

## 6. Optimización en la Fase de Compilación: `OpCode::CallSelf` para Recursividad Estática

**Cuello de botella en la compilación**: Al compilar una función recursiva como `fib(n-1)`, el compilador trata el callee como una llamada regular. Esto obliga a emitir bytecode para cargar la variable global `fib` (`LoadGlobalIdx`), guardar su valor en un registro temporal, y luego hacer un `Call` genérico que resuelve la closure en runtime.

### Solución: Un Opcode dedicado a la auto-recursividad
En la fase de análisis del compilador (`crates/varn-compiler/src/codegen/expr/calls.rs`), podemos comprobar estáticamente si la expresión del callee es un identificador que coincide con el nombre de la función que se está compilando (`c.name`):

```rust
if let ExprKind::Identifier { name } = &callee.kind {
    if name == &c.name {
        // ¡Es una llamada recursiva a nosotros mismos!
        return emit_recursive_call(c, offset, args);
    }
}
```

Al detectar esto, emitimos un opcode dedicado: **`OpCode::CallSelf`**.

### Ventajas e Implementación:

1. **Ahorro de Bytecode y Registros**:
   * No necesitamos emitir un `LoadGlobalIdx` para el callee.
   * No reservamos un registro temporal para almacenar la closure del callee.
   * El formato de la instrucción `CallSelf` en bytecode simplemente omite el registro del callee:
     ```text
     [OpCode::CallSelf]
     [dest_reg, 0]
     [arg_count, arg_start]
     ```

2. **Ejecución en el Intérprete**:
   * Cuando el intérprete procesa `CallSelf`, obtiene directamente el puntero raw de la closure actual desde la pila del `CallFrame` activo (`(*ctx).frames[frame_idx].closure_ptr`).
   * Bypassa todas las comprobaciones dinámicas (si es heap, si es closure, si es async/generator, permisos).
   * Añade el frame directamente y pone el `ip` a `0` (el inicio de la función).

3. **Generación en el JIT (`codegen/calls.rs`)**:
   * JIT-compila `CallSelf` a un salto de máquina superoptimizado:
     * Compara el stack limit con `stack.cap` (resize si se excede).
     * Empuja el frame directamente a `ctx.frames` usando la closure actual (`ARG_CLOSURE`).
     * Ejecuta una llamada relativa directa en ensamblador a la dirección de entrada de nuestro propio JIT (o leyendo `ARG_CLOSURE + 56`), evitando cualquier helper de Rust o lookup de lookup.
     * Decrementa `frames.len` al retornar.

Esta optimización reduce a cero el overhead de resolución de nombres y JIT helpers en recursión, permitiendo a Varn ejecutar algoritmos recursivos puros a la velocidad nativa de un bucle compilado en C.

