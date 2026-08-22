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

## 9. `std:csv`

```varn
CSV.parse(str)                        // con header
CSV.parse(str, { delimiter: ";" })    // custom delimiter
CSV.parse(str, { hasHeader: false })  // sin header → str[][]
CSV.stringify(rows)                   // str: CSV con header
```

---

## 10. `std:json`

```varn
JSON.parse(jsonStr)        // object
JSON.stringify(obj)        // str
```

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
