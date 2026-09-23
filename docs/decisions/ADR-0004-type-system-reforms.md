# ADR 0004: Reforma del Sistema de Tipos — Nominalidad Estricta de Clases, Prohibición de Narrowing Implícito y Consolidación de ADTs

## Estado
Aprobado e Implementado.
Sección 2 ("Prohibición de Narrowing Implícito en Numéricos") reemplazada por ADR-0015: los anchos angostos dejan de existir.

## Contexto
Durante la auditoría integral del sistema de tipos de Varn, se detectaron rezagos de diseño de tipado dinámico/estructural heredados de TypeScript que entraban en conflicto con la directiva primaria de Varn como lenguaje estáticamente tipado:
1. **Clases Estructurales:** Dos clases independientes con los mismos campos (`Point` y `Vector`) eran consideradas compatibles por el checker mediante comparación estructural de miembros. Esto violaba la encapsulación nominal e impedía optimizaciones seguras de devirtualización y layout fijo en el compilador/JIT.
2. **Narrowing Numérico Implícito:** Variables de 64 bits (`int` y `float`) podían asignarse silenciosamente a tipos granulares más estrechos (`i8`, `i16`, `i32`, `u8`, `u16`, `u32`, `f32`), provocando potencial truncamiento y violando la Regla 4 de tipado estático estricto.
3. **Duplicación de Tipos Suma:** El AST y el parser mantenían `Decl::SumType`, una construcción a medio implementar y redundante con los `Enum` con payload (ADTs canónicos de Varn con pattern matching exhaustivo).

## Decisiones

### 1. Nominalidad Estricta de Clases
- Las clases son nominales: una clase declarada `C` solo acepta instancias de `C` o de una subclase de `C` (`is_subclass_or_same`).
- Dos clases no emparentadas nunca son compatibles entre sí, independientemente de si sus miembros coinciden.
- El polimorfismo estructural se reserva exclusivamente para `interface` y `Record`.

### 2. Prohibición de Narrowing Implícito en Numéricos
- Las variables de tipos enteros amplios (`int`) o flotantes (`float`) no pueden asignarse implícitamente a tipos granulares (`i8`, `i16`, `i32`, `u8`, `u16`, `u32`, `f32`). Se requiere un cast explícito (`as i8`, etc.).
- Los literales numéricos constantes (`42`, `3.14`) son permitidos en inicializaciones y llamadas únicamente si caben dentro del rango representable del tipo destino (`literal_fits_type`).
- El widening seguro (`i8 -> int`, `i32 -> int`, `int -> float`, `f32 -> float`) se preserva.

### 3. Consolidación de ADTs y Purga de `Decl::SumType`
- Se eliminó `Decl::SumType` del AST, parser, binder, LSP y debug tools.
- La única fuente canónica de tipos algebraicos de datos (ADTs / tagged unions) en Varn es `Enum` con payload.

## Consecuencias
- **Seguridad de Tipos:** Se erradica la posibilidad de desbordamiento silencioso en asignaciones entre variables enteras.
- **Optimización:** El JIT y el SSA pueden asumir con certeza la identidad de tipo de las clases sin falsas equivalencias estructurales.
- **Simplificación del Código:** Se eliminaron ~200 líneas de código muerto y redundante a lo largo de 9 módulos del compilador.
