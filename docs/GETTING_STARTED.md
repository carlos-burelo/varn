# Primeros pasos

## 1. Compilar `vn`

```bash
git clone https://github.com/carlos-burelo/varn
cd varn
cargo build --profile quick --bin vn      # ~3 min; binario en target/quick/vn
```

Toolchain, perfiles y PATH: [`INSTALL.md`](INSTALL.md).

## 2. Hola mundo

`hola.vn`:

```vn
function saludar(nombre: str): str {
    return `Hola, ${nombre} desde Varn!`
}

print(saludar("Mundo"))
```

```bash
vn run hola.vn        # Hola, Mundo desde Varn!
vn check hola.vn      # solo tipos, sin ejecutar
vn debug -p bytecode hola.vn
```

## 3. Algo más real

```vn
class Contador {
    private n: int = 0
    inc(): int {
        this.n = this.n + 1
        return this.n
    }
}

async function doble(n: int): int {
    return n * 2
}

const c = new Contador()
c.inc()
print(`contador = ${c.inc()}`)

const palabras = ["varn", "es", "tipado"].map((w) => w.length)
print(palabras)
print(await doble(21))
```

Puntos clave:

- Tipos estáticos con inferencia: `let x = 1` es `int`; `dynamic` existe pero es explícito.
- Nulos explícitos: `str?`, con `?.`, `??` y estrechamiento por flujo.
- `async`/`await`, generadores (`function*`), `match`, enums con payload, clases, interfaces, extensiones.
- La std se importa por esquema: `std:fs`, `std:http`, `std:task`, `std:test`…

Guía del lenguaje: [`lang/README.md`](lang/README.md).

## 4. Un proyecto

```bash
vn init mi-proyecto
cd mi-proyecto
vn run main.vn
```

`vn init` crea `main.vn` y `varn.toml`. La caché es global (`~/.vn/cache`, o
`VARN_HOME`/`VARN_CACHE_DIR`); `vn run` no escribe en el proyecto.

## 5. Tests

```vn
import { describe, it, expect } from "std:test"

await describe("suma", async () => {
    await it("suma enteros", () => {
        expect(1 + 2).toBe(3)
    })
})
```

```bash
vn test               # cada .vn de tests/
vn test ruta/al/archivo.vn
```

## Siguiente

- [`CLI_REFERENCE.md`](CLI_REFERENCE.md) — todos los comandos y variables de entorno.
- [`CLI_INSPECT.md`](CLI_INSPECT.md) — `vn debug -p …` para ver cada fase.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — cómo está hecho el compilador.
