# ADR-0010: Industrial Standard Library Foundations (Binary I/O, Compression, UDP, HTTP Streaming, KDF & MessagePack)

## Context
Following the elimination of obsolete aliases, redundant wrappers, and orphan root folders (`std/json`, `std/csv`, `std/buffer`), the Varn standard library needed to evolve from foundational capabilities into industrial-grade systems programming facilities.
Specifically:
1. **Binary I/O (`std:fs`, `std:io`)**: Lack of full `Bytes` support across file operations (`readBytes`, `writeBytes`, `appendBytes`) and lack of native asynchronous streaming reader/writer (`FileStream`) integrated into `Stream.pipe`.
2. **Binary Compression (`std:compress`)**: Previous APIs operated strictly on strings. A modern systems runtime requires direct in-memory gzip/gunzip and deflate/inflate operating directly over contiguous memory `Bytes`.
3. **Datagrams & Streaming Networks (`std:net`, `std:http`)**: Network capabilities only supported TCP. Modern networked applications (DNS, gaming, telemetry, VoIP, QUIC foundations) require UDP sockets (`UdpSocket`, `UdpPacket`). In addition, the HTTP server required first-class chunked response streaming (`HttpResponse.stream(reader: AsyncReader)`) without buffering infinite payloads in RAM.
4. **Cryptography & Security (`std:crypto`)**: Need for industry-standard password derivation (`PBKDF2-HMAC-SHA256`, `hashPassword`, `verifyPassword`), constant-time comparison (`timingSafeEqual`), and cryptographically secure random bytes returning canonical `Bytes`.
5. **Binary Serialization (`std:encoding`)**: Compact, high-throughput binary serialization specification (MessagePack) operating directly on `Bytes` alongside JSON and CSV in `std:encoding`.

## Decision
1. **Pilar 1: Binary I/O and Compression**:
   - Extended `runtime:fs` with native primitives `readFdBytes$`, `writeFdBytes$`, `readFileBytes$`, `writeFileBytes$`.
   - Added `readBytes`, `writeBytes`, and `appendBytes` to `File` and `std:fs` module functions.
   - Introduced `FileStream` implementing `AsyncReader` and `AsyncWriter`.
   - Upgraded `Stream.pipe` in `std:io` to return `Task<int>` with the total number of bytes transferred.
   - Migrated `std:compress` (`gzip`, `gunzip`, `deflate`, `inflate`) to accept `Bytes | str` and return contiguous `Bytes`.
2. **Pilar 2: UDP Sockets and HTTP Streaming**:
   - Added `runtime:net` bindings: `udpBind$`, `udpSendTo$`, `udpRecvFrom$`, `udpClose$`.
   - Implemented `UdpSocket` and typed `UdpPacket` (`{ data: Bytes, host: str, port: int }`).
   - Implemented `HttpResponse.stream(reader: AsyncReader)` using HTTP `Transfer-Encoding: chunked` for memory-efficient real-time delivery.
3. **Pilar 3: Cryptography KDF & Password Security**:
   - Added native host primitives: `pbkdf2$`, `timingSafeEqual$`, `randomBytesBuffer$`.
   - Implemented `pbkdf2(password, salt, iterations, keyLen): Bytes`.
   - Implemented `timingSafeEqual(a, b): bool` with constant-time XOR comparison against timing attacks.
   - Implemented `hashPassword` and `verifyPassword` using standard format `$pbkdf2-sha256$i=<iter>$<salt>$<key>`.
   - Updated `randomBytes` to return contiguous `Bytes`.
4. **Pilar 4: Binary Serialization (MessagePack)**:
   - Implemented `ByteWriter`, `ByteReader`, `MsgPack.encode(val: dynamic): Bytes` and `MsgPack.decode<T>(buf: Bytes): T` conforming to official MessagePack specification.
   - Supported fixint, u8, u16, u32, i64, fixstr, str8, str16, fixarray, array16, array32, fixmap, map16, map32, and bin8/16/32 for `Bytes`.

## Consequences
- Zero dynamic typing in public signatures: all contracts use explicit canonical types (`Bytes`, `int`, `str`, `Task<T>`, `AsyncReader`, `AsyncWriter`).
- Full end-to-end binary fidelity across filesystem, network, cryptography, and serialization.
- High efficiency and zero copy where possible through `Bytes` buffers.
