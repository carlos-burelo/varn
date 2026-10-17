# Varn Language — Módulos y Paquetes

> Fuentes: `tests/21-async.vn`, `tests/27-decorators.vn`, `tests/42-stdlib-comprehensive-test.vn`, `tests/47-isolates-multithread.vn`, `tests/54-channels.vn`, `tests/68-module-locals.vn`, `tests/80-capabilities-security.vn`, `tests/82-std-csv.vn`, `tests/83-canonical-types-regex-uuid-time.vn`, `tests/84-stdlib-process-compress-cli-env.vn`, `tests/85-stdlib-sqlite-ws-archive.vn`, `tests/86-tagged-templates.vn`.

---

## 1. Declaraciones `import`

### Importación nombrada desde stdlib

```varn
import { sleep, TaskGroup, spawn, parallel } from "std:task"   // tests/21-async.vn
import { channel, Sender, Receiver, ChannelClosed, spawnIsolate } from "std:task"   // tests/47-isolates-multithread.vn
import { describe, it, expect } from "std:test"   // tests/21-async.vn
import { MetaKey, ClassRef } from "std:reflect"   // tests/27-decorators.vn
import { WebSocket } from "std:ws"   // tests/85-stdlib-sqlite-ws-archive.vn
import { readText, writeText, remove, exists } from "std:fs"   // tests/80-capabilities-security.vn
import { Env } from "std:env"
import { platform, cwd } from "std:sys"
```

Resto de exports por paquete: ver `standard_library.md` (solo lo citado
con `tests/*.vn`).

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
// tests/47-isolates-multithread.vn
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
// tests/76-generator-values.vn
```

---

## 4. Paquetes de la Stdlib

Ver `standard_library.md` (mapa: `../STDLIB_ARCHITECTURE.md`). Aquí solo los
imports ejercitados:

```varn
import { sleep, TaskGroup, spawn, parallel } from "std:task"   // tests/21-async.vn
import { channel, Sender, Receiver, ChannelClosed, spawnIsolate } from "std:task"   // tests/47-isolates-multithread.vn
import { describe, it, expect } from "std:test"   // tests/21-async.vn
import { MetaKey, ClassRef } from "std:reflect"   // tests/27-decorators.vn
import { readText, writeText, remove, exists } from "std:fs"   // tests/80-capabilities-security.vn
import { Env } from "std:env"
import { platform, cwd } from "std:sys"
```

APIs por paquete (`CSV`, `Regex`, `UUID`, `DateTime`, `Process`, `Compress`,
`CLI`, `Env`, `Path`, `Database`, `WebSocket`): solo lo citado en
`standard_library.md` con `tests/*.vn`.

