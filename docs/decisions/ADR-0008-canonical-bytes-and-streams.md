# ADR 0008: Tipo Canónico de Primera Clase `Bytes` y Streaming I/O

## Estado
Aprobado e Implementado.

## Contexto
Históricamente, los lenguajes de alto nivel como JavaScript acumularon décadas de deuda técnica en el manejo de memoria cruda (`ArrayBuffer`, `DataView`, `Buffer` de Node.js, `Uint8Array`, `Int8Array`, y guerras de especificaciones de Streams).

En Varn, la infraestructura anterior de red y E/S convertía implícitamente los bytes leídos de los sockets a cadenas de texto UTF-8 mediante `String::from_utf8_lossy` en el driver de host. Esto:
1. Corrompía transferencias binarias no UTF-8 arbitrarias (reemplazando secuencias binarias con el carácter de reemplazo `\u{FFFD}`).
2. Incurría en asignaciones innecesarias de strings y validaciones UTF-8 costosas.
3. Carecía de un tipo canónico de memoria contigua no sujeta a GC innecesario.
4. Impedía el streaming continuo con contrapresión (*backpressure*).

Bajo las reglas arquitectónicas del lenguaje (Regla 1: Prohibición absoluta de alias; Regla 10: Breaking changes favorables; Regla 19: Búsqueda del pináculo arquitectónico), se diseñó e implementó una solución sin deuda técnica: un único tipo canónico `Bytes` integrado en todos los niveles del compilador y la VM, junto con contratos limpios de streaming.

## Decisiones

### 1. `Bytes` como Tipo de Primera Clase Canónico
- Se formalizó `Bytes` como tipo intrínseco en todo el compilador:
  - `varn-core`: `TypeTag::Bytes`, `IntrinsicType::Bytes`, y registro en `CORE_CLASSES`.
  - `varn-tir`: `BackendTy::Bytes`.
  - `varn-compiler`: `HirType::Ref` con `TypeTag::Bytes` para integración con el recolector de basura y el compilador a bytecode SSA / JIT.
  - `varn-types`: `Value::Buffer(VmBuffer)` con trait implementations de hashing, igualdad y representación.
  - `varn-vm`: Representación en heap como `HeapObj::Buffer`, soporte de indexación directa `bytes[idx]` y `bytes[idx] = val` (0..255) y acceso a `.length`.
  - `core:bytes` (`crates/varn-builtins/src/modules/primitives/bytes/`): Declaración nativa con despacho directo vía op-id:
    - `.length: int`
    - `.getByte(index: int): int`
    - `.setByte(index: int, value: int): void`
    - `.slice(start?: int, end?: int): Bytes`
    - `.toString(): str`
    - `.toHex(): str`
    - `.toBase64(): str`
    - `.fill(value: int, start?: int, end?: int): Bytes`
    - `.copy(target: Bytes, targetStart?: int, sourceStart?: int, sourceEnd?: int): int`
    - `Bytes.alloc(size: int): Bytes`
    - `Bytes.from(data: dynamic): Bytes`
    - `Bytes.fromString(s: str): Bytes`
    - `Bytes.fromBytes(bytes: int[]): Bytes`

### 2. Eliminación de la Corrupción UTF-8 en Sockets de Red
- En `crates/varn-builtins/src/modules/host/net/driver.rs`, se eliminó `String::from_utf8_lossy`.
- Las lecturas de socket (`tcpRead$`) ahora devuelven `Value::Buffer(buf)` crudo o `Value::Null` en EOF.
- Las escrituras (`tcpWrite$`) aceptan tanto `Bytes` como `str` (extrayendo `.as_bytes()`), enviando bytes directamente al driver del sistema operativo.
- `TcpStream.read(size)` en `std:net` retorna `Task<Bytes?>`, garantizando transparencia binaria absoluta.
- `TcpStream.readText(size)` provee un helper de conveniencia para decodificar texto explícitamente vía `.toString()`.

### 3. Contratos de Streaming I/O y Backpressure (`std:io`)
- En `std:io`:
  - `Reader`: Interfaz con `read(size?: int): dynamic`, `readAll(): dynamic` y `pipeTo(dst: Writer): void`.
  - `Writer`: Interfaz con `write(content: dynamic): dynamic`.
  - `Stream.pipe(reader, writer, bufferSize)`: Tubería asíncrona con contrapresión natural garantizada por el modelo `await`, transfiriendo chunks sin saturar memoria.

## Consecuencias
- **Transparencia Binaria 100%**: Cualquier secuencia binaria arbitraria (incluyendo bytes nulos `0x00`, `0x80..0xFF`) se transmite y manipula sin alteración.
- **Rendimiento**: Cero conversiones innecesarias de cadenas en el transporte de paquetes.
- **Coherencia Arquitectónica**: Varn evita por completo los 15 tipos redundantes de JS (`Buffer`, `ArrayBuffer`, `SharedArrayBuffer`, `DataView`, `Uint8Array`, etc.); `Bytes` es la única abstracción canónica.
- **Compatibilidad y Verificación**: 112 suites de pruebas (2408+ tests) pasando al 100% en verde con `vn test` y `cargo test`.
