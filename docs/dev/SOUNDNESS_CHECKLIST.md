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
- [x] `Json.parse<T>` sobre interfaces locales sin parámetros (`DynReason::Interface` lleva su forma)
- [ ] Completar la forma de interfaces genéricas e importadas.
  En `varn-emit/src/ty/lower.rs::lower_kind`, `Generic(name, _, origin)` ignora
  los argumentos y solo resuelve clases/enums; `Named` con origen externo sin
  clase/enum cae en `interface()` (`Declared`). `NameIndex` solo carga propiedades
  de `bind.type_members.interfaces`, indexadas por nombre.
  Repros mínimos pendientes de incorporar a la suite:
  `interface Box<T> { value: T; }` con `Json.parse<Box<int>>("{\"value\":\"x\"}")`;
  y `Json.parse<Box>("{\"value\":\"x\"}")` importando
  `export interface Box { value: int; }` de otro módulo.
  Deben rechazar el campo `str`. Pérdida verificada por lectura del lowering;
  estos repros no se han ejecutado.
- [x] `Json.parse<T>` sobre `class`: decisión: no decodificable; falla con TypeError en runtime (una clase puede ser un retorno legítimo de otras funciones genéricas, por eso no es error de compilación)
- [ ] `as T` con `T` genérico sin comprobación (`signals/reactive`, `ui/bound`, `reflect/meta`)
- [ ] Paridad de límites de recursión entre intérprete (10000 frames) y JIT (bytes de pila)
- [x] VN3031 entre módulos (regla por tipo; descartar a propósito: `_ = llamada()`)
- [x] `Task` (perezosa) vs `TaskHandle` (iniciada): contratos nativos retipados
- [x] Tareas lanzadas y nunca ejecutadas se descartan al salir: decisión (como Go/tokio); los rechazos de las que corrieron sí se informan

### Semántica
- [x] `indexOf`/`includes` usan la igualdad del lenguaje (`NaN` no se encuentra; tuplas y records por valor)
- [ ] Igualdad: `[1]==[1]` (identidad) vs `#{}` (valor): decidir si `==` sobre arrays/objetos mutables debe ser error
- [x] Sombreado de predefinidos: `Infinity`/`NaN` y tipos primitivos no se redeclaran (módulos core exentos)
- [x] `x instanceof int` es el test de tipo para uniones (la std lo usa y estrecha): decisión, sin cambio
- [ ] `dynamic` sin autorización explícita (Ley 17): inventariar usos
- [ ] `print(1.0)` muestra `1`; strings sin comillas dentro de contenedores
- [x] `max()` / `min()` sin argumentos: error de compilación (primer argumento obligatorio)
- [ ] `Json.parse` descarta campos desconocidos en silencio

### Limpieza
- [x] Eliminar `ExprKind::RegexLiteral` del AST y de sus `match`
- [x] Rango del error "let redeclara un parámetro": señala el declarador que colisiona
- [x] `clippy::wildcard_enum_match_arm` obligatorio (`deny` en workspace); eliminados los siete avisos existentes
- [x] `Array.shift` es O(n): solo rendimiento, sin primitiva de remoción en la representación; aceptado (Queue usa dos pilas)

### Sin auditar
- [ ] Concurrencia entre isolates y data races
- [ ] FFI
- [ ] Imports circulares y orden de inicialización entre módulos
- [ ] Normalización Unicode
- [ ] `std:time`, `std:ui`
