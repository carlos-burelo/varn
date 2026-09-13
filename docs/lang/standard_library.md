# Varn Language — Biblioteca Estándar (stdlib)

> Documentación de referencia consolidada de toda la funcionalidad de la stdlib ejercitada por la suite de pruebas. Para casos de uso con ejemplos completos, ver los archivos de prueba referenciados.

---

## 1. Tipos Primitivos — Métodos Integrados

### `str` — Cadenas de Texto

> Fuente: `tests/03-strings.vn`

```varn
// Inspección
s.length          // int: número de caracteres
s.isEmpty()       // bool
s.isBlank()       // bool: solo espacios/whitespace
s.isDigit()       // bool: "7" → true, "a" → false
s.isLetter()      // bool: "z" → true, " " → false

// Búsqueda
s.includes(sub)         // bool
s.startsWith(prefix)    // bool
s.endsWith(suffix)      // bool
s.indexOf(sub)          // int (-1 si no encontrado)

// Transformación
s.trim()              // str: elimina espacios inicio y fin
s.trimStart()         // str
s.trimEnd()           // str
s.toUpperCase()       // str
s.toLowerCase()       // str
s.capitalize()        // str: primera letra mayúscula
s.reverse()           // str

// Extracción
s.slice(start, end)   // str: [start, end)
s.slice(-5)           // str: últimos 5 caracteres
s.substr(start)       // str
s.substr(start, len)  // str
s.at(i)               // str: carácter en índice i
s[i]                  // str: acceso por índice
s[0..5]               // str: slicing con rango exclusivo (test 88)
s[0..=4]              // str: slicing con rango inclusivo (test 88)

// Modificación
s.replace(old, new)       // str: primera ocurrencia
s.replaceAll(old, new)    // str: todas las ocurrencias
s.repeat(n)               // str
s.padStart(len, pad)      // str
s.padEnd(len, pad)        // str
s.concat(other)           // str

// Análisis
s.split(delim)         // str[]
s.lines()              // str[]: divide por \n
s.words()              // str[]: divide por espacios
s.charCode()           // int: code point del primer carácter

// Métodos de clase (estáticos)
str.from(value)           // str: convierte int/bool/etc a str
str.EMPTY                 // "": cadena vacía
str.fromCharCode(72, 105) // "Hi"
str.join(arr, sep)        // str: une array con separador
```

### `char` — Carácter Unicode

> Fuente: `tests/34-char-type.vn`

```varn
// Literales
const a: char = 'a'

// Creación
char.fromCode(65)       // 'A'

// Clasificación
c.charCodeAt()          // int: code point
c.isAlphabetic()        // bool
c.isAlphanumeric()      // bool
c.isDigit()             // bool
c.isWhitespace()        // bool
c.isUppercase()         // bool
c.isLowercase()         // bool
c.isAscii()             // bool
c.isPunctuation()       // bool

// Conversión
c.toUppercase()         // char
c.toLowercase()         // char
c.toString()            // str: "a"
```

### `int` — Entero

```varn
n.toString()          // str
n + 1.0               // → float (widening automático)
(5).isEven()          // extensión: bool (si definida)
(7).clamp(0, 10)      // extensión: int
```

### `float` — Flotante

```varn
f.toString()          // str
```

### `decimal` — Decimal de Precisión Exacta

> Fuente: `tests/35-decimal-bigint.vn`

```varn
d.toFixed(n)    // str: redondeo a n decimales
d.abs()         // decimal
d.negate()      // decimal
d.floor()       // decimal
d.ceil()        // decimal
d.round()       // decimal
d.trunc()       // decimal
d.isZero()      // bool
d.isPositive()  // bool
d.isNegative()  // bool
d.toString()    // str
```

### `bigint` — Entero de Precisión Arbitraria

> Fuente: `tests/35-decimal-bigint.vn`

```varn
bi.toString()   // str
bi.toStr()      // str (alias)
bi.toInt()      // int
bi.toFloat()    // float
```

### `Bytes` — Bloque Canónico de Bytes Crudos

> Fuente: `tests/112-canonical-bytes-and-streams.vn`

```varn
// Creación y fábrica
const b = Bytes.alloc(1024)             // Bytes: asigna 1024 bytes inicializados a cero
const fromStr = Bytes.fromString("Varn") // Bytes: a partir de texto UTF-8
const fromArr = Bytes.fromBytes([0, 255])// Bytes: a partir de array numérico int[]

// Indexación directa (0..255)
b[0] = 65                               // asignación por índice
const val = b[0]                        // int: 65

// Inspección y slicing zero-copy
b.length                                // int: tamaño en bytes
b.getByte(index)                        // int: lectura segura
b.setByte(index, val)                   // void: escritura de byte
b.slice(start, end)                     // Bytes: sub-bloque sin copias
b.copy(target, targetStart, srcStart)   // int: copia eficiente entre buffers

// Formatos y representaciones
b.toString()                            // str: decodificación UTF-8
b.toHex()                               // str: representación hexadecimal ("48656c6c6f")
b.toBase64()                            // str: codificación Base64 ("SGVsbG8=")
b.fill(255)                             // Bytes: llena con valor byte
```

---

## 2. Array — Métodos de Colección

> Fuente: `tests/05-arrays.vn`

```varn
// Propiedad
arr.length        // int

// Búsqueda
arr.indexOf(v)        // int (-1 si no encontrado)
arr.includes(v)       // bool
arr.find(predicate)   // T | null
arr.findIndex(pred)   // int (-1 si no encontrado)
arr.some(predicate)   // bool
arr.every(predicate)  // bool

// Transformación funcional
arr.map(fn)          // T2[]
arr.filter(fn)       // T[]
arr.reduce(fn, init) // T
arr.flat()           // T[] (un nivel)
arr.flatMap(fn)      // T[]

// Iteración
arr.forEach(fn)      // void

// Mutación
arr.push(v)          // void: añade al final
arr.pop()            // T: extrae del final
arr.sort()           // T[]: ordenado lexicográficamente (str) o numéricamente
arr.reverse()        // T[]

// Extracción
arr.slice(start, end) // T[]: [start, end)
arr.concat(arr2)      // T[]: concatenación
arr.join(sep)         // str: une elementos con separador
arr.join()            // str: separador "," por defecto

// Slicing por rango
arr[1..4]             // T[]: slice exclusivo (test 88)
arr[1..=3]            // T[]: slice inclusivo (test 88)

// Spread
[...arr1, ...arr2]    // T[]: combinar arrays
```

---

## 3. Map

> Fuente: `tests/17-map-set.vn`

```varn
const map = new Map<int>()
map.set("key", value)    // void
map.get("key")           // T | null
map.has("key")           // bool
map.delete("key")        // void
map.keys()               // str[]
map.values()             // T[]
map.size                 // int
map.clear()              // void
```

---

## 4. Set

> Fuente: `tests/17-map-set.vn`

```varn
const set = new Set<str>()
set.add("x")     // void (duplicados ignorados)
set.has("x")     // bool
set.delete("x")  // void
set.size         // int
set.clear()      // void
```

---

## 5. Range

> Fuente: `tests/18-ranges.vn`, `tests/88-range-indexing.vn`

```varn
const r = 0..5          // exclusivo: [0, 5)
const r = 1..=5         // inclusivo: [1, 5]
const r = Range.from(5, 10)

r.start             // int
r.end               // int
r.length            // int
r.contains(n)       // bool
r.toArray()         // int[]
r.step(n)           // int[]: cada n pasos
```

---

## 6. Error y Subclases

```varn
new Error("msg")
new TypeError("msg")
new RangeError("msg")

e.name      // str
e.message   // str
```

---

## 7. `std:task` — Concurrencia

Ver [`async_concurrency.md`](async_concurrency.md) para la documentación completa.

| Función/Clase | Descripción |
|--------------|-------------|
| `sleep(ms)` | Suspende durante ms milisegundos |
| `spawn(task)` | Tarea concurrente con handle awaitable |
| `parallel(tasks[])` | Ejecución paralela de múltiples tareas |
| `TaskGroup<T>` | Agrupa tareas con join/cancel/disposeAsync |
| `channel<T>(cap)` | Canal tipado con `{ tx, rx }` |
| `spawnIsolate(fn, args)` | Isolate con heap propio |
| `ChannelClosed` | Error al usar canal cerrado |

---

## 8. `std:test` — Framework de Tests

```varn
import { describe, test, expect, assertEqual } from "std:test"

describe("Grupo de pruebas", () => {
    test("prueba individual", () => {
        let x = 42
        expect(x).toBe(42)
        assertEqual(x, 42)
    })
})
```

---

## 9. `std:encoding` (JSON, CSV, TOML, Base64, Hex)

> Ver sección dedicada: [24. `std:encoding` — Formatos y Datos](#24-stdenencoding--formatos-y-datos)

---

## 11. `std:regex`

```varn
let re = Regex.compile("pattern")
re.test(str)              // bool
re.exec(str)              // { match, index, groups } | null
re.findAll(str)           // { match, index }[]
re.replace(str, replacement)  // str
re.split(str)             // str[]
```

---

## 12. `std:crypto`

```varn
UUID.v4()                  // str: "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx"
UUID.v7()                  // str: time-ordered UUID v7
UUID.isValid(str)          // bool
```

---

## 13. `std:time`

```varn
let now = DateTime.now()
now.toMs                   // int: timestamp en ms
now.toISOString()          // str: ISO 8601
now.addSeconds(n)          // DateTime
now.diff(other)            // int: diferencia en ms

let dur = new Duration(5000)
dur.totalMilliseconds      // 5000
dur.seconds                // 5
dur.add(other)             // Duration

let pdt = new PlainDateTime(year, month, day, hour, min, sec, ms)
pdt.year, pdt.month, pdt.day  // meses 1-based (enero=1)
```

---

## 14. `std:process`

```varn
let r = Process.exec("echo hello")
r.exitCode    // int
r.success     // bool
r.stdout      // str
```

---

## 15. `std:compress`

```varn
Compress.gzip(str)       // bytes
Compress.gunzip(bytes)   // str
Compress.deflate(str)    // bytes
Compress.inflate(bytes)  // str

Tar.create(srcDir, tarPath)        // bool
Tar.extract(tarPath, destDir)      // bool
Zip.create(srcDir, zipPath)        // bool
Zip.extract(zipPath, destDir)      // bool
```

---

## 16. `std:cli`

```varn
let parsed = CLI.parse(args: str[])
parsed.flags["name"]        // str
parsed.positionals          // str[]
```

---

## 17. `std:env`

```varn
Env.set(key, value)              // void
Env.get(key)                     // str | null
Env.getInt(key, default)         // int
Env.getBool(key, default)        // bool
Env.parse(dotenvContent)         // Record<str, str>
```

---

## 18. `std:path`

```varn
Path.join(...parts)        // str: ruta unida
Path.extname(filename)     // str: ".gz", ".html"
```

---

## 19. `std:sqlite`

```varn
let db = new Database(":memory:")
db.exec(sql)                  // int: rowsAffected
db.all(sql, ...params)        // dynamic[]
db.get(sql, ...params)        // dynamic | null
db.sql`SELECT ... WHERE x = ${val}`      // dynamic[]
db.sqlOne`SELECT ... WHERE id = ${val}`  // dynamic | null
db.close()
```

---

## 20. `std:ws`

```varn
typeof WebSocketClient === "class"   // validación de shape
// Client API a confirmar con docs de la implementación
```

---

## 21. `std:fs`

```varn
read(path)            // str
write(path, content)  // void
exists(path)          // bool
remove(path)          // void
mkdir(path)           // void
removeAll(path)       // void
```

---

## 22. `std:sys`

```varn
platform()    // str: "windows" | "linux" | ...
cwd()         // str: directorio de trabajo
env(key)      // str: valor de variable de entorno
```

---

## 23. `std:reflect`

```varn
const key = MetaKey.create<T>()
key.set(ClassRef, value)
key.get(ClassRef)         // T | null
```

---

## 24. `std:encoding` — Formatos y Datos

> Fuente: `tests/110-stdlib-consolidation.vn`

```varn
import { JSON, CSV, Base64, Hex, TOML } from "std:encoding";

// Base64
let b64 = Base64.encode("Hello");
let str = Base64.decode(b64);

// Hex
let hex = Hex.encode("Data");
let data = Hex.decode(hex);

// TOML
let tomlStr = TOML.stringify({ server: { host: "localhost", port: 8080 } });
let parsed = TOML.parse(tomlStr);

// JSON & CSV
let json = JSON.stringify({ ok: true });
let csv = CSV.stringify([["a", "b"], ["1", "2"]]);
```

---

## 25. `std:cli` — Terminal, Argumentos y Colores

> Fuente: `tests/110-stdlib-consolidation.vn`

```varn
import { Color, Table, CLI, Prompt, prompt, confirm } from "std:cli";

// Colores e Introspección ANSI
let msg = Color.red("error");
let plain = Color.stripAnsi(msg);
let interactive = Color.isInteractive(); // bool (respeta NO_COLOR y TERM)

// Tablas ASCII/ANSI
let table = new Table(["ID", "Name"]);
table.addRow(["1", "Alpha"]);
let rendered = table.render();

// Parser de Argumentos
let args = CLI.parse(["--port=8080", "-v", "app.vn"]);
// args.flags["port"] == "8080", args.positionals == ["app.vn"]

// Prompts Interactivos
let name = prompt("Nombre de usuario");
let ok = confirm("¿Desea continuar?", true);
```

---

## 26. `std:task` (Sincronización Concurrente)

> Fuente: `tests/110-stdlib-consolidation.vn`

```varn
import { Mutex, Semaphore, WaitGroup, Task, spawn } from "std:task";

// Mutex asíncrono
let mutex = new Mutex();
await mutex.withLock(async (): Task<void> => {
    // sección crítica protegida
});

// Semáforo de capacidad concurrente
let sem = new Semaphore(4);
await sem.withPermit(async (): Task<void> => {
    // máximo 4 tareas en paralelo
});

// WaitGroup (concurrencia cooperativa)
let wg = new WaitGroup();
wg.add(2);
spawn((async (): Task<void> => { /* do work */ wg.done(); })());
spawn((async (): Task<void> => { /* do work */ wg.done(); })());
await wg.wait();
```

---

## 27. `std:log` — Diagnóstico y Logging Estructurado

> Fuente: `tests/110-stdlib-consolidation.vn`

```varn
import { LogLevel, Logger, getLogger, info, warn, error } from "std:log";

// Logger configurable
let logger = new Logger("api", LogLevel.Info, false, true);
logger.info("Servidor iniciado", { "port": 3000 });

// Modo JSON estructurado
let jsonLogger = new Logger("service", LogLevel.Debug, true, false);
jsonLogger.debug("Petición procesada", { "status": 200, "ms": 12 });

// Sub-loggers contextuales
let childLogger = logger.child("database");
childLogger.warn("Conexión lenta detectada");

// Helpers globales directos
info("Mensaje global");
error("Fallo crítico");
```

---

## 28. `std:collections` — Estructuras de Datos Avanzadas

> Fuente: `tests/110-stdlib-consolidation.vn`

```varn
import { PriorityQueue, LRUCache, List, Stack, Queue } from "std:collections";

// PriorityQueue (Min-Heap default o comparador personalizado)
let pq = new PriorityQueue<int>();
pq.push(30);
pq.push(10);
let min = pq.pop(); // 10

// LRUCache (evicción automática del menos recientemente usado)
let cache = new LRUCache<str>(2);
cache.set("a", "alpha");
cache.set("b", "beta");
cache.get("a"); // "a" se marca como reciente
cache.set("c", "gamma"); // desaloja "b"
```

---

## 29. `std:net` — Redes de Bajo Nivel y Transporte TCP

> Fuente: `tests/111-network-apis-overhaul.vn`

```varn
import { TcpListener, TcpStream, connect, listen, isIP, isIPv4, isIPv6 } from "std:net";

// Servidor TCP
let listener = TcpListener.listen(8080);
let client = await listener.accept();
let msg = await client.read();
await client.writeAll("Eco: " + msg);
client.close();
listener.close();

// Cliente TCP
let sock = await TcpStream.connect("127.0.0.1", 8080);
await sock.writeAll("Hola Servidor\n");
let res = await sock.read();
sock.close();

// Validaciones IP
isIPv4("192.168.1.1"); // true
isIPv6("::1");          // true
```

---

## 30. `std:http` — Cliente y Servidor HTTP Completo

> Fuente: `tests/111-network-apis-overhaul.vn`

```varn
import { HttpServer, Request, Response, HttpResponse, json, http, fetch } from "std:http";

// Servidor HTTP con CORS y enrutamiento
let app = new HttpServer();
app.cors("*", "GET,POST,PUT,PATCH,DELETE,OPTIONS");

app.get("/api/users", (req: Request, res: HttpResponse) => {
    return json([{ id: 1, name: "Alice" }]);
});

app.post("/api/users", (req: Request, res: HttpResponse) => {
    return json({ created: true }, 201);
});

// Cliente HTTP por Namespace
let getRes = await http.get("https://api.example.com/data");
let postRes = await http.post("https://api.example.com/items", { name: "Nuevo" });
let patchRes = await http.patch("https://api.example.com/items/1", { status: "active" });
```

---

## 31. `std:ws` — WebSockets Bidireccionales

> Fuente: `tests/111-network-apis-overhaul.vn`

```varn
import { WebSocket, WebSocketClient, WebSocketReadyState } from "std:ws";

// Cliente WebSocket con ciclo de eventos
let ws = new WebSocket("ws://localhost:8080/feed");

ws.onOpen(() => {
    ws.send("subscribirse");
});

ws.onMessage((data: str) => {
    print("Recibido: " + data);
});

ws.onError((err: str) => {
    print("Error: " + err);
});

ws.onClose(() => {
    print("Conexión cerrada");
});

// Estados de conexión
if (ws.readyState === WebSocketReadyState.Open) {
    ws.send("ping");
}
```

---

## 32. `std:io` — Streaming E/S con Backpressure

> Fuente: `tests/112-canonical-bytes-and-streams.vn`

```varn
import { Stream, AsyncReader, AsyncWriter, Reader, Writer } from "std:io";

// Contratos asíncronos para streaming binario de alto rendimiento
export interface AsyncReader {
    read(size?: int): Task<Bytes?>;
}

export interface AsyncWriter {
    write(data: Bytes): Task<int>;
    flush(): Task<void>;
    close(): void;
}

// Tubería asíncrona fuertemente tipada con contrapresión garantizada mediante await
await Stream.pipe(reader, writer, 4096);
```

