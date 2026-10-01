# STDLIB Ultra Profesional — Memoria de Diseño Varn

Fecha: 2026-09-30
Objetivo: la STDLIB mejor diseñada jamás hecha. Sobrevive décadas sin cambios. Rendimiento casi nativo con llamadas al host mínimas pero eficientes. Crear más módulos stdlib o implementación en Rust con casi zero boilerplate.

## 0. Estado verificado

### 0.1 Mecanismo host actual (bueno, conservar)

- 1 función Rust + 1 línea `declare` en `contract.vn` = 1 función Varn.
- Macro `varn_contract!` en `crates/varn-op-macros/src/varn_contract.rs:224` genera:
  - `__varn_wrap_*` (boxed ABI `ctx, args: &[VmValue]`)
  - `__varn_fast_wrap_*` solo si escalar (ver 0.3)
  - `NativeOpEntry` en sección linker `.varn_ops`
  - `__VARN_LINK_MARKER_*` para fallback
  - trait que chequea firma Rust contra contrato `.vn` en tiempo de compilación.
- Cero registro manual por función. Registro por módulo en `crates/varn-builtins/src/modules/mod.rs:58` (`force_link_builtins`) + colecta en `crates/varn-builtins/src/dispatch.rs:87` (`all_native_ops`).
- Marshal en `crates/varn-types/src/marshal.rs:20`: escalar `i64/f64/bool` = 1 cmp + mov, cero alloc. `&str` param = `VnStr.as_str()`, cero copia. `VnArray` = solo check, cero copia. Retorno `String/Vec<VmValue>` = siempre `alloc_str_owned/alloc_array`, heap + GC root.
- `this:&VnStr/VnArray` cero copia. `Option<T>` = null check. `&[VmValue]` para rest. `Function` retorna `Result<T,String>`, `Method` retorna `T` o `Result<T,NativeError>` si `@fallible`.
- Nuevo módulo hoy = nuevo dir + `.rs` + `_runtime.vn` + 1 línea en `modules/mod.rs` + entrada `build.rs` registry + `std/*/*.vn` wrapper. Sin FFI C, sin `unsafe` en autor. Scatter de 5 toques manuales: ese es el boilerplate a eliminar, no la función individual.

### 0.2 Costo real por llamada host (medido por código)

- ABI boxed universal: todo arg/ret pasa como `VmValue{tag,payload}` 16B.
- Interp `CallNativeOp`: `native_op_fn(op_id)` → `FxHashMap<u64,DispatchEntry>` lookup por ejecución + `box_reg` receiver + `Regs` view + `buf[17]` copia + indirect `fn(ctx,args)` + `unbox_into_reg`. Ver `crates/varn-vm/src/exec/dispatch/mod.rs:570`.
- JIT evita hash (`func_ptr` bakeado en `crates/varn-jit/src/clif/from_ssa/call.rs:717`) pero paga `boxed_window` (box por arg a scratch 16B) + `jit_call_native_window` extern C + slice + `invoke_native`.
- `dyn NativeCtx` = llamada virtual por heap op. Loops (`str join/split`, `array map/filter/reduce/sort`) hacen N virtuales + N copias 16B + `str_repr_borrowed` por elemento. `map/filter/forEach` peor: `ctx.call_vm(callback)` por elemento = frame push + dispatch completo por iteración. Sin fusión.
- Profiling on = impuesto extra (`native_op_name_by_fn` hash + `rdtsc` por llamada).
- Retorno string/array siempre alloca. `split/lines/words/join/concat/splice` allocan `String` + N `alloc_str` + `alloc_array`.
- Consecuencia: N llamadas pequeñas en loop Varn dominan costo, no el trabajo útil. Regla de oro: 1 llamada bulk >> N llamadas chunk.

### 0.3 Fast path escalar existe pero muerto para bulk

- `raw_func_ptr` + `SignatureDescriptor` generados en `varn_contract.rs:462` pero `call.rs` solo usa `func_ptr`. Fast path real solo 5 helpers str + vtable 8 entradas.
- `is_fast_eligible` en `varn_contract.rs:739`: solo `Function|StaticMethod`, retorno escalar, params escalares, no fallible, no rest. `Str/Bytes/Array` excluidos aunque serían los más rentables en bulk.
- Acción: conectar `raw_func_ptr` y extender elegibilidad a bulk `Str/Bytes` con firma fija, no solo escalares.

### 0.4 Inventario stdlib `.vn` (auditoría 2026-09-30)

Estructura: 27 dirs en `std/`, ~90 archivos `.vn`, 24 `mod.vn` (19 re-export puro ~120L).

Duplicados verificados:
- `std/encoding/json.vn` vs `runtime:json`; `std/encoding/csv.vn` vs `runtime:csv`; `std/env/env.vn` vs `std/sys/env.vn`.
- `read/readText`, `write/writeText/writeBytes/appendText` en `std/fs/ops.vn`, `std/fs/file.vn`, `std/io/stream.vn` con firmas `read` incompatibles (`File` sync str vs `TcpStream` async `Bytes?`).
- `hex` x3: `std/crypto/password.vn:hexNibble`, `std/encoding/hex.vn:byteToHex`, `Bytes.toHex` usado en `std/http/response.vn:195`.
- `trimWhitespace/parseInteger/parseFloating` en `std/encoding/toml.vn:1` reinventan `str.trim/int.parse`.
- `Url.parse` con 10 ramas char en `std/net/url.vn:56` vs `int.parse` en resto.
- `path/utils.vn:269L` loops `keepTrimming/keepSearching` manuales para `normalize/dirname/basename/join/resolve`.
- Loop `keepReading/keepCopying/while running` copiado 7x: `std/fs/file.vn:62,84`, `std/fs/ops.vn:95`, `std/http/request.vn:28`, `std/http/response.vn:53,188`, `std/net/socket.vn:65`.
- `for(let i=0;i<len;i=i+1)` estilo C ~40x donde `for...of` vale.
- `import runtime:net` repetido en `std/http/client.vn:1`, `std/http/server.vn:1`, `std/http/request.vn:1`, `std/net/socket.vn:1` sin fachada única.
- `mod.vn` wrappers extra: `std/path/mod.vn` 11L ns 1:1, `std/http/mod.vn` 6L `const Http`, `std/test/mod.vn` ~20L, `std/http/client.vn` 20L clon, 6 alias 1:1 en `std/encoding/*`.

Inconsistencias naming:
- `Json` type vs `JSON` ns vs `CSV` vs `CsvOptions` vs `TOML` vs `MsgPack` vs `Base64/Hex` vs `Url` vs `URLSearchParams` vs `Query`.
- `del` (`std/http/client.vn:121`) vs `delete` (`std/net/query.vn:96`, `std/collections/lru.vn:57`).
- `read/readText/readAll/readLines`, `write/writeText/writeBytes/appendText`, `get/getInt/getBool/getAll`.
- `parseQuery/buildQuery` vs `Query.parse/fromObject` vs `new URLSearchParams`.
- `TaskGroup/TaskGroupImpl/task_group` triple mismo objeto (`std/task/group.vn:78`).
- `HttpResponse` (server builder) vs `Response` (fetch result) en `std/http/response.vn:82,239`.
- `File` sync str vs `FileStream/TcpStream` async `Bytes?`.
- `Task<T>` builtin vs `TaskHandle<T>` (`std/task/combinators.vn:13`).

Errores: coherencia nula.
- `throw new Error` 52 lugares, prefijos ad-hoc: `TcpWriteError:`, `NetworkError:`, `MsgPack:`, `413 Payload Too Large`, `invalid hex`, `ASSERT FAIL:`.
- Mismo fallo dos estilos: `Semaphore/LRUCache` throw si capacity<=0, `Database` cerrado retorna `0/[]/null` silencioso (`std/sqlite/db.vn:23`), `List.get/removeAt/Queue.dequeue` retornan `null`, `JSON.tryParse/TOML.tryParse` null, `fetch` throw, `parseRawResponse` null.
- `verifyPassword` retorna `false` con hash malformado (`std/crypto/password.vn:28`).

Clases vs funciones sin regla:
- Puras clases: `collections/*`, `TcpStream/TcpListener`, `Database`, `Logger`, `Mutex/Semaphore/WaitGroup`.
- Namespace+alias libre duplicado: `Base64/Hex/TOML`, `Prompt/prompt/confirm/password`, `CLI.parse/parseArgs`, `Process.exec/spawn`.
- Híbrido: `fs` free + `File/FileStream`, `http` free + `Request/HttpResponse/Response/HttpServer`, `task` free + `Task` ns + 4 clases sync.
- `io/mod.vn`: `Reader/Writer` sync str vs `AsyncReader/AsyncWriter` async `Bytes`, dos jerarquías paralelas.

### 0.5 Restricciones arquitectura compilador (no negociables)

- Frontera: `parser -> checker --emite--> tir <--consume-- compiler`. Checker no depende de compiler ni revés. Hablan vía `varn-tir` + `varn-core`. Ver `docs/COMPILER_ARCHITECTURE.md`, `docs/TIR_CONTRATO_TIPADO.md`.
- Tipos estables `BackendTy` (Copy, sin Default): solo `Int(i64)/Float(f64)`. `Str,Bytes,Decimal,BigInt` = referencia. `Class(ClassId), Enum(EnumId), Fn(SigId), Array/Map/Set/Tuple, Nullable(TyId), Void, Never, Dynamic(DynReason)`. `Nullable` conserva nulabilidad. `Dynamic` exige razón (`HostBoundary,Union,IndexSignature,Unannotated,NotYetSupported`).
- Regla stdlib: anotar todo público. Cero `Unannotated`. Cero unión expuesta `A|B`. Cero `{[key:str]:T}`. Enum/class con ID estable. `int?` OK, `A|B` no. `as` explícito entre dominios.
- `TirExpr{kind,ty,res,span}`. `Resolution`: `None,Local,Param,Upvalue,GlobalSlot,ModuleSlot,FieldSlot,StaticField,VtableSlot,DirectFn,Intrinsic,NativeOp,EnumVariant,ByName`. Favorecer resoluble estático: clases cerradas, métodos no dinámicos, globales numerados, intrínsecos vía `Intrinsic` no `strcmp` dispatch. Evitar spread/named en API caliente. Evitar `?.`/`match` complejo en hot path si deja `ByName/Dynamic`.
- `FunctionProto{code, constants, register_count, upvalue_count, state_size}`. Caché disco invalida entera al cambiar forma; bump versión, sin migración.
- `VmValue` dos palabras, heap tabla índices, slots 16B. `Array::push` sentencia → opcode `ArrayPush` dedicado (evita ventana contigua + dump JIT).
- Generadores/async siguen interpretados, sin OSR. Pequeñas hojas puras (`is_leaf`) inlineadas gratis. Bucles `for` con inducción `int` nativa para LICM/JIT bounds-check elim.
- Leyes: IO invertido, ids internos no cruzan módulos (interfaz portable nombres+sintaxis), una tabla un dueño, determinismo (`IndexMap/BTreeMap`, mismo input→mismo bytecode), diagnósticos acumulativos, una verdad por hecho, match exhaustivo sin `_=>`, absolutista no parche, commits atómicos, breaking solo con ganancia medible. Tamaño >400 modularizar dominio. Sin `utils/helpers` genérico.
- Validación canónica `tests/main.vn`, no `cargo test`. Matriz 4 cuadrantes: `dev-checkout/@embedded × JIT/VARN_NO_JIT=1`. `scripts\verify.ps1 -Fast`. Caché sensible (`VARN_CACHE_DIR` limpio para aislar Ley2).
- Ley 11: zero comentarios de código, zero tests unitarios. Solo suite completa.

## 1. Principios ultra (contrato décadas)

1. Capas fijas, nunca romper:
   `intrínseco JIT (str/array/math)` → `runtime:host (fd, socket, clock, rand)` → `std/*.vn (pura Varn, compone política)`.
2. Núcleo mínimo estable: solo `BackendTy` portable. Lo que no es `BackendTy` no entra al núcleo.
3. Una verdad por hecho: un `copy`, un `hex`, un `Error`, un `Reader`. Borrar caminos paralelos en el mismo esfuerzo (Ley 8).
4. Bulk primero: `std/*.vn` nunca hace loop byte-a-byte sobre host. Host expone bulk/stream. Varn compone.
5. Determinismo: mismo input → mismo bytecode. Sin `HashMap` estándar en orden visible. Sin estado global mutable sin dueño.
6. Breaking permitido solo con ganancia medible (Ley 10): declarar ganancia + verificación + qué se borra.

## 2. Taxonomía canónica propuesta

```
std:core      → Result, Option, Disposable, Encoding base (utf8/hex/base64), Assert
std:fs        → File, Stats, readText/writeText/appendText/readBytes/writeBytes/copy/move/stat/list
std:io        → Reader, Writer, AsyncReader, AsyncWriter, Stream.pipe, stdin/stdout/stderr
std:path      → join/resolve/dirname/basename/normalize (delega runtime, no loop Varn)
std:net       → TcpListener/TcpStream/UdpSocket, Url, Query (fachada única runtime:net)
std:http      → Request/Response/Headers unificados fetch+server, serve()
std:encoding  → JSON/CSV/TOML/MsgPack con misma forma parse/stringify/tryParse
std:crypto    → sha256/hmac/aes/pbkdf2/uuid/randomBytes (Bytes in/out)
std:collections → List/Stack/Queue/PriorityQueue/LRU (una forma add/remove/toArray)
std:task      → spawn/sleep/parallel/channel/TaskGroup/Mutex/Semaphore (un handle)
std:time      → Instant/Duration/PlainDateTime (un reloj)
std:sys/process/env → unificado: args/env/cwd/platform/spawn (hoy triplicado sys/env/process)
std:reflect/test/log/cli/markdown/compress/sqlite/ws → dominios hoja, misma regla errores
```

Fusiones obligatorias:
- `encoding/json` + `runtime:json` → una forma.
- `env/env` + `sys/env` + `process` → `std:sys` único.
- `fs/ops` + `fs/file` + `fs/stats` + `io/stream` (parte Reader/Writer) → `std:fs` coherente + `std:io` contratos.
- `http/client` + `http/server` + `net/socket` → `Request/Response` únicos.

## 3. Reglas API (estilo ultra)

- Tipos `PascalCase`, fns/vars `camelCase`, consts globales `UPPER`. Namespaces `PascalCase` (`JSON`, `CSV`, `TOML`, `Base64`, `Hex` → unificar a uno: o todo UPPER acronym o todo Pascal; propuesto: `Json`, `Csv`, `Toml`, `MsgPack`, `Base64`, `Hex`, `Url`).
- Verbos únicos globales:
  - fs: `open/read/write/close/stat/list/copy/move/remove`
  - texto: `readText/writeText/appendText/readLines`
  - binario: `readBytes/writeBytes`
  - codec: `parse/stringify`, `encode/decode`, `tryParse` (retorna null solo aquí, documentado)
  - net/http: `fetch/get/post/serve/listen`, `get/set/has/delete/append` (solo `delete`, borrar `del`)
- `Bytes` para IO/socket/compress/crypto. `str` solo texto validado UTF-8.
- Una jerarquía IO:
  ```
  Reader { read(size?: int): str; readAll(): str; pipeTo(dst: Writer): void }
  Writer { write(content: str): int }
  AsyncReader { read(size?: int): Task<Bytes?> }
  AsyncWriter { write(data: Bytes): Task<int>; flush(): Task<void>; close(): void }
  ```
  No duplicar. `File implements Reader+Writer`, `FileStream implements AsyncReader+AsyncWriter`, `TcpStream` igual que `FileStream`, no firma distinta.
- Clases cerradas, jerarquía sin nativa opaca, métodos no dinámicos, layout+vtable fija publicada.
- Anotar todo público. Cero `any`/unión expuesta. `T?` para nulable, no `T|null` unión libre.

## 4. Reglas errores (unificar en un paso por dominio)

Hoy: 52 throws ad-hoc + null silencioso mezclados. Propuesto canónico:
```
class StdError { code: str; msg: str; cause?: StdError }
type Result<T> = T | StdError  // o Result<T,E> core cuando exista
```
- Códigos `E_FS_NOT_FOUND`, `E_FS_DENIED`, `E_NET_TIMEOUT`, `E_CODEC_INVALID`, etc. Nunca mensaje libre como código.
- `tryParse` → `null` documentado. Resto → `throw StdError`, nunca `null/0/[]` silencioso.
- `stat/copy/read` fallido siempre throw, nunca `false`/`null`. `exists` es la única que retorna `bool` sin throw.
- Migración por dominio completo, no por función (Ley 8). Paso 1 fs mantiene throw `String` por compat suite; paso errores migra todo fs+io junto.

## 5. Reglas rendimiento (llamadas mínimas, casi nativo)

- Bulk obligatorio en host (Rust), no loop Varn:
  - `readFileText/writeFileText/appendFileText/copyFile/readLines` (1 call vs N+2)
  - `readFdAll/readFdAllBytes` (File.readAll/pipeTo en 1 call)
  - `hashFile`, `readDir`, `stat` ya bulk, conservar
  - `split/join` bulk para `readLines`, no `content.split("\n")` + loop trim en Varn
- Firmas zero-copy: `(&str, VnArray, &[u8], this:&VnStr)` dentro. `String/Vec` solo en borde.
- Evitar `str_repr/array_get/set` por elemento. Usar batch o slice nativo.
- Prohibir callback Varn por elemento en hot path. Proveer versión nativa con op o clave.
- Preferir `ArrayPush` opcode a `CallNativeOp` genérico. Hojas puras pequeñas para inline. `for i<int` para LICM.
- Conectar `raw_func_ptr`: extender `is_fast_eligible` a `Str/Bytes` bulk con firma fija. Medir con `bench -v` (77k calls hoy, mayoría sin atribuir por falta de nombre en JIT).
- `copy` nunca lee/escribe chunks en Varn. `readLines` nunca `read` + `split` + loop en Varn. `pipeTo/Stream.pipe` streaming real: loop solo cuando es stream infinito, no para archivo finito.

## 6. Zero boilerplate (crear módulo en 1 comando)

Hoy 5 toques manuales. Objetivo 1 comando:
```
xtask std-scaffold <mod>  →  crates/.../<mod>.rs + contract.vn + std/<mod>/{mod.vn,*.vn} + registro mod.rs + build.rs
```
- Única fuente: `contract.vn` manda. `varn_contract!` ya valida firma Rust contra contrato en compile-time.
- Generar `mod.vn` re-export y `force_link_builtins` desde registry único, no a mano.
- Generar fachada `std/*.vn` wrapper desde contrato (thin delegate, sin lógica).
- Fachada única por dominio: un `import runtime:X` por dominio, no 4 repetidos.
- Sin `utils/helpers` genérico. Nombre declara dominio. >400L modularizar.

## 7. Roadmap por pasos atómicos (un commit por paso)

- Paso 1 (este esfuerzo): piloto `std:fs` bulk + dedup wrappers, API compatible. Host: `readFileText$/writeFileText$/appendFileText$/copyFile$/readLines$/readFdAll$/readFdAllBytes$`. Varn: `ops.vn` y `file.vn` delegan thin, sin loops. Sin cambio errores, sin cambio firmas públicas. Verificación: suite 4 cuadrantes verde.
- Paso 2: unificar `Reader/Writer/AsyncReader/AsyncWriter` + `Stream.pipe` bulk para archivo finito, streaming solo para socket. Fusionar `io/stream` contratos en un sitio. ESTADO 2026-09-30: IMPLEMENTADO. `std/io/stream.vn` sitio único 4 contratos + `BulkPump`; `std/io/mod.vn` re-export puro; `Stream.pipe` fast path `pumpTo` vía probe `dynamic` + loop canónico + buffer 8192; `FileStream implements BulkPump` + `readAll` bulk. Suite 4 cuadrantes verde 2002/0.
- Paso 3: unificar errores `fs+io` a `StdError` con códigos. Migrar `Database/List/JSON.tryParse/fetch` en mismo patrón después, dominio por dominio. ESTADO 2026-09-30: IMPLEMENTADO fs+io. Nuevo `std:error/mod.vn`: `StdError extends Error` + `code/message` + consts `E_FS_*`/`E_IO_*` + `stdError()` factory + `toStdError()` traductor idempotente. Rust `fs.rs`: `coded()/fs_io_err()/denied()` fuente única, `ErrorKind::NotFound/Denied/Exists` → códigos, `BAD_FD/BAD_ARG/ENCODING`; `io.rs`: `E_IO_FLUSH/E_IO_READ`. Wrappers `ops.vn/file.vn/stream.vn` try/catch thin. Suite 4 cuadrantes verde 2002/0. Restan otros dominios con `Error` libre.
- LÍMITE CHECKER VERIFICADO (cross-módulo): herencia `extends` invisible entre módulos. `throw new StdError` cross-módulo → VN4008; `.message` tras narrowing `instanceof StdError` → VN3004. Solución canónica: factories retornan `Error` estático (`stdError/toStdError`), valor runtime `StdError`; shadowing `message: str` en subclase para acceso estático; campo asignado en `try` se vuelve `T?` en usos → init `= -1` en declaración. No tocar checker en pasos stdlib.
- Paso 4: naming canónico global (`delete` no `del`, `Json/Csv/Toml` no `JSON/CSV/TOML`, `Response` único, `Task` único). Codemod + suite verde. ESTADO 2026-09-30: IMPLEMENTADO. Extiende ADR-0009 (colocación) con capa naming. Tabla: `Json(ns)+JsonValue(tipo)`, `Csv`, `Toml`, `Cli`, `Uuid`, `Ffi`, `Query` única (borrada `URLSearchParams` + `parseQuery/buildQuery`, `valuesList→values`, `parseURL→parseUrl`), `HttpResponse→ServerResponse`, borrados `http` ns + `Http` objeto + `server()` + `Task.sleep/delay` + `TaskGroupImpl/task_group` (clase `TaskGroup`, uso con `new`) + `parseArgs/prompt/confirm/password/isatty` libres + `uuidV4/isValidUuid` + `base64Encode/hexEncode/parseToml` libres + `read/write` fs libres + `env/setEnv/parseDotEnv` sys + `testObj/assertSummary` + `sep→SEP/delimiter→DELIMITER`. Excepción documentada: libre `del` (delete reservado en gramática para fns libres; métodos sí usan `delete`). `TaskHandle` intacto (builtin compilador, diseño intencional). Suite 4 cuadrantes verde 2002/0.
- LÍMITES GRAMÁTICA/CHECKER (fase 4): tipo+ns mismo nombre colisiona en posición tipo (ns gana; tipo necesita nombre propio → `JsonValue`); clase+fn mismo nombre = duplicado (factory `TaskGroup()` eliminada); `delete` reservado como fn libre (VN2007) pero válido como método; `new TaskGroup()` sin genéricos no infiere T (anotar `<int>`); campo asignado en `try` se vuelve `T?` (init en declaración).
- Paso 5: `xtask std-scaffold` + registry único + `mod.vn` generado. Eliminar `force_link` manual. ESTADO 2026-09-30: IMPLEMENTADO. `cargo xtask std-scaffold <mod> [--class Name] [--dry-run]` en `crates/xtask/src/scaffold.rs`: genera `.rs` contrato + `_runtime.vn` + `std/<mod>/mod.vn` + inserta registro en `modules/mod.rs` (único toque manual restante, ahora automático; `build.rs` ya autodetecta `std/`). Verificado con `--dry-run`. Corrección a auditoría: no existe registry en `build.rs` (autodescubre).
- Paso WS (extra): SUPERADO 2026-09-30 (ver Paso 8 abajo). Diseño final: plano de datos 100% Varn-async sobre driver existente, sin hilos ni bloqueo; tungstenite eliminado.
- Paso 6: conectar `raw_func_ptr` + extender fast path a bulk `Str/Bytes`. Medir `bench -v` antes/después. ESTADO 2026-09-30: IMPLEMENTADO por mecanismo canónico (helpers `jit_slow` por op-id, no sistema `raw` paralelo — Ley 8). `includes`: 121ms→33ms por 1M calls JIT (~3.7x, ~33ns/call, a la par de `startsWith`). Cambios: `str_includes_op_id` (`varn-core/src/op_id.rs`), `jit_str_includes` (`intrinsics.rs`, reusa `find_bytes` = cuerpo del contrato), ramas op-id + lane dinámico por nombre (`call.rs`). Paridad JIT/interp verificada en bordes (vacío, unicode, no-str→throw). Nota: `raw_func_ptr` genérico sigue muerto por diseño (ABI C inestable para `&str`/fat pointers); el camino es helpers dedicados.
- Paso 7: fusiones restantes (`env/sys/process`, `encoding/json`, `http/net`, `path` delega runtime). ESTADO 2026-09-30: COMPLETO. `path` → `runtime:path` COMPLETO: `normalize/dirname/basename/extname/isAbsolute/join/resolve/sep/delimiter` en host (transliteración exacta del algoritmo Varn, 33/33 casos paridad incl. drives/`..`/raíz), `std/path/utils.vn` thin delegates, 269L loops Varn borrados. `env/sys` ya coherente tras fase 4 (sin duplicados; fusión física = churn sin ganancia, no hacer). `http/net` Request/Response unificado COMPLETO (ver Paso 8).

## 8. Paso 8 — Response unificado + ws async + WsServer (2026-09-30, COMPLETO)

- `Response` único: borrada `ServerResponse`; `Response` gana `cookies/setCookie/bodyStream/stream()`; middleware `(req, next)=>Response?`; handler debe retornar `Response` (500 si no, 404 si null/sin handler); `sendHttpResponse$` gana `cookies: str[]` (serialización única en host; arregla bug latente: cookies se perdían en path return); chunked en servidor (`_sendChunked`).
- WS async sin cirugía mio: plano de datos 100% Varn-async sobre `tcpConnect/tcpRead/tcpWrite/tcpClose/tcpAccept/tcpListen`; host solo primitivas puras (`wsKey$`, `wsAcceptResponse$`, `wsValidateResponse$`, `wsFrameBytes$` maskeado, `wsParseFrame$` con unmask+fin+consumed). Cero hilos, cero bloqueo. Tungstenite eliminado del runtime (dep removida, `sha1` crate añadida). `WsServer(listen/accept/close)` + `WebSocket.connect` async + `AsyncReader/AsyncWriter` + `listen()` + fragmentación + ping/pong/close automáticos. Verificado full-duplex propio (texto, binario 300B, close).
- Límites honestos: `wss://` sin soporte (sin TLS); `close()` best-effort sin frame; paths no-ascii: semántica char; parser tolera tramas maskeadas en ambas direcciones (simplifica; RFC estricto las rechazaría del servidor).
- Suite 4 cuadrantes verde 2002/0.

## 9. Criterio de aceptación por paso

- Checklist Leyes 1-11 + checklist fase (sección 4 AGENTS.md).
- `.\scripts\verify.ps1 -Fast`: fmt + clippy + audit + release + 4 cuadrantes `PASSED: N`, `FAILED: 0`, `ALL TESTS PASSED`.
- `VARN_CACHE_DIR=<temp>` limpio para aislar Ley2 si fallo no reproduce.
- Sin comentarios código, sin tests unitarios nuevos. Solo `tests/main.vn`.
- Un commit por paso, mensaje atómico, sin `git` automático (lo hace usuario).

## 9. Paso 9 — Decoradores completos (2026-09-30, COMPLETO)

Auditoría de `tests/27-decorators.vn`: static/async/factories ya funcionaban (verificado por probes). Implementado lo faltante:
- Decoradores en funciones libres (antes silent-drop): statements `f = deco(f) ?? f` en posición de declaración + fallback ByName (`decorated_fns` en MCtx/ModuleCtx) para que TODAS las llamadas (incluida recursión) vean el valor decorado. Verificado: orden innermost-first, null conserva original, export, recursión (factd 120).
- Bug encontrado y corregido: el `Call` aparecía 2× (cond + else del Select) ejecutando el decorador dos veces → hoist a temp reutilizando `hoist` (hecho `pub`, cero duplicación).
- Miembros de namespace decorados: valor envuelto en el objeto literal (misma semántica).
- Rechazos parser (antes silent-drop): `@` en getters/setters/ctor/dtor/static-blocks/properties → error claro.
- `this`/`super` en método decorado (antes crash `OpGetFixedField ... null`): error `VN2013 invalid-decorator-target` con mensaje, vía walker exhaustivo sin wildcards (`checker/decorator_receiver.rs`: respeta scopes — desciende en arrows, no en `function`/`class`; incluye defaults de params y patrones).
- Límite honesto: bare calls a miembros ns decorados usan DirectFn crudo (sin global al cual rutear); pre-existente, documentado.
- Suite 4 cuadrantes verde 2002/0. fmt/clippy limpio en archivos tocados.

## 10. Tareas futuras registradas (no descartadas, una por una)

COMPLETADAS 2026-09-30 (matriz 4/4 verde 2002/0):
- T-fn-free: decoradores en funciones libres (statements + ByName + wrap en objetos ns; bug doble-ejecución corregido con hoist).
- T-heritage: herencia cross-módulo en `is_subclass_or_same` (walk local+core+scan stdlib con guarda ciclos); `toStdError` retorna `StdError` honesto.
- T-static-this: `VN4115 ThisOutsideInstance` (nuevo código en catálogo).
- T-spread: spread en MethodCall vía GetProperty+CallSpread (con `this` intacto).
- T-dispatch: `inst["metodo"]()` vía `get_index` → `get_property` (JIT+interp, un solo punto).
- T-scaffold-verify: `demo` end-to-end (run real cazó bug de inserción por pares, corregido con `insert_mod_pair`).
- T-redis-hang: era probe (tasks in-process run-to-completion sin interleave; concurrencia = procesos o `parallel`), no runtime. Verificado cliente full-duplex en 2 procesos.
- T-sysclock: `weekday` en `msToParts$` (chrono); cron conserva `dowOf` propio e independiente.
- T-wssrv: `closeGracefully()` async (close sync sigue best-effort).
- T-taskmodel: tasks bare no se interlean (run-to-completion); documentado arriba.

PENDIENTES (diseño hecho):
- T-this: this-forwarding con convención receiver-como-arg0 estilo Python (VM+JIT+interp; hoy `VN2013` bloquea con mensaje).
- T-tls: TLS vía rustls dedicado sobre mio (NO tokio wholesale) → `https://` + `wss://`. Requiere alta de crate con red.
- T-fndeco-ns-bare: bare calls a miembros ns decorados usan DirectFn crudo (sin global; pre-existente, sin crash).
- T-superspread: spread en `SuperMethodCall` (sigue error; exótico).
- T-getters-idx: getters vía `inst["prop"]` retornan Null en vez de invocar (documentado).
- T-funcdeco-ui: `@route`/`@component` en funciones libres (decoradores en fns libres se ignoran silenciosamente a nivel parser/checker).

## 11. Paso 1 detalle (implementación en curso)

Alcance: `crates/varn-builtins/src/modules/runtime/fs/fs.rs`, `fs_runtime.vn`, `std/fs/ops.vn`, `std/fs/file.vn`. Sin tocar `std/fs/mod.vn` exports, sin tocar `std:io`, sin cambiar errores.

Nuevos host (bulk, 1 call):
- `readFileText$(path: str): str` → `fs::read_to_string`, check `fs.read`
- `writeFileText$(path: str, data: str): void` → `fs::write`, check `fs.write`
- `appendFileText$(path: str, data: str): void` → `OpenOptions append+create`, check `fs.write`
- `copyFile$(from: str, to: str): void` → `fs::copy`, check `read(from)+write(to)`
- `readLines$(path: str): str[]` → `read_to_string` + `lines()` + `alloc_str` por línea, 1 call
- `readFdAll$(fd: int): str` → `Read::read_to_end` + `String::from_utf8`, loop en Rust
- `readFdAllBytes$(fd: int): Bytes` → `read_to_end` + `alloc_buffer_from_bytes`, loop en Rust

Reescritura Varn:
- `ops.read/readText` → `readFileText$`
- `ops.write/writeText` → `writeFileText$`
- `ops.appendText` → `appendFileText$`
- `ops.copy` → `copyFile$`
- `ops.readLines` → `readLines$`
- `ops.readBytes/writeBytes` → ya bulk, conservar
- `File.readAll` → `readFdAll$`, `File.pipeTo` → `readFdAll$` + 1 `write`, `FileStream` sin cambio (streaming real)
- Aliases `read/readText`, `write/writeText` conservados como thin delegate por compat suite; lógica única en host, no duplicada en Varn.

No incluye (futuro): cambio a `Result`, fusión archivos, cambio `io`, naming, scaffold, fast path.
