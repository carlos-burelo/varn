# Próximos Pasos

Ver [ROADMAP.md](ROADMAP.md) para la hoja de ruta completa.

## Inmediato

### `.vnc` zero-copy
El formato actual usa postcard (serialización O(n) allocations). Para load sub-milisegundo: mmap + flat binary (estilo rkyv o formato propio). El postcard actual tarda ~650µs en cargar 80KB — aceptable pero mejorable.

### Capability system enforcement
`NativeCtx::has_capability()` retorna `true` incondicionalmente. Las capabilities están declaradas en las macros (`cap = "fs.write"`) y el mask se almacena en `NativeOpEntry::capability_mask`, pero no se verifica en runtime. Conectar con un `CapabilitySet` real.

### Verificación IDL vs implementación
Cada módulo stdlib tiene un archivo `.vn` (interfaz) y una implementación Rust. Actualmente no hay verificación automática de que coincidan. Añadir check compile-time o test de superficie.

## Mediano plazo

### Optimizador (varn-ir)
`varn-ir` implementa SSA completo. Falta aplicar passes:
- Constant folding: `OpAdd(LoadConst(2), LoadConst(3))` → `LoadConst(5)`.
- Dead Code Elimination: bloques inalcanzables después de `return`.

### Match guards
`match x { n if n > 5 => ... }` — el parser/checker lo soportan parcialmente. Falta codegen completo del guard condicional con back-patching correcto.

### `vn publish`
Publicar paquetes al registro. Requiere definir el protocolo del registro primero.

## Largo plazo

Ver [ROADMAP.md](ROADMAP.md#largo-plazo).
