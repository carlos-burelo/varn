# Checklist de soundness semántica

Origen: auditoría de semántica por sondas `.vn` y lectura de código (2026-10-25). Cada ítem cerrado
lleva su commit atómico, su test en `tests/` o su fixture en `tests/errors/`.

## Cerrado

- [x] `Array.pop()` / `shift()` devuelven `T?`
- [x] `split("")` devuelve caracteres (una sola implementación en `str_util::split_ranges`)
- [x] Shifts fuera de 0..63 lanzan error (VM, plegado de constantes y JIT)
- [x] El spread con override reemplaza el miembro y su tipo
- [x] `Json.parse<T>` valida en el sitio de llamada (`decoded_calls`, `CheckKind::Decode`)
- [x] `Json.tryParse` sin parámetro de tipo
- [x] Campos de clase no nulos deben quedar asignados en cada ruta del constructor
- [x] Lectura de un campo antes de asignarlo / initializer que lee un campo posterior
- [x] Uso antes de la declaración (VN4003), directo y a través de llamadas
- [x] Estáticos de clase se inicializan después de métodos y binding de clase
- [x] Parámetros de función compatibles contravariantemente
- [x] Indexar por nombre una clase o record cerrado (VN3006); clave literal = acceso con `.`
- [x] Recursión infinita en el JIT lanza error capturable
- [x] Llamar a una función async y descartar el `Task` (VN3031, mismo módulo)
- [x] Rechazo de tarea no observado hace fallar el programa
- [x] `cancel()` rechaza con un `Error` real y cuenta como manejado
- [x] `Task.race` rechaza cuando la primera tarea falla
- [x] `print` / plantillas muestran estructura; ciclos protegidos; un solo texto de float
- [x] `for…in` sobre receptores sin claves (VN3029)
- [x] `a.length = n` y `s[i] = x` son errores de compilación
- [x] Literales regex rechazados en el parser
- [x] Operador coma solo en cabeceras de `for`
- [x] Signo antes de `**` exige paréntesis
- [x] `Json.stringify(NaN)` lanza error
- [x] `substring` / `repeat` validan argumentos
- [x] `let` en el cuerpo no redeclara un parámetro
- [x] `return` dentro de `finally` (VN4023)
- [x] Cast entre contenedores valida los elementos
- [x] `indexOf` / `lastIndexOf` / `findIndex` devuelven `int?`
- [x] `std:sys.now()` nunca lee 0

## Pendiente

### Soundness
- [x] Recursión nativa en Rust: stringify acotado, igualdad iterativa (GC, drop, hash y marshalling probados sin fallo)
- [ ] `Json.parse<T>` sobre `interface` (hoy no se valida)
- [ ] `Json.parse<T>` sobre `class`: diagnóstico de compilación en lugar de error de runtime
- [ ] `as T` con `T` genérico sin comprobación (`signals/reactive`, `ui/bound`, `reflect/meta`)
- [ ] Paridad de límites de recursión entre intérprete (10000 frames) y JIT (bytes de pila)
- [ ] VN3031 entre módulos (los ids de símbolo no cruzan módulos)
- [ ] `Task` vs `TaskHandle`: separar tarea perezosa de operación nativa ya iniciada
- [ ] Tareas lanzadas y nunca esperadas se descartan al salir sin ejecutarse

### Semántica
- [ ] Igualdad: `[1]==[1]` (identidad) vs `#{}` (valor); `NaN` en `indexOf` vs `==`
- [ ] Sombreado de predefinidos (`const Infinity = 5`, `class int {}`)
- [ ] `5 instanceof int` es `true`; `dynamic` sin autorización explícita
- [ ] `print(1.0)` muestra `1`; strings sin comillas dentro de contenedores
- [ ] `max()` sin argumentos: ruta de la intrínseca vs la de la std
- [ ] `Json.parse` descarta campos desconocidos en silencio

### Limpieza
- [ ] Eliminar `ExprKind::RegexLiteral` del AST y de sus `match`
- [ ] Rango del error "let redeclara un parámetro" (hoy todo el cuerpo)
- [ ] `clippy::wildcard_enum_match_arm` en código nuevo
- [ ] `Array.shift` es O(n)

### Sin auditar
- [ ] Concurrencia entre isolates y data races
- [ ] FFI
- [ ] Imports circulares y orden de inicialización entre módulos
- [ ] Normalización Unicode
- [ ] `std:time`, `std:ui`
