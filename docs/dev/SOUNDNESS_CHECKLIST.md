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
- [x] `Json.parse<T>` sobre interfaces locales, genéricas e importadas:
  el checker resuelve origen, sustitución y herencia en un catálogo único;
  el emitter conserva `Interface(ShapeId)` sin re-resolver por nombre.
  Sondas de `Box<int>`, imports renombrados, miembros anidados, herencia y
  recursión rechazan tipos incorrectos; `Box<str>` y `Envelope<int>` válidos pasan.
- [x] Records conservan identidad estática propia (`#{ campo: Tipo }`) desde AST
  hasta el contrato runtime, incluidos alias genéricos/importados y recursión.
  Las formas llevan `readonly`; no se acepta un objeto como record ni un record
  como vista mutable. Se eliminan los borrados de forma a 6/32 niveles.
  Conform valida sin mutar datos; decode construye contenedores nuevos. Se borran
  el normalizador mutante y el protocolo de reemplazo de argumentos. Tres suites
  existentes migran las conversiones implícitas a decoders tipados explícitos.
- [x] `Json.parse<T>` sobre `class`: decisión: no decodificable; falla con TypeError en runtime (una clase puede ser un retorno legítimo de otras funciones genéricas, por eso no es error de compilación)
- [x] `as T` exige prueba estática dentro de cuerpos genéricos; la conversión implícita
  de `dynamic` a un parámetro abierto también se rechaza. Solo el retorno directo de
  un decoder síncrono con parámetros elegidos por el llamador se difiere al contrato
  concreto del sitio de llamada. Signals usa `Option<T>` para distinguir ausencia
  de un `T` nulo; UI usa handles `State<T>` por owner y elimina slots heterogéneos;
  `MetaKey<T>` conserva almacenamiento tipado. Colas, ejemplos y consumidores migrados.
  Se eliminaron 15 casts a tipos abiertos de std y consumidores; los tres ejemplos
  declarativos y las dos suites que dependían de slots implícitos usan 14 handles
  tipados. Se migró además el memoizador `once` de la suite de orden superior.
- [x] Intérprete y JIT comparten el límite lógico de 10000 llamadas; las llamadas
  JIT sin frame participan en el contador. La guarda de pila nativa sigue siendo
  un límite físico adicional. El contador se restaura también al suspender;
  desbordamientos consecutivos se capturan y permiten continuar con recursión acotada.
  La ruta de self-call con frame rechaza antes de reservarlo: se elimina el frame
  sobrante que hacía repetir el bucle del llamador tras capturar el error.
- [x] VN3031 entre módulos (regla por tipo; descartar a propósito: `_ = llamada()`)
- [x] `Task` (perezosa) vs `TaskHandle` (iniciada): contratos nativos retipados
- [x] Tareas lanzadas y nunca ejecutadas se descartan al salir: decisión (como Go/tokio); los rechazos de las que corrieron sí se informan

### Semántica
- [x] `indexOf`/`includes` usan la igualdad del lenguaje (`NaN` no se encuentra; tuplas y records por valor)
- [x] Igualdad: decisión autorizada (2026-10-26): `==` / `!=` rechazan arrays,
  objetos, clases, funciones, colecciones mutables, `dynamic` y tipos abiertos
  (VN3010). Se conservan comprobaciones de nulidad y tipos por valor. `sameValue`
  ofrece comparación explícita; los métodos `equals` se invocan explícitamente.
  Once suites y cinco consumidores de std migran sus comparaciones; JWT estrecha
  el tipo de su algoritmo antes de compararlo con texto.
- [x] Sombreado de predefinidos: `Infinity`/`NaN` y tipos primitivos no se redeclaran (módulos core exentos)
- [x] `x instanceof int` es el test de tipo para uniones (la std lo usa y estrecha): decisión, sin cambio
- [ ] `dynamic` sin autorización explícita (Ley 17): inventariar usos
- [x] Presentación conserva `.0` en floats enteros (incluido `-0.0`); strings
  y chars anidados llevan delimitadores. JSON y presentación comparten escapes
  de cadenas, incluidos todos los controles U+0000..U+001F. Los seis consumidores
  de la suite que fijaban el formato anterior se migraron.
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
