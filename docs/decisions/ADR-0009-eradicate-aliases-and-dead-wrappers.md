# ADR 0009: Erradicación Total de Alias y Wrappers Redundantes en la Stdlib

## Estado
Aprobado e Implementado.

## Contexto
Varn es un lenguaje de diseño moderno sin bases de código heredadas en producción que requieran compatibilidad hacia atrás ni deuda técnica artificial. Mantener alias de compatibilidad (`WebSocketClient`, `HttpRequest`, `Buffer`, módulos raíz dispersos como `std:json` y `std:csv`, funciones de hash que duplicaban encoders) violaba los principios fundamentales del lenguaje:
- **Regla 1**: Prohibición absoluta de alias de tipos.
- **Regla 10**: Breaking changes válidos y bienvenidos para mejorar la arquitectura.
- **Regla 19**: Búsqueda del pináculo arquitectónico ("el deber ser").

## Decisiones

### 1. Eliminación de Clases y Subclases Alias
- **`std:ws`**: Eliminada la clase alias `WebSocketClient`. La única clase canónica y exportada es `WebSocket` (junto al enum `WebSocketReadyState`).
- **`std:http`**: Eliminada la subclase alias `HttpRequest`. La única clase canónica para peticiones entrantes es `Request`.

### 2. Erradicación del Wrapper `Buffer` y Módulo `std:buffer`
- Eliminado completamente el módulo `std:buffer`. La abstracción canónica de memoria contigua en Varn es el tipo de primera clase `Bytes`. Ningún wrapper superficial estilo Node.js es necesario ni tolerado.

### 3. Consolidación Estricta en `std:encoding`
- Eliminadas las carpetas huérfanas `std/json` y `std/csv`.
- Implementaciones canónicas alojadas directamente en:
  - `std:encoding/json` (`Json`, `JSON`)
  - `std:encoding/csv` (`CSV`, `CsvOptions`)
  - `std:encoding/toml` (`TOML`, `parseToml`, `stringifyToml`)
  - `std:encoding/base64` (`Base64`, `base64Encode`, `base64Decode`)
  - `std:encoding/hex` (`Hex`, `hexEncode`, `hexDecode`)
- El módulo paraguas `std:encoding` es el único punto de entrada unificado para formatos y codificaciones.

### 4. Depuración de Utilidades Duplicadas
- Eliminadas las funciones `base64Enc` y `base64Dec` de `std:crypto/hash.vn` y `std:crypto/mod.vn` (la codificación no pertenece a criptografía; reside en `std:encoding`).
- Eliminada la función `parseArgs` de `std:sys/args.vn` y `std:sys/mod.vn` (el procesamiento de argumentos de CLI pertenece canónicamente a `std:cli`).

## Consecuencias
- Cero deuda técnica ni nombres duplicados en la superficie pública.
- Una única forma canónica de hacer cada tarea en la biblioteca estándar.
- Las 112 suites de pruebas (2408+ tests) se ejecutan y pasan al 100% en verde con `target/debug/vn.exe test` y `cargo test`.
