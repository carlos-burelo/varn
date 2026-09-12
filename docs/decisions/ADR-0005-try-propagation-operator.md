# ADR 0005: Operador Prefijo de Propagación de Errores (`try <expr>`)

## Estado
Aprobado e Implementado.

## Contexto
En Varn, el manejo canónico de errores se basa en tipos de datos algebraicos (`Result<T, E>`), opcionales (`Option<T>`) y tipos anulables (`T?`), evitando el costo de las excepciones (`throw`/`try-catch`) para errores esperables.

Originalmente se contempló usar el símbolo posfijo `?` para propagación (estilo Rust). Sin embargo, el análisis de diseño demostró que el carácter `?` en Varn ya desempeña múltiples roles esenciales:
1. **Tipos anulables (`T?`)**: Anotación de tipos.
2. **Operador ternario (`cond ? a : b`)**: Expresión condicional.
3. **Encadenamiento opcional (`obj?.prop`, `arr?.[i]`)**: Navegación segura.
4. **Null coalescing (`??`)**: Provisión de valores por defecto.

Añadir `?` como operador de sufijo (`parse()?`) generaba ambigüedades sintácticas complejas con el ternario en expresiones anidadas y sobrecargaba la semántica del signo `?`.

## Decisiones

### 1. Sintaxis Prefija con Palabra Clave `try <expr>`
- Se adopta la sintaxis prefija `try <expr>` (inspirada en Zig, Swift y C# propuestas de modernización).
- No existe colisión sintáctica con `try { ... } catch { ... }`: el parser desambigua mediante `s.peek_kind(1) == TokenKind::LBrace` a nivel de sentencias, permitiendo `try <expr>` como expresión unaria.

### 2. Soporte Polimórfico en el Checker y Emisor TIR
El operador `try <expr>` soporta de forma canónica tres familias de tipos:
- **`Result<T, E>`**: Si el valor es `Ok(val)`, desenvuelve `val: T`. Si es `Err(err)`, efectúa un retorno anticipado (`return Result.Err(err)` o `return hoisted`) en la función contenedora. La función contenedora debe retornar un `Result`.
- **`Option<T>`**: Si el valor es `Some(val)`, desenvuelve `val: T`. Si es `None`, efectúa un retorno anticipado (`return Option.None`). La función contenedora debe retornar un `Option`.
- **Anulable (`T?`)**: Si el valor no es `null`, desenvuelve el valor a tipo no anulable `T`. Si es `null`, retorna `null` anticipadamente. La función contenedora debe tener tipo de retorno anulable.

### 3. Emisión en TIR
- Se evalúa la expresión interna una sola vez y se ancla mediante `hoist` a un local temporal.
- Para enums (`Result`, `Option`), se extrae el discriminante mediante `TirExprKind::Discriminant` y se compara con el tag de la variante de error.
- En caso de coincidencia con la variante de error, se emite un `TirStmt::If` con retorno anticipado (`TirStmt::Return`).
- La expresión resultante se transforma en `TirExprKind::VariantPayload` para extraer el valor con desempaquetado de costo cero en tiempo de compilación.
- Para tipos anulables, se emite `TirUnOp::IsNull` con retorno anticipado de `null` y desempaquetado vía `TirExprKind::Cast`.

### 4. Corrección de Invariantes del JIT y Binder
- Se corrigió `crates/varn-checker/src/binder/decl_values/declarations.rs` para registrar todas las variantes de enum en `sum_type_variants`.
- Se corrigió `crates/varn-jit/src/clif/kinds.rs` para clasificar `OpCode::GetEnumTag` como `K::Int` (en lugar de `boxed`), garantizando que la comparación de tags en el backend JIT use instrucciones nativas directas (`icmp`).
- Se corrigió `crates/varn-checker/src/checker_expressions/check/exhaustiveness.rs` para reconocer `MatchPattern::EnumVariant` en el análisis de exhaustividad.

## Consecuencias
- **Cero Ambigüedad Sintáctica**: El operador ternario, encadenamiento opcional y null coalescing permanecen limpios y sin ambigüedades.
- **Ergonomía de Manejo de Errores**: Se elimina el código repetitivo de `match` para propagar errores en cascada.
- **Rendimiento**: Ejecuta a nivel de bytecode y JIT con operaciones nativas de discriminante y desempaquetado directo de campo.
