# LBI Architecture — Linker-Bound Interface

El sistema **LBI** permite registrar funciones nativas de Rust sin tabla centralizada manual. Las ops se "autodeclaran" y el motor las descubre al arrancar.

## El problema que resuelve

Sin LBI, cada función nativa requiere añadirse a un mapa global. Esto genera acoplamiento circular, mantenimiento pesado, y conflictos de merge. LBI elimina ese registro centralizado.

---

## Cómo funciona

### 1. Declaración con macros

```rust
#[varn_module("std:time")]
pub mod time_module {
    #[varn_fn("now", cap = "time.now")]
    pub fn time_now(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> NativeFnResult {
        // implementación
    }

    #[varn_class("Instant")]
    pub mod instant {
        #[varn_constructor]
        pub fn new(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> Result<(), String> {
            // ...
        }

        #[varn_getter("epochMilliseconds")]
        pub fn epoch_ms(ctx: &mut dyn NativeCtx, this: VmValue) -> NativeFnResult {
            // ...
        }

        #[varn_static("now")]
        pub fn now(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            // ...
        }
    }
}
```

### 2. Secciones del linker

La macro `#[varn_module]` genera `NativeOpEntry` estáticos en secciones específicas del binario:

| OS      | Sección                  |
|---------|--------------------------|
| Windows | `.varn_ops$B`            |
| macOS   | `__DATA,varn_ops`        |
| Linux   | `varn_ops`               |

Cada entry contiene: `module_id`, `namespace_path`, `symbol_name`, `func_ptr`, `capability_mask`, `entry_kind`.

### 3. Descubrimiento en startup

```rust
fn iter_native_ops() -> impl Iterator<Item = &'static NativeOpEntry> {
    // Escanea __varn_OPS_START .. __varn_OPS_END
    // Filtra entries con func_ptr nulo (marcadores de inicio/fin)
}
```

Al arrancar, `iter_native_ops()` itera la sección de memoria y construye el dispatch table en un `HashMap<u64, DispatchEntry>` con clave FNV1a de `"module_id::symbol_name"`.

---

## Construcción de módulos

`build_module(id, ctx)` ensambla el objeto Varn para un módulo dado:

1. Itera todas las entries con `module_id == id`.
2. `entry_kind == 0x01` (Function) → `ctx.alloc_fn(...)` y `ctx.set_field(target, symbol, val)`.
3. `entry_kind == 0x10` (ClassDef) → llama al builder fn que retorna `Rc<ClassObj>` como `VmValue`.
4. `entry_kind == 0x09` (StaticValue) → `ctx.call_static(fn)`.
5. Entries de miembros de clase (0x11–0x15) son procesadas por el ClassDef builder — se saltan aquí.
6. `namespace_path` no vacío → crea objetos intermedios anidados automáticamente.

---

## Entry Kinds

| Kind | Valor | Descripción |
|------|-------|-------------|
| `Function` | 0x01 | Función libre del módulo |
| `ClassConstructor` | 0x02 | Constructor (legacy) |
| `InstanceMethod` | 0x03 | Método de instancia |
| `StaticMethod` | 0x04 | Método estático |
| `Getter` | 0x05 | Getter de instancia |
| `Setter` | 0x06 | Setter de instancia |
| `PrimitiveExt` | 0x07 | Extension sobre tipo primitivo |
| `EnumVariant` | 0x08 | Variante de enum |
| `StaticValue` | 0x09 | Valor constante (getter fn sin args) |
| `ClassDef` | 0x10 | Builder de clase completa |
| `Constructor` | 0x11 | Constructor granular |
| `InstanceGetter` | 0x12 | Getter granular |
| `InstanceSetter` | 0x13 | Setter granular |
| `StaticGetter` | 0x14 | Getter estático |
| `StaticSetter` | 0x15 | Setter estático |
| `ExtMethod` | 0x16 | Extension method |
| `ExtGetter` | 0x17 | Extension getter |
| `EnumDef` | 0x18 | Definición de enum |
| `ConstValue` | 0x19 | Constante |
| `AsyncFunction` | 0x1A | Función async libre |
| `NamespaceDef` | 0x1B | Declaración de namespace |

---

## Macros disponibles

| Macro | Uso |
|-------|-----|
| `#[varn_module("id")]` | Módulo contenedor. Genera todas las entries del módulo. |
| `#[varn_fn("name")]` | Función libre. |
| `#[varn_fn("name", cap = "cap.name")]` | Función con capability requerida. |
| `#[varn_class("Name")]` | Clase. Genera builder fn y ClassDef entry. |
| `#[varn_constructor]` | Constructor de clase. |
| `#[varn_method("name")]` | Método de instancia. |
| `#[varn_getter("name")]` | Getter de instancia. |
| `#[varn_static("name")]` | Método estático. |
| `#[varn_static_getter("name")]` | Getter estático. |
| `#[varn_namespace("name")]` | Namespace anidado. |
| `#[varn_extends("type")]` | Extension sobre tipo primitivo. |

---

## Capabilities

Cada op puede declarar una capability requerida:

```rust
#[varn_fn("readFile", cap = "fs.read")]
pub fn read_file(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    // solo ejecuta si ctx.has_capability("fs.read")
}
```

`dispatch_host_op()` verifica la capability antes de invocar la función. Error si no tiene permiso: `E_HOST_PERMISSION_DENIED:id=...:capability=fs.read`.

Capabilities disponibles: `fs.read`, `fs.write`, `net.client`, `net.server`, `sys.env`, `sys.proc`, `sys.ffi`, `crypto.random`, `time.now`.

---

## Dispatch

`dispatch_host_op(id, ctx, args)`:
1. Lookup por `id` (FNV1a de `"module::symbol"`) en el dispatch table.
2. Verifica capability si la entry la declara.
3. Llama `entry.func(ctx, args)`.

Lookup O(1) por HashMap. Sin strings en caliente, sin tablas posicionales frágiles.
