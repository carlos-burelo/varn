# Roadmap para Multi-Threading Completo e Isolates de Alto Rendimiento en Varn

Este documento detalla los pasos pendientes para consolidar una arquitectura de multi-threading al 100% basada en el modelo de Isolates (paso de mensajes / actores) y optimizar el rendimiento del motor.

---

## 1. Completar la Arquitectura de Isolates (Hacia el 100%)

Para que el modelo de actores sea completamente maduro y comparable a entornos de producción como Dart o Erlang/Elixir, se proponen las siguientes fases:

### Fase A: Event Loop Integrado (Mensajería No Bloqueante)
* **Estado actual:** La recepción de mensajes mediante `IsolatePort.receive()` suspende el hilo actual de forma síncrona/bloqueante a nivel de SO (`receive_blocking`).
* **Mejora:** Integrar un sistema de callbacks reactivo (por ejemplo, `port.on("message", callback)`) que se ejecute a través del event loop de cada Isolate sin bloquear el hilo físico.

### Fase B: Transferencia Zero-Copy (Transferables)
* **Estado actual:** Toda mensajería hace una copia profunda a través del tipo intermedio `SendValue`, lo cual penaliza el rendimiento con grandes volúmenes de datos.
* **Mejora:** Implementar semánticas de transferencia de propiedad (*ownership transfer*) para estructuras mutables grandes (como arrays o buffers), invalidando la referencia en el emisor para evitar copias físicas en memoria.

### Fase C: Gestión de Ciclo de Vida y Tolerancia a Fallos
* **Monitoreo de Errores:** Permitir al Isolate padre escuchar fallos no controlados del Isolate hijo (excepciones/panics) en lugar de que el hilo secundario muera de forma silenciosa.
* **Terminación Forzada:** Agregar soporte para llamadas controladas como `isolate.terminate()` desde el hilo padre.
* **Isolate Pools:** Implementar un pool interno de hilos reutilizables para evitar el alto coste de creación de hilos nativos (`std::thread::spawn`) bajo demanda.

---

## 2. Diagnóstico y Corrección de Benchmarks

### Limitación Detectada en Async Execution
* **Comportamiento actual en `vn bench`:**
  En la implementación de `bench_impl.rs`, el comando `vn bench` ejecuta `machine.run(closure)` una sola vez por muestra de rendimiento.
  Si el script bajo prueba contiene llamadas asíncronas (`await`), la VM suspende su ejecución en el primer opcode de `Await` y retorna un estado suspendido. El benchmark registra esto como una ejecución exitosa de apenas unas pocas microsegundos (ej. 37 µs en el test de Isolates), **sin medir realmente la ejecución paralela o el procesamiento de los workers**.
* **Acción Propuesta:**
  Modificar `crates/varn-cli/src/bench_impl.rs` para que implemente el bucle de reanudación y espera de tareas asíncronas (`VmSuspend::Await`) idéntico al que utiliza el runner estándar de producción en `execute.rs`:
  ```rust
  loop {
      let res = machine.run(closure.clone());
      match res {
          Ok(_) => match machine.ctx.vm_suspend.take() {
              None => break,
              Some(varn_vm::exec::VmSuspend::Await { value, dest_reg }) => {
                  // Resolver y esperar la tarea/promesa síncronamente antes de reanudar la VM
              }
              ...
          }
      }
  }
  ```

---

## 3. Optimización Extrema de JIT (Hacia el Rendimiento de JS/V8)

Para lograr que Varn JIT compita de cerca con motores maduros como V8 (JavaScript) o LuaJIT, se debe reducir al mínimo la sobrecarga del intérprete y el empaquetado de memoria. Se proponen cuatro optimizaciones arquitectónicas estructuradas por fases:

### Fase A: Inlining de Funciones JIT (Llamadas directas)
* **Objetivo:** Eliminar la sobrecarga física de llamada a función (pushing call frames, guardar punteros, saltos de memoria) en funciones pequeñas y recursivas (como `fib`).
* **Diseño:**
  1. En el compilador, analizar el árbol de sintaxis abstracta (AST) o el bytecode para marcar funciones candidatas (ej. menos de 30 bytes de bytecode y sin llamadas indirectas/dinámicas complejas).
  2. Al generar código de máquina JIT, en lugar de emitir un opcode `Call` (que requiere saltar al ayudante JIT y registrar un nuevo marco), insertar el bloque de código de la función hija directamente dentro del flujo de la función padre.
  3. Mapear los registros de entrada y salida del hijo directamente a los registros virtuales libres del padre.

### Fase B: Especialización de Tipos en Caliente (Type Specialization)
* **Objetivo:** Ejecutar aritmética y comparaciones en registros de CPU nativos en lugar de usar dynamic boxing / dynamic dispatch a través del envoltorio `VmValue`.
* **Diseño:**
  1. El compilador JIT debe especializar bloques matemáticos enteros. Si la firma de la función o el análisis de tipos de Varn determina que las variables son `int` fijas (de 64 bits), no deben ser representadas como `VmValue` dinámicos en la CPU.
  2. Generar instrucciones assembly puras de hardware:
     - `add r11, r10` para sumas de enteros.
     - `sub r11, 1` para decrementos.
     - `cmp r10, 1` para comparaciones rápidas.
  3. Solo "envolver" (*box*) el entero nativo a un `VmValue` cuando el valor deba ser retornado fuera de la zona JIT optimizada o guardado en estructuras dinámicas (objetos no tipados).

### Fase C: Asignación Avanzada de Registros (Linear Scan)
* **Objetivo:** Reducir al mínimo los accesos a memoria (pila RSP) y mantener la mayor cantidad de variables locales en registros físicos de la CPU.
* **Diseño:**
  1. Reemplazar la asignación simple de registros actual por un algoritmo de **Asignación de Registros por Escaneo Lineal** (*Linear Scan Register Allocation*).
  2. Calcular el intervalo de vida de cada variable virtual.
  3. Mapear las variables con intervalos solapados a los 14 registros de propósito general libres de x86_64 (`rax`, `rcx`, `rdx`, `rbx`, `rsi`, `rdi`, `r8`–`r15`).
  4. Realizar derrames (*spill*) a la pila física solo si la cantidad de variables concurrentemente activas excede los registros físicos libres de la CPU.

### Fase D: Nursery con Asignación por Puntero de Empuje (Bump Allocation)
* **Objetivo:** Reducir a 1 ciclo de CPU el coste de asignar memoria para nuevos objetos en el heap (como closures o arrays en bucles calientes).
* **Diseño:**
  1. Dividir el Heap en una zona joven (*nursery*) y una zona vieja (*tenured*).
  2. La nursery mantiene un puntero de límite y un puntero actual.
  3. Para asignar memoria en JIT, simplemente generar dos instrucciones assembly:
     - Sumar el tamaño del objeto al puntero actual de asignación.
     - Comparar el puntero actual con el puntero de límite. Si excede, saltar al recolector de basura (GC) para realizar una colección menor (*minor GC*).
  4. Esto evita llamadas lentas al asignador general del sistema operativo (`malloc`/`jemalloc`) y optimiza el uso del caché de CPU.

