# Varn Language — Módulos y Paquetes

> Fuentes: `tests/21-async.vn`, `tests/27-decorators.vn`, `tests/39-package-imports.vn`, `tests/42-stdlib-comprehensive-test.vn`, `tests/47-isolates-multithread.vn`, `tests/54-channels.vn`, `tests/68-module-locals.vn`, `tests/80-capabilities-security.vn`, `tests/82-std-csv.vn`, `tests/83-canonical-types-regex-uuid-time.vn`, `tests/84-stdlib-process-compress-cli-env.vn`, `tests/85-stdlib-sqlite-ws-archive.vn`, `tests/86-tagged-templates.vn`.

---

## 1. Declaraciones `import`

### Importación nombrada desde stdlib

```varn
import { sleep, TaskGroup, spawn, parallel } from "std:task"
import { CSV } from "std:csv"
import { JSON } from "std:json"
import { Regex } from "std:regex"
import { UUID } from "std:crypto"
import { DateTime, Duration, Instant, PlainDateTime } from "std:time"
import { Process } from "std:process"
import { Compress, Tar, Zip } from "std:compress"
import { CLI } from "std:cli"
import { Env } from "std:env"
import { Path } from "std:path"
import { Database } from "std:sqlite"
import { WebSocketClient } from "std:ws"
import { read, write, remove, exists } from "std:fs"
import { env, platform, cwd } from "std:sys"
import { MetaKey, MethodContext, ClassRef } from "std:reflect"
import { describe, test, expect, assertEqual } from "std:test"
```

### Importación desde módulo local

```varn
import { workerMain } from "./some-module"
```

---

## 2. Declaraciones `export`

```varn
export async function workerMain(rx: Receiver<ValueMsg>, tx: Sender<int>) {
    // ...
}
// tests/47-isolates-multithread.vn:8
```

Solo las funciones exportadas (`export`) de nivel superior pueden usarse con `spawnIsolate`.

---

## 3. Módulos Locales y Variables Globales del Módulo

Las variables declaradas en el nivel superior del módulo son accesibles desde todos los ámbitos del archivo, incluyendo dentro de generadores y funciones:

```varn
let moduleCounter = 0
function bumpCounter(): int {
    moduleCounter = moduleCounter + 1
    return moduleCounter
}
function* touchesModuleState() {
    yield bumpCounter()    // accede a moduleCounter del módulo
    yield bumpCounter()
}
// tests/76-generator-values.vn:29
```

---

## 4. Paquetes de la Stdlib

### `std:task` — Concurrencia y Async

| Exportación | Descripción |
|------------|-------------|
| `sleep(ms)` | Pausa la ejecución por `ms` milisegundos. |
| `spawn(task)` | Ejecuta una tarea concurrente; devuelve handle awaitable. |
| `parallel(tasks[])` | Ejecuta todas las tareas en paralelo; devuelve resultados en orden. |
| `TaskGroup<T>` | Grupo de tareas con join/cancel/dispose. |
| `channel<T>(capacity)` | Crea un canal tipado con `{ tx: Sender<T>, rx: Receiver<T> }`. |
| `Sender<T>` | Extremo emisor de un canal. |
| `Receiver<T>` | Extremo receptor de un canal. |
| `ChannelClosed` | Error lanzado al usar un canal cerrado. |
| `spawnIsolate(fn, args)` | Crea un isolate separado ejecutando la función dada. |

### `std:test` — Testing

| Exportación | Descripción |
|------------|-------------|
| `describe(label, fn)` | Agrupa tests relacionados. |
| `test(label, fn)` | Define un test individual. |
| `expect(value).toBe(expected)` | Aserción de igualdad. |
| `assertEqual(a, b)` | Aserción de igualdad estricta. |
| `assert(label, condition)` | Aserción genérica. |

### `std:csv` — CSV

```varn
import { CSV } from "std:csv"
let users = CSV.parse(csvString)                        // con header
let matrix = CSV.parse(csvMatrix, { hasHeader: false }) // sin header
let products = CSV.parse(csvSemi, { delimiter: ";" })   // delimitador custom
let serialized = CSV.stringify(users)                   // serialize a CSV
// tests/82-std-csv.vn
```

### `std:json` — JSON

```varn
import { JSON } from "std:json"
let obj = JSON.parse("{\"message\": \"hello\", \"status\": 200}")
let str = JSON.stringify(obj)
// tests/82-std-csv.vn:80
```

### `std:regex` — Expresiones Regulares

```varn
import { Regex } from "std:regex"
let re = Regex.compile("^[a-zA-Z]+$")
re.test("hello")          // bool
re.exec("contact@g.com") // { match, index, groups }
re.findAll("1024 42 99")  // array de matches
re.replace("foo bar", " ") // str
re.split("a,b; c   d")    // str[]
// tests/83-canonical-types-regex-uuid-time.vn
```

### `std:crypto` — UUID

```varn
import { UUID } from "std:crypto"
let id = UUID.v4()    // UUID v4 aleatorio
let id7 = UUID.v7()   // UUID v7 (time-ordered)
UUID.isValid(id)      // bool
// tests/83-canonical-types-regex-uuid-time.vn:59
```

### `std:time` — Fecha y Hora

```varn
import { DateTime, Duration, Instant, PlainDateTime } from "std:time"
let now = DateTime.now()
let iso = now.toISOString()     // "2026-08-22T14:30:00Z"
let dt2 = now.addSeconds(10)
let diff = dt2.diff(now)        // 10000 (ms)

let dur = new Duration(5000)
dur.totalMilliseconds           // 5000
dur.seconds                     // 5

let pdt = new PlainDateTime(2026, 8, 22, 14, 30, 0, 0)
pdt.year   // 2026
pdt.month  // 8 (agosto; índice 1-based)
pdt.day    // 22
// tests/83-canonical-types-regex-uuid-time.vn:75
```

### `std:process` — Procesos

```varn
import { Process } from "std:process"
let result = Process.exec("echo VarnProcessTest")
result.exitCode   // int
result.success    // bool
result.stdout     // str
// tests/84-stdlib-process-compress-cli-env.vn:11
```

### `std:compress` — Compresión

```varn
import { Compress, Tar, Zip } from "std:compress"
let gz = Compress.gzip(text)    // bytes comprimidos
let original = Compress.gunzip(gz)
let def = Compress.deflate(text)
let inflated = Compress.inflate(def)

Tar.create(srcDir, tarPath)     // crea .tar
Tar.extract(tarPath, destDir)   // extrae .tar
Zip.create(srcDir, zipPath)     // crea .zip
Zip.extract(zipPath, destDir)   // extrae .zip
// tests/84-stdlib-process-compress-cli-env.vn / tests/85-stdlib-sqlite-ws-archive.vn
```

### `std:cli` — CLI Parser

```varn
import { CLI } from "std:cli"
let parsed = CLI.parse(["--port=9000", "--host=localhost", "-d", "serve", "api/v1"])
parsed.flags["port"]         // "9000"
parsed.flags["host"]         // "localhost"
parsed.flags["d"]            // "true"
parsed.positionals[0]        // "serve"
parsed.positionals[1]        // "api/v1"
// tests/84-stdlib-process-compress-cli-env.vn:36
```

### `std:env` — Variables de Entorno

```varn
import { Env } from "std:env"
Env.set("VARN_TEST_PORT", "8080")
Env.get("VARN_TEST_PORT")              // "8080"
Env.getInt("VARN_TEST_PORT", 3000)    // 8080 (int)
Env.getBool("VARN_DEBUG_FLAG", false) // bool
Env.parse(".env content")             // Map<str, str>
// tests/84-stdlib-process-compress-cli-env.vn:48
```

### `std:path` — Rutas de Archivo

```varn
import { Path } from "std:path"
Path.join("src", "modules", "app.vn")  // "src/modules/app.vn" (o \\ en Windows)
Path.extname("archive.tar.gz")          // ".gz"
Path.extname("index.html")              // ".html"
// tests/84-stdlib-process-compress-cli-env.vn:64
```

### `std:sqlite` — Base de Datos SQLite

```varn
import { Database } from "std:sqlite"
let db = new Database(":memory:")
db.exec("CREATE TABLE heroes (...)")   // DDL / DML; devuelve rowsAffected: int
db.all("SELECT * FROM heroes")         // array de objetos
db.get("SELECT ... WHERE id = ?", 1)  // un objeto o null

// Tagged template SQL (parametrizado y seguro)
let rows = db.sql`SELECT * FROM users WHERE role = ${targetRole}`
let one  = db.sqlOne`SELECT * FROM users WHERE id = ${targetId}`
db.close()
// tests/85-stdlib-sqlite-ws-archive.vn / tests/86-tagged-templates.vn
```

### `std:ws` — WebSocket

```varn
import { WebSocketClient } from "std:ws"
assert("ws class shape", typeof WebSocketClient === "class")
// tests/85-stdlib-sqlite-ws-archive.vn:92
```

### `std:fs` — Sistema de Archivos

```varn
import { read, write, remove, exists, mkdir, removeAll } from "std:fs"
let content = read("Cargo.toml")         // str
write("output.txt", "contenido")
let ok = exists("Cargo.toml")            // bool
remove("temp.tmp")
mkdir("./tmp_dir")
removeAll("./tmp_dir")
// tests/80-capabilities-security.vn / tests/85-stdlib-sqlite-ws-archive.vn
```

### `std:sys` — Sistema

```varn
import { env, platform, cwd } from "std:sys"
let p = platform()    // str: "windows", "linux", etc.
let d = cwd()         // str: directorio de trabajo actual
let v = env("OS")     // str: valor de la variable de entorno "OS"
// tests/80-capabilities-security.vn:6
```

### `std:reflect` — Metaprogramación

```varn
import { MetaKey, MethodContext, ClassRef } from "std:reflect"
const RouteKey = MetaKey.create<str>()
RouteKey.set(MyClass, "/api/items")
RouteKey.get(MyClass)    // "/api/items" o null
// tests/27-decorators.vn
```
