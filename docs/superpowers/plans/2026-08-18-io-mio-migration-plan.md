# Plan de Implementación: Migración del Subsistema de Red Host a `mio`

Plan detallado paso a paso para la migración del subsistema de red en `crates/varn-builtins/src/modules/host/net` hacia `mio` v1.0.

---

## Fase 1: Incorporación de Dependencias y Estructura Base

- [x] **Tarea 1.1: Manifiesto de `varn-builtins`**
  - Añadir la dependencia `mio = { version = "1.0", features = ["net", "os-poll"] }` en `Cargo.toml` y `crates/varn-builtins/Cargo.toml`.
  - Verificar que compile limpiamente con `cargo check -p varn-builtins`.

- [x] **Tarea 1.2: Estructura Modular de Red**
  - Crear el módulo `crates/varn-builtins/src/modules/host/net/driver.rs` para el reactor de eventos.
  - Implementar la estructura `IoDriver` con `mio::Poll`, `mio::Events`, y `mio::Waker`.

---

## Fase 2: Implementación del Reactor `IoDriver`

- [x] **Tarea 2.1: Registro de Tokens y Estado de Sockets**
  - Implementar `TokenRegistry` para mapear de forma segura `Token(usize)` a:
    - Listeners TCP (`mio::net::TcpListener`).
    - Streams TCP (`mio::net::TcpStream`).
    - Operaciones pendientes (`AsyncTask`).

- [x] **Tarea 2.2: Bucle del Reactor en Background**
  - Implementar el hilo de despacho continuo `IoDriver::run_event_loop`:
    - `poll.poll(&mut events, Some(Duration::from_millis(50)))`
    - Iterar eventos listos (`event.is_readable()`, `event.is_writable()`).
    - Ejecutar lecturas/escrituras no bloqueantes y llamar a `task.settle(Ok(value))`.

---

## Fase 3: Conexión con Opcodes Nativos (`net.rs`)

- [x] **Tarea 3.1: Reescritura de `tcpListen$` y `tcpAccept$`**
  - `tcpListen$`: Registrar `mio::net::TcpListener` en el `IoDriver`.
  - `tcpAccept$`: Fast-path no bloqueante; en `WouldBlock`, encolar `AsyncTask` en el `IoDriver`.

- [x] **Tarea 3.2: Reescritura de `tcpConnect$`, `tcpRead$` y `tcpWrite$`**
  - `tcpConnect$`: Iniciar handshake no bloqueante y suspender sobre `Interest::WRITABLE`.
  - `tcpRead$` / `tcpWrite$`: Fast-path síncrono; fallback reactivo a `IoDriver`.

- [x] **Tarea 3.3: Reescritura de `tcpClose$` y `tcpCloseListener$`**
  - Desregistrar del reactor mediante comandos canalizados seguros `DriverCommand`.
  - Despertar tareas canceladas con valor `-1` o `""`.

---

## Fase 4: Validación y Benchmarks de Concurrencia

- [x] **Tarea 4.1: Suite de Tests de Integración HTTP y Servidor**
  - Ejecutar `tests/main.vn` para asegurar cero regresiones en los 79 módulos de tests (1114/0 aserciones).

- [x] **Tarea 4.2: Pruebas de Red y HTTP End-to-End**
  - Verificado servidor y cliente TCP y HTTP completo entre Isolates.

- [x] **Tarea 4.3: Matriz de 4 Oficial**
  - Árbol con JIT: 1114/0 PASSED.
  - Árbol sin JIT: 1114/0 PASSED.
  - Embebido con JIT: 1114/0 PASSED.
  - Embebido sin JIT: 1114/0 PASSED.

  - Validar Tree JIT, Tree NO_JIT, Embedded JIT, Embedded NO_JIT (1094/0 verde).
  - Actualizar binario global en `C:\Users\x\.cargo\bin\vn.exe`.
