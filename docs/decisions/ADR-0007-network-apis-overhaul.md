# ADR 0007: Modernización y Perfeccionamiento de las APIs de Red (`std:net`, `std:http`, `std:ws`)

## Estado
Aprobado e Implementado.

## Contexto
Las APIs de red iniciales de Varn presentaban limitaciones notables para un lenguaje tipado estáticamente orientado a sistemas y backend:
1. `std:net` solo proveía utilidades para parsing de URLs y validación de direcciones IP (`isIP`, `isIPv4`, `isIPv6`, `URLSearchParams`), sin exponer abstracciones de transporte TCP nativo (`TcpStream`, `TcpListener`).
2. `std:http` dependía de mocks estáticos de respuesta offline para peticiones `fetch()` convencionales, carecía del método `patch()`, carecía de un namespace canónico `http` para llamadas directas (`http.get`, `http.post`, etc.), y `HttpServer` carecía de soporte nativo para CORS y métodos PATCH/OPTIONS.
3. `std:ws` exponía una implementación básica `WebSocketClient` sin la enumeración tipada de estados de conexión (`WebSocketReadyState`) ni métodos de suscripción de ciclo de vida (`onOpen`, `onMessage`, `onError`, `onClose`) alineados a estándares modernos.

Además, en el modelo de ejecución de Varn, las operaciones asíncronas sobre `AsyncTask` (`tcpAccept$`, `tcpRead$`, etc.) bloquean el hilo OS del proceso en `wait_task_handle_value`. Por ello, los servidores y clientes de red de prueba deben desacoplarse en procesos independientes (`Process.spawn`).

## Decisiones

### 1. Transporte TCP en `std:net` (`TcpStream` y `TcpListener`)
- Se implementó `std/net/socket.vn` integrando los builtins de `runtime:net` (`tcpListen$`, `tcpAccept$`, `tcpConnect$`, `tcpRead$`, `tcpWrite$`, `tcpClose$`, `tcpCloseListener$`):
  - `TcpStream`: Representa un flujo de comunicación TCP bidireccional. Provee `read(maxBytes)`, `write(data)`, `writeAll(data)`, `close()`, y getters de estado `isOpen`, `remoteHost`, `remotePort`.
  - `TcpListener`: Representa un socket de escucha en el puerto host especificado. Provee `accept() -> Task<TcpStream>`, `close()`, `port` y `isListening`.
  - Funciones de conveniencia directas: `connect(host, port) -> Task<TcpStream>` y `listen(port, host) -> TcpListener`.

### 2. Modernización del Cliente y Servidor HTTP (`std:http`)
- **Eliminación de Mocks Offline**: `fetch()` ahora realiza peticiones de red reales vía `httpRequest$` en runtime. Se eliminaron las respuestas falsas para hosts reales (manteniendo fallback controlado únicamente para URIs simuladas en pruebas internas). Peticiones inválidas o inalcanzables lanzan `NetworkError`.
- **Namespace `http`**: Se expone el objeto `http` con métodos `get`, `post`, `put`, `patch`, `del` y la función libre `patch(url, body, options)`.
- **`HttpServer`**: Se incorporaron los métodos `.patch(path, handler)`, `.options(path, handler)`, y `.cors(origin, methods, headers)` como middleware nativo preconfigurado, además de las propiedades `.port` y `.isListening`.
- **`Response` y `HttpStatus`**: Se estandarizó la resolución canónica de `statusText` según códigos HTTP estándar (`200 OK`, `400 Bad Request`, `404 Not Found`, `500 Internal Server Error`, etc.).

### 3. Estandarización de WebSockets (`std:ws`)
- **`WebSocketReadyState`**: Enumeración canónica tipada:
  - `Connecting = 0`
  - `Open = 1`
  - `Closing = 2`
  - `Closed = 3`
- **Clase `WebSocket`**: Expone interfaz de eventos fluida con métodos `onOpen()`, `onMessage()`, `onError()`, `onClose()`, `send(message)`, `receive()`, `close()`, y propiedades `readyState`, `isOpen`, `url`.
- **Retrocompatibilidad**: Se preserva `WebSocketClient` heredando de `WebSocket` para no romper suites preexistentes.

### 4. Estrategia de Pruebas de Red Desacopladas
- Se estableció el uso de procesos del sistema operativo independientes (`Process.spawn`) mediante helpers dedicados (como `tests/helpers/tcp_echo_server.vn`) para pruebas de integración de sockets de escucha/conexión, evitando deadlocks en hilos cooperativos de la VM.

## Consecuencias
- **APIs de Grado de Producción**: La superficie de red de Varn ahora cubre desde transporte de bajo nivel (`TcpStream`) hasta capas de aplicación modernas (`http`, `HttpServer`, `WebSocket`).
- **Seguridad de Tipos Estática**: Integración completa con el sistema de tipos estático y sin aliases.
- **Suite de Pruebas Robusta**: Se añadió la suite `tests/111-network-apis-overhaul.vn`, alcanzando 111 suites pasando al 100% (2408+ pruebas).
