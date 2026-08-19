# Plan de Implementación: Migración del Subsistema de Red Host a `mio`

Plan detallado paso a paso para la migración del subsistema de red en `crates/varn-builtins/src/modules/host/net` hacia `mio` v1.0.

---

## Fase 1: Incorporación de Dependencias y Estructura Base

- [ ] **Tarea 1.1: Manifiesto de `varn-builtins`**
  - Añadir la dependencia `mio = { version = "1.0", features = ["net", "os-poll"] }` en `crates/varn-builtins/Cargo.toml`.
  - Verificar que compile limpiamente con `cargo check -p varn-builtins`.

- [ ] **Tarea 1.2: Estructura Modular de Red**
  - Crear el módulo `crates/varn-builtins/src/modules/host/net/driver.rs` para el reactor de eventos.
  - Implementar la estructura `IoDriver` con `mio::Poll`, `mio::Events`, y `mio::Waker`.

---

## Fase 2: Implementación del Reactor `IoDriver`

- [ ] **Tarea 2.1: Registro de Tokens y Estado de Sockets**
  - Implementar `TokenRegistry` para mapear de forma segura `Token(usize)` a:
    - Listeners TCP (`mio::net::TcpListener`).
    - Streams TCP (`mio::net::TcpStream`).
    - Operaciones pendientes (`AsyncTask<Value>`).

- [ ] **Tarea 2.2: Bucle del Reactor en Background**
  - Implementar el hilo de despacho continuo `IoDriver::run_loop`:
    - `poll.poll(&mut events, None)`
    - Iterar eventos listos (`event.is_readable()`, `event.is_writable()`).
    - Ejecutar lecturas/escrituras no bloqueantes y llamar a `task.settle(Ok(value))`.

---

## Fase 3: Conexión con Opcodes Nativos (`net.rs`)

- [ ] **Tarea 3.1: Reescritura de `tcpListen$` y `tcpAccept$`**
  - `tcpListen$`: Registrar `mio::net::TcpListener` en el `IoDriver`.
  - `tcpAccept$`: Fast-path no bloqueante; en `WouldBlock`, encolar `AsyncTask` en el `IoDriver`.

- [ ] **Tarea 3.2: Reescritura de `tcpConnect$`, `tcpRead$` y `tcpWrite$`**
  - `tcpConnect$`: Iniciar handshake no bloqueante y suspender sobre `Interest::WRITABLE`.
  - `tcpRead$` / `tcpWrite$`: Fast-path síncrono; fallback reactivo a `IoDriver`.

- [ ] **Tarea 3.3: Reescritura de `tcpClose$` y `tcpCloseListener$`**
  - Desregistrar del reactor mediante `poll.registry().deregister(...)`.
  - Despertar tareas canceladas con valor `-1` o error controlado.

---

## Fase 4: Validación y Benchmarks de Concurrencia

- [ ] **Tarea 4.1: Suite de Tests de Integración HTTP y Servidor**
  - Ejecutar `tests/main.vn` para asegurar cero regresiones en los 79 módulos de tests.

- [ ] **Tarea 4.2: Benchmark de Estrés de Concurrencia (10k, 50k, 100k reqs)**
  - Medir RPS y latencia con microbenchmarks concurrentes.
  - Verificar comportamiento en ráfagas masivas.

- [ ] **Tarea 4.3: Matriz de 4 Oficial**
  - Validar Tree JIT, Tree NO_JIT, Embedded JIT, Embedded NO_JIT (1094/0 verde).
  - Actualizar binario global en `C:\Users\x\.cargo\bin\vn.exe`.
