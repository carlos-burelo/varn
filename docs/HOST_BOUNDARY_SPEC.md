# Especificación del Boundary Host en Varn

Cómo Varn conecta su stdlib pública con el host sin contaminar la API del lenguaje.

## Principios

1. **Fuente única por operación**: ninguna op host se escribe en múltiples archivos.
2. **Modularización por dominio**: `crypto`, `fs`, `net`, `time`, etc. son unidades aisladas.
3. **IDs wire estables**: FNV1a de `"module_id::symbol_name"` — gaps no rompen el dispatch.
4. **Sin `__*` en el código fuente**: el bridge host no contamina la stdlib ni la API pública.
5. **Sin stubs vacíos**: toda API de stdlib tiene implementación ejecutable real.

## Modelo actual

### 1. Declaración en Rust

```rust
#[varn_module("std:crypto")]
pub mod crypto_impl {
    #[varn_fn("sha256", cap = "crypto.random")]
    pub fn sha256(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        let input = /* extraer str de args[0] */;
        Ok(/* VmValue con el digest */)
    }

    #[varn_fn("randomBytes", cap = "crypto.random")]
    pub fn random_bytes(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        // ...
    }
}
```

La macro `#[varn_module]` genera `NativeOpEntry` en secciones del linker. Al arrancar, `build_module("std:crypto", ctx)` ensambla el objeto Varn completo.

### 2. API pública en Varn (archivo `.vn`)

```Varn
// crates/varn-builtins/src/modules/std/crypto/crypto.vn
export namespace crypto {
    export type HexDigest = str

    export function sha256(input: str): HexDigest
    export function randomBytes(count: int): str
    export function uuid(): str
    export function base64Encode(data: str): str
    export function base64Decode(data: str): str
}
```

### 3. Uso desde código Varn

```Varn
import { crypto } from "std:crypto"

const digest = crypto.sha256("hola mundo")
print(digest)
```

## Reglas de diseño por constructo

| Constructo | Uso correcto |
|-----------|-------------|
| `function` | Operaciones host-backed: I/O, hashing, parsing, utilidades |
| `class` | Handles con lifecycle: `File`, `Socket`, `Connection` |
| `namespace` | Agrupa un dominio: `crypto`, `fs`, `net` |
| `extension` | Solo ergonomía, nunca abre ruta host nueva |
| `interface` | Contratos: `Disposable`, `Readable`, `Hasher` |
| `using` | Lifecycle de recursos: `using file = await fs.open(...)` |
| `async` | Operaciones diferidas reales con `Task<T>` |

## Qué no debe existir

- Enum global monolítico de host ops.
- Dispatch por índice posicional (frágil ante gaps).
- Resolución textual en caliente.
- `__*` en la API pública del lenguaje.
- Módulos con API declarada sin implementación.

## Capabilities

Cada op declara la capability que requiere:

```rust
#[varn_fn("readFile", cap = "fs.read")]
```

| Capability | Descripción |
|-----------|-------------|
| `fs.read` | Leer archivos |
| `fs.write` | Escribir archivos |
| `net.client` | Conexiones salientes |
| `net.server` | Escuchar puertos |
| `sys.env` | Variables de entorno |
| `sys.proc` | Procesos del sistema |
| `sys.ffi` | FFI nativo |
| `crypto.random` | Generación de aleatoriedad |
| `time.now` | Leer tiempo del sistema |

> **Estado actual**: `has_capability()` retorna `true` siempre. Enforcement real está pendiente.

## Dispatch

```
OpHostCall(wire_id, argc)
    │
    ▼
dispatch_host_op(id, ctx, args)
    │
    ├─ Lookup en HashMap<u64, DispatchEntry>  (O(1))
    ├─ Verificar capability
    └─ Llamar entry.func(ctx, args)
```

Ver detalles de implementación en [LBI_ARCHITECTURE.md](LBI_ARCHITECTURE.md).

## Versionado: `HOST_API_VERSION`

`varn_core::HOST_API_VERSION: u32` versiona la superficie `runtime:*` como
unidad — es lo que hace posible distribuir `std.vnb` desacoplado del binario
`vn` (ver [STDLIB_ARCHITECTURE.md](STDLIB_ARCHITECTURE.md)).

- **Breaking change** (firma de un native cambia, símbolo se elimina/renombra)
  → **bump obligatorio** de `HOST_API_VERSION`. `StdBundle::validate_compat_with`
  rechaza en carga cualquier bundle cuyo `host_api_version` no calce
  exactamente con el del binario — error inmediato, sin fallback silencioso.
- **Cambio aditivo** (símbolo nuevo, sin tocar los existentes) → **no** bump.
  Un bundle que use el símbolo nuevo sobre un binario viejo no lo detecta el
  check de versión; falla al resolver ese import puntual con un error de
  módulo claro (`runtime:x tiene un import no resuelto`). El check de versión
  cubre breaking changes; la resolución de imports cubre los aditivos.
