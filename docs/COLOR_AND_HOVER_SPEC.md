# Especificación Exhaustiva de Resaltado Sintáctico, Tokens Semánticos y Hovers en Varn

> **Documento Canónico de Referencia del Lenguaje**  
> **Versión**: 1.0.0 (Varn Enterprise & Developer Experience)  
> **Área**: Language Server Protocol (`varn-lsp`), Sintaxis TextMate, Extensiones de Editor (VS Code, NeoVim, Helix) y Herramientas de Inspección  
> **Crates Normativos**: `varn-core`, `varn-lexer`, `varn-parser`, `varn-checker`, `varn-lsp`, `varn-builtins`, `varn-modules`

---

## Índice General

1. [Fundamentos y Arquitectura de Renderizado](#1-fundamentos-y-arquitectura-de-renderizado)
2. [Taxonomía de Tokens Semánticos y Paleta Universal de Colores](#2-taxonomía-de-tokens-semánticos-y-paleta-universal-de-colores)
3. [Gramática Léxica y Literales](#3-gramática-léxica-y-literales)
4. [Sistema de Tipos Estático y Representación Estructural](#4-sistema-de-tipos-estático-y-representación-estructural)
5. [Variables, Constantes y Bindings de Ámbito](#5-variables-constantes-y-bindings-de-ámbito)
6. [Funciones, Métodos, Closures y Generadores](#6-funciones-métodos-closures-y-generadores)
7. [Programación Orientada a Objetos, Structs y Extensiones](#7-programación-orientada-a-objetos-structs-y-extensiones)
8. [Enums, Variantes y Algebraic Data Types (ADTs)](#8-enums-variantes-y-algebraic-data-types-adts)
9. [Flujo de Control Avanzado y Pattern Matching](#9-flujo-de-control-avanzado-y-pattern-matching)
10. [Concurrencia, Runtime Asíncrono e Isolates](#10-concurrencia-runtime-asíncrono-e-isolates)
11. [Operadores Especiales, Pipeline y Sintaxis Expresiva](#11-operadores-especiales-pipeline-y-sintaxis-expresiva)
12. [Decoradores, Metadatos y Capacidades de Frontera Host](#12-decoradores-metadatos-y-capacidades-de-frontera-host)
13. [Biblioteca Estándar (`std:*`) y Módulos](#13-biblioteca-estándar-std-y-módulos)
14. [Reglas de Desambiguación y Matriz Maestra de Contraste Visual](#14-reglas-de-desambiguación-y-matriz-maestra-de-contraste-visual)

---

## 1. Fundamentos y Arquitectura de Renderizado

El sistema de experiencia de desarrollo (DX) de Varn combina dos capas sincronizadas:
1. **Capa Léxica / TextMate Grammar (`varn.tmLanguage.json`)**: Realiza el tokenizado sintáctico rápido síncrono en el cliente basado en expresiones regulares (identificando palabras clave, delimitadores, literales y operadores).
2. **Capa Semántica LSP (`varn-lsp` Semantic Tokens Provider)**: Una vez analizado el AST por `varn-parser` y tipado por `varn-checker`, el servidor LSP superpone tokens semánticos enriquecidos (`SemanticTokenType` y `SemanticTokenModifier`), resolviendo ambigüedades, tipos inferidos, llamadas polimórficas y desambiguación de miembros.

```
  Código Fuente (.vn)
        │
        ├──► [TextMate Grammar] ────► Resaltado Sintáctico Inmediato (Scopes)
        │
        └──► [varn-parser + checker] ──► [varn-lsp] ────► Semantic Tokens Enriquecidos + Hovers Markdown
```

### 1.1. Contrato de Hover Markdown

Todo Hover devuelto por `varn-lsp` implementa el estándar `MarkupKind::Markdown` y debe adherirse rígidamente a esta plantilla de bloques:

````markdown
```varn
[decoradores / atributos]
[visibilidad] [modificadores] [palabra_clave] [nombre_simbolo][<Genéricos>][firma_completa]: [tipo_retorno]
```
*(insignia de procedencia o módulo si no es local, e.g. `(from std:http)` o `(parameter)`)*
***
[Documentación docstring `/** ... */` parseada en Markdown enriquecido]

[Avisos de advertencia, deprecación `@deprecated` o requerimiento de capacidades `@capability`]
````

---

## 2. Taxonomía de Tokens Semánticos y Paleta Universal de Colores

### 2.1. Mapeo Canónico de Semantic Tokens y Modificadores

| Tipo de Token LSP (`SemanticTokenType`) | Modificadores Asociados (`SemanticTokenModifier`) | TextMate Scope Canónico |
| :--- | :--- | :--- |
| `KEYWORD` (`0`) | `DECLARATION` | `keyword.control.varn`, `keyword.other.varn` |
| `TYPE` (`1`) | `DECLARATION`, `READONLY`, `DEFAULT_LIBRARY` | `entity.name.type.varn`, `support.type.primitive.varn` |
| `VARIABLE` (`2`) | `DECLARATION`, `READONLY`, `STATIC` | `variable.other.varn`, `variable.other.constant.varn` |
| `FUNCTION` (`3`) | `DECLARATION`, `ASYNC`, `STATIC`, `DEFAULT_LIBRARY` | `entity.name.function.varn`, `support.function.varn` |
| `CLASS` (`4`) | `DECLARATION`, `ABSTRACT`, `DEFAULT_LIBRARY` | `entity.name.type.class.varn`, `support.class.varn` |
| `PARAMETER` (`5`) | `DECLARATION`, `READONLY` | `variable.parameter.varn` |
| `PROPERTY` (`6`) | `DECLARATION`, `READONLY`, `STATIC` | `variable.other.property.varn` |
| `NUMBER` (`7`) | — | `constant.numeric.varn` |
| `STRING` (`8`) | — | `string.quoted.varn`, `string.template.varn` |
| `ENUM_MEMBER` (`9`) | `DECLARATION`, `READONLY` | `variable.other.enummember.varn`, `constant.other.enum.varn` |
| `NAMESPACE` (`10`) | `DECLARATION` | `entity.name.namespace.varn`, `support.module.varn` |
| `INTERFACE` (`11`) | `DECLARATION`, `DEFAULT_LIBRARY` | `entity.name.type.interface.varn` |
| `TYPE_PARAMETER` (`12`)| `DECLARATION` | `entity.name.type.parameter.varn` |

---

### 2.2. Guía Cromática de Referencia (Temas Oscuros y Claros)

| Elemento Semántico | Tema Oscuro (Dark+ / OneDark) | Tema Claro (Light+ / GitHub Light) | Clase Visual y Descripción |
| :--- | :--- | :--- | :--- |
| **Control Keywords** (`if`, `match`, `return`, `async`, `await`) | `#C586C0` (Magenta) | `#AF00DB` (Púrpura Profundo) | Palabras de control de flujo y ejecución |
| **Declaration Keywords** (`let`, `const`, `var`, `fn`, `class`) | `#569CD6` (Azul Cielo) | `#0000FF` (Azul Puro) | Palabras de introducción de estructuras |
| **Tipos Primitivos** (`int`, `str`, `bool`, `float`, `decimal`) | `#4EC9B0` (Verde Turquesa) | `#267F99` (Teal Oscuro) | Tipos intrínsecos de la máquina virtual |
| **Clases, Structs y Extensiones** (`User`, `Vector3`, `File`) | `#4EC9B0` (Verde Menta) | `#267F99` (Teal) | Tipos nominales instanciables |
| **Interfaces y Type Parameters** (`Reader`, `<T, K>`) | `#B8D7A3` (Verde Olivo Claro) | `#008000` (Verde) | Contratos estructurales y parámetros genéricos |
| **Funciones y Métodos** (`println`, `fetchData`, `list.map`) | `#DCDCAA` (Amarillo Oro) | `#795E26` (Marrón Dorado) | Símbolos invocables |
| **Variables Locales y Parámetros** (`total`, `count`, `item`) | `#9CDCFE` (Celeste Suave) | `#001080` (Azul Marino) | Bindings mutables e inmutables de ámbito |
| **Constantes Globales y Miembros Enum** (`MAX_RETRIES`, `Ok`) | `#4FC1FF` (Azul Eléctrico) | `#0070C1` (Azul Cobalto) | Valores de sólo lectura conocidos en compilación |
| **Campos y Propiedades** (`user.name`, `config.port`) | `#9CDCFE` (Celeste) | `#001080` (Azul) | Accesores de miembros de estructuras |
| **Literales de Cadena y Caracteres** (`"texto"`, `'a'`) | `#CE9178` (Naranja Suave) | `#A31515` (Rojo Carmesí) | Datos de texto UTF-8 |
| **Literales Numéricos y Booleanos** (`42`, `3.14m`, `true`, `null`)| `#B5CEA8` (Verde Salvia) | `#098658` (Verde Esmeralda) | Valores escalares y discriminantes booleanos |
| **Decoradores y Atributos** (`@inline`, `@deprecated`) | `#DCDCAA` (Oro Brillante) | `#795E26` (Dorado) | Metadatos y transformaciones de compilador |
| **Operadores y Delimitadores** (`\|>`, `??`, `===`, `=>`, `{}`) | `#D4D4D4` (Blanco Humo) | `#000000` (Negro) | Puntuación y operadores semánticos |
| **Comentarios y Docstrings** (`// ...`, `/** ... */`) | `#6A9955` (Verde Bosque) | `#008000` (Verde Comentario) | Documentación no ejecutable |

---

## 3. Gramática Léxica y Literales

### 3.1. Enteros y Números de Precisión

Varn distingue internamente entre enteros rápidos de 48 bits, flotantes de doble precisión, números decimales de alta precisión financiera (`decimal`) y enteros arbitrarios (`bigint`):

| Literal / Sintaxis | Semantic Token | Resaltado | Hover Esperado |
| :--- | :--- | :--- | :--- |
| `42` | `NUMBER` | `#B5CEA8` | ````varn\n42: int\n````\n*Entero de 48 bits (i48) con aritmética de bajo coste y wrap seguro.* |
| `0xFF_5A` *(Hexadecimal)* | `NUMBER` | `#B5CEA8` | ````varn\n65370: int\n````\n*(Hex: 0xFF5A)* |
| `0b1011_0001` *(Binario)* | `NUMBER` | `#B5CEA8` | ````varn\n177: int\n````\n*(Bin: 0b10110001)* |
| `0o755` *(Octal)* | `NUMBER` | `#B5CEA8` | ````varn\n493: int\n````\n*(Oct: 0o755)* |
| `3.14159` *(Float)* | `NUMBER` | `#B5CEA8` | ````varn\n3.14159: float\n````\n*Número de punto flotante de doble precisión (IEEE 754 f64).* |
| `129.99m` *(Decimal)* | `NUMBER` | `#B5CEA8` | ````varn\n129.99m: decimal\n````\n*Número de coma fija de 128 bits para operaciones financieras exactas.* |
| `9007199254740993n` *(BigInt)* | `NUMBER` | `#B5CEA8` | ````varn\n9007199254740993n: bigint\n````\n*Entero de precisión arbitraria sin límite de desbordamiento.* |

---

### 3.2. Strings, Caracteres y Template Literals

| Literal / Sintaxis | Semantic Token | Resaltado | Hover Esperado |
| :--- | :--- | :--- | :--- |
| `"Hola Varn\n"` | `STRING` | `#CE9178` | ````varn\n"Hola Varn\n": str\n````\n*(Longitud: 10 caracteres, 10 bytes UTF-8)* |
| `'z'` *(Carácter)* | `STRING` | `#CE9178` | ````varn\n'z': char\n````\n*(Unicode Scalar Value: U+007A, decimal: 122)* |
| `'\u{1F680}'` *(Emoji Rocket)* | `STRING` | `#CE9178` | ````varn\n'🚀': char\n````\n*(Unicode Scalar Value: U+1F680)* |
| `\x60Total: ${price * count}\x60` | Parte fija: `STRING`<br>Expresión `${...}`: Evaluada semánticamente | `#CE9178` (texto)<br>`#9CDCFE` (vars) | *Hover sobre la plantilla completa:*<br>````varn\ntemplate: str\n````<br>*Hover sobre `${price * count}`:*<br>````varn\n(expression) price * count: float\n```` |

---

## 4. Sistema de Tipos Estático y Representación Estructural

### 4.1. Tipos Primitivos y Especiales

| Tipo | Semantic Token | Resaltado | Hover Exacto Esperado |
| :--- | :--- | :--- | :--- |
| `int` | `TYPE` | `#4EC9B0` | ````varn\ntype int\n````\n*Entero con signo de 48 bits optimizado en nanosegundos.* |
| `float` | `TYPE` | `#4EC9B0` | ````varn\ntype float\n````\n*Número en coma flotante IEEE 754 de 64 bits.* |
| `decimal` | `TYPE` | `#4EC9B0` | ````varn\ntype decimal\n````\n*Coma fija de 128 bits con redondeo financiero.* |
| `bigint` | `TYPE` | `#4EC9B0` | ````varn\ntype bigint\n````\n*Entero de longitud arbitraria con asignación en heap.* |
| `str` | `TYPE` | `#4EC9B0` | ````varn\ntype str\n````\n*Cadena inmutable UTF-8 con SSO (Small String Optimization).* |
| `char` | `TYPE` | `#4EC9B0` | ````varn\ntype char\n````\n*Punto de código Unicode indivisible de 32 bits.* |
| `bool` | `TYPE` | `#4EC9B0` | ````varn\ntype bool\n````\n*Valor lógico booleano (`true` o `false`).* |
| `symbol` | `TYPE` | `#4EC9B0` | ````varn\ntype symbol\n````\n*Identificador único e inmutable a nivel de proceso.* |
| `void` | `TYPE` | `#4EC9B0` | ````varn\ntype void\n````\n*Indica la ausencia intencional de valor de retorno.* |
| `never` | `TYPE` | `#4EC9B0` | ````varn\ntype never\n````\n*Tipo fondo que denota funciones que divergen (bucle infinito o `throw`).* |
| `dynamic` | `TYPE` | `#4EC9B0` | ````varn\ntype dynamic\n````\n*Desactiva la comprobación estática; despachado mediante Inline Caches.* |

---

### 4.2. Tipos Compuestos: Arrays, Maps, Sets, Tuplas y Records

| Constructo | Declaración / Uso | Resaltado | Hover Exacto Esperado |
| :--- | :--- | :--- | :--- |
| **Array genérico** | `let list: Array<int>` o `let list: int[]` | `Array` (`TYPE`), `int` (`TYPE`) | ````varn\nlet list: Array<int>\n```` |
| **Map asociativo** | `let map: Map<str, User>` | `Map` (`TYPE`), `str` (`TYPE`), `User` (`CLASS`) | ````varn\nlet map: Map<str, User>\n```` |
| **Set de unicidad**| `let unique: Set<int>` | `Set` (`TYPE`), `int` (`TYPE`) | ````varn\nlet unique: Set<int>\n```` |
| **Tupla Posicional** | `let pair: (int, str, bool)` | Delimitadores y tipos resaltados | ````varn\nlet pair: (int, str, bool)\n```` |
| **Acceso a Tupla** | `let first = pair.0` | `0` (`PROPERTY`) | ````varn\n(tuple element 0) pair.0: int\n```` |
| **Record Anónimo** | `let pt: { x: int, y: int, name?: str }` | `x`, `y`, `name` (`PROPERTY`) | ````varn\nlet pt: {\n    x: int,\n    y: int,\n    name?: str\n}\n```` |
| **Unión de Tipos** | `let id: int \| str` | `int`, `str` (`TYPE`) | ````varn\nlet id: int | str\n```` |
| **Nullable (`T?`)** | `let u: User?` | `User` (`CLASS`) | ````varn\nlet u: User | null\n```` |
| **Alias (`type`)** | `type Callback<T> = (data: T) => void` | `Callback` (`TYPE`), `T` (`TYPE_PARAMETER`) | ````varn\ntype Callback<T> = fn(data: T): void\n```` |

---

## 5. Variables, Constantes y Bindings de Ámbito

### 5.1. Reglas de Inferencia y Modificadores

| Declaración | Inferencia / Semántica | Semantic Token & Modificadores | Hover Esperado |
| :--- | :--- | :--- | :--- |
| `const MAX_CONNECTIONS = 500` | Inferido a `int`, inmutable | `VARIABLE` + `DECLARATION` + `READONLY` (`#4FC1FF`) | ````varn\nconst MAX_CONNECTIONS: int = 500\n```` |
| `let buffer = new StringBuffer()` | Inferido a `StringBuffer` | `VARIABLE` + `DECLARATION` (`#9CDCFE`) | ````varn\nlet buffer: StringBuffer\n```` |
| `var flag = false` | Reasignable, mutable | `VARIABLE` + `DECLARATION` (`#9CDCFE`) | ````varn\nvar flag: bool\n```` |
| `pub const API_VERSION: str = "v2"` | Exportado a nivel de módulo | `VARIABLE` + `READONLY` (`#4FC1FF`) | ````varn\npub const API_VERSION: str = "v2"\n(exported symbol)\n```` |

---

### 5.2. Destructuring Complejo

| Patrón de Destructuring | Elemento Bajo Cursor | Resaltado | Hover Esperado |
| :--- | :--- | :--- | :--- |
| `let (x, y, ...rest) = getCoords()` | Hover en `x` | `VARIABLE` (`#9CDCFE`) | ````varn\nlet x: int\n```` |
| `let (x, y, ...rest) = getCoords()` | Hover en `rest` | `VARIABLE` (`#9CDCFE`) | ````varn\nlet rest: Array<int>\n```` |
| `let { id, profile: { username } } = user` | Hover en `username` | `VARIABLE` (`#9CDCFE`) | ````varn\nlet username: str\n```` |
| `let { port = 8080 } = config` | Hover en `port` | `VARIABLE` (`#9CDCFE`) | ````varn\nlet port: int\n```` |

---

## 6. Funciones, Métodos, Closures y Generadores

### 6.1. Funciones Libres y Asíncronas

```varn
/**
 * Realiza una petición HTTP con reintentos exponenciales.
 * @param url Dirección de destino.
 * @param timeoutMs Tiempo máximo de espera en milisegundos.
 */
async function fetchWithRetry(url: str, timeoutMs: int = 5000): Task<Response>
```

- **Resaltado**: `fetchWithRetry` (`FUNCTION` + `ASYNC`), `url` (`PARAMETER`), `timeoutMs` (`PARAMETER`), `Task` (`TYPE`), `Response` (`CLASS`).
- **Hover Exacto Esperado**:
````markdown
```varn
async function fetchWithRetry(url: str, timeoutMs: int = 5000): Task<Response>
```
***
Realiza una petición HTTP con reintentos exponenciales.

**Parámetros**:
- `url`: Dirección de destino.
- `timeoutMs`: Tiempo máximo de espera en milisegundos.
````

---

### 6.2. Generadores Síncronos y Asíncronos

| Constructo | Declaración | Semantic Token | Hover Esperado |
| :--- | :--- | :--- | :--- |
| **Generador Síncrono** | `function* fibonacci(): Generator<int>` | `fibonacci` (`FUNCTION`) | ````varn\nfunction* fibonacci(): Generator<int>\n```` |
| **Generador Asíncrono** | `async function* streamLines(f: File): AsyncGenerator<str>` | `streamLines` (`FUNCTION` + `ASYNC`) | ````varn\nasync function* streamLines(f: File): AsyncGenerator<str>\n```` |
| **Expresión `yield`** | `yield nextValue;` | `yield` (`KEYWORD`) | ````varn\nyield nextValue: int\n```` |
| **Expresión `yield*`** | `yield* subGenerator();` | `yield` (`KEYWORD`) | ````varn\nyield* subGenerator(): Generator<int>\n```` |

---

### 6.3. Named Arguments y Rest Parameters

| Firma y Sitio de Llamada | Elemento | Resaltado | Hover Esperado |
| :--- | :--- | :--- | :--- |
| `function openWindow(title: str, width: int = 800, height: int = 600, resizable: bool = true)` | Declaración | `title`, `width`, `height`, `resizable` (`PARAMETER`) | ````varn\nfunction openWindow(title: str, width: int = 800, height: int = 600, resizable: bool = true): Window\n```` |
| `openWindow("App", resizable: false, width: 1024)` | Hover sobre `resizable:` en la llamada | `PROPERTY` / `PARAMETER` (`#9CDCFE`) | ````varn\n(parameter) resizable: bool\n````\n*Valor por defecto: `true`* |
| `openWindow("App", resizable: false, width: 1024)` | Hover sobre `width:` en la llamada | `PROPERTY` / `PARAMETER` (`#9CDCFE`) | ````varn\n(parameter) width: int\n````\n*Valor por defecto: `800`* |

---

## 7. Programación Orientada a Objetos, Structs y Extensiones

### 7.1. Clases, Herencia y Modificadores de Acceso

```varn
pub abstract class DatabaseConnection<TDriver> implements Disposable, MetricSource {
    pub readonly connectionId: str
    priv socket: Socket?
    prot retryCount: int = 0
    pub static defaultPoolSize: int = 16

    pub constructor(connectionId: str)
    pub abstract async function connect(): Task<void>
    pub async function executeQuery(query: str): Task<ResultSet>
}
```

#### Contratos de Hover en POO:
* **Hover en `DatabaseConnection`**:
````markdown
```varn
pub abstract class DatabaseConnection<TDriver> implements Disposable, MetricSource {
    pub readonly connectionId: str
    pub static defaultPoolSize: int
    constructor(connectionId: str)
    abstract async function connect(): Task<void>
    async function executeQuery(query: str): Task<ResultSet>
}
```
````
* **Hover en `connection.connectionId`**:
````markdown
```varn
(property) DatabaseConnection.connectionId: str
```
*(readonly)*
````
* **Hover en `DatabaseConnection.defaultPoolSize`**:
````markdown
```varn
(static property) DatabaseConnection.defaultPoolSize: int = 16
```
````
* **Hover en `this` dentro de `executeQuery`**:
````markdown
```varn
this: DatabaseConnection<TDriver>
```
````
* **Hover en `super.connect()`**:
````markdown
```varn
(method) BaseClass.connect(): Task<void>
```
````

---

### 7.2. Structs (Value Types en Stack)

Los `struct` en Varn se alojan directamente en la pila sin punteros en el heap:

```varn
pub struct Vector3 {
    pub x: float
    pub y: float
    pub z: float

    pub function length(): float {
        return Math.sqrt(this.x * this.x + this.y * this.y + this.z * this.z);
    }
}
```

* **Hover en `Vector3`**:
````markdown
```varn
pub struct Vector3 {
    pub x: float
    pub y: float
    pub z: float
    function length(): float
}
```
*Value Type alojado directamente en pila.*
````

---

### 7.3. Extensiones de Tipo (`extension ... on ...`)

Varn permite agregar métodos a tipos existentes sin modificar su código fuente:

```varn
extension StringValidation on str {
    pub function isEmail(): bool {
        return this.contains("@") && this.contains(".");
    }
}
```

* **Hover en `StringValidation`**:
````markdown
```varn
extension StringValidation on str {
    function isEmail(): bool
}
```
````
* **Hover en `"admin@varn.org".isEmail()`**:
````markdown
```varn
(extension method) StringValidation.isEmail(): bool
```
*(defined on `str` via `extension StringValidation`)*
````

---

## 8. Enums, Variantes y Algebraic Data Types (ADTs)

Varn soporta tanto Enums C-Style (con valores asociados) como ADTs completos con variantes que transportan datos (records y tuplas):

### 8.1. Enums Simples vs. Enums con Payload (ADTs)

```varn
// Enum C-Style
pub enum HttpStatus {
    Ok = 200,
    NotFound = 404,
    InternalError = 500
}

// Algebraic Data Type (ADT)
pub enum Result<T, E> {
    Ok(T),
    Err(E),
    Pending
}
```

#### Contratos de Hover en Enums y ADTs:
* **Hover en `HttpStatus`**:
````markdown
```varn
pub enum HttpStatus {
    Ok = 200,
    NotFound = 404,
    InternalError = 500
}
```
````
* **Hover en `HttpStatus.Ok`**:
````markdown
```varn
(enum member) HttpStatus.Ok = 200
```
````
* **Hover en `Result.Ok("datos")`**:
````markdown
```varn
Result.Ok(value: str): Result<str, E>
```
*(payload constructor de variante de `Result<T, E>`)*
````
* **Hover en `Result.Pending`**:
````markdown
```varn
(enum member) Result.Pending: Result<T, E>
```
*(variante sin payload)*
````

---

## 9. Flujo de Control Avanzado y Pattern Matching

### 9.1. Expresión `match` Exhaustiva

```varn
function evaluate(res: Result<int, str>): str {
    return match (res) {
        Result.Ok(val) if val > 100 => `Alto valor: ${val}`,
        Result.Ok(val) => `Valor normal: ${val}`,
        Result.Err("timeout") => "Petición expirada",
        Result.Err(msg) => `Error: ${msg}`,
        Result.Pending => "En progreso"
    };
}
```

#### Resaltado y Hovers en `match`:
* `match`: `KEYWORD` (`#C586C0`).
* `Result.Ok(val)`: `Result` (`TYPE`), `Ok` (`ENUM_MEMBER`), `val` (`VARIABLE` / `PARAMETER`).
* **Hover en `val` en `Result.Ok(val)`**:
````markdown
```varn
(pattern binding) val: int
```
````
* **Hover en `msg` en `Result.Err(msg)`**:
````markdown
```varn
(pattern binding) msg: str
```
````
* **Hover en la palabra clave `match`**:
````markdown
```varn
match (subject: Result<int, str>): str
```
*Expresión condicional exhaustiva evaluada en tiempo de ejecución.*
````

---

### 9.2. Control-Flow Type Narrowing

El checker estrecha automáticamente los tipos en ramas condicionales:

```varn
function printLength(item: str | Array<int> | null) {
    // 1. Antes del check: item es str | Array<int> | null
    if (item == null) {
        // 2. Aquí item es estrictamente null
        return;
    }
    // 3. Aquí item es str | Array<int>
    if (item is str) {
        // 4. Aquí item es estrictamente str
        println(item.length);
    } else {
        // 5. Aquí item es estrictamente Array<int>
        println(item.length);
    }
}
```

* **Hover en paso (1)**: `let item: str | Array<int> | null`
* **Hover en paso (2)**: `let item: null`
* **Hover en paso (3)**: `let item: str | Array<int>`
* **Hover en paso (4)**: `let item: str` *(narrowed by `is str`)*
* **Hover en paso (5)**: `let item: Array<int>` *(narrowed by exclusion)*

---

## 10. Concurrencia, Runtime Asíncrono e Isolates

Varn implementa un modelo de actores/isolates sin memoria compartida, con canales tipados y futures de coste cero:

```varn
import { spawnIsolate, Channel } from "std:task"

function workerThread(chan: Channel<int>) {
    let msg = chan.receive();
    println(`Recibido: ${msg}`);
}

async function main() {
    let chan = new Channel<int>();
    let handle = spawnIsolate(workerThread, chan);
    await chan.send(42);
    await handle.join();
}
```

| Elemento | Resaltado | Hover Esperado |
| :--- | :--- | :--- |
| `spawnIsolate` | `FUNCTION` (`#DCDCAA`) | ````varn\nfunction spawnIsolate<T>(entry: fn(arg: T): void, arg: T): IsolateHandle\n(from std:task)\n````\n*Lanza un nuevo Isolate del runtime con heap independiente y GC propio.* |
| `Channel<int>` | `Channel` (`CLASS`), `int` (`TYPE`) | ````varn\nclass Channel<T>\n(from std:task)\n````\n*Canal MPSC/SPMC tipado para paso de mensajes entre isolates.* |
| `chan.receive()` | `receive` (`FUNCTION` + `ASYNC`) | ````varn\nasync function Channel.receive(): Task<int>\n```` |
| `await` | `KEYWORD` (`#C586C0`) | ````varn\nawait Task<T>: T\n````\n*Suspende la ejecución hasta la resolución del future sin bloquear el hilo del scheduler.* |

---

## 11. Operadores Especiales, Pipeline y Sintaxis Expresiva

### 11.1. Pipeline Operator (`|>`)

```varn
let result = "  varn language  "
    |> trim
    |> toUpper
    |> (s => `[${s}]`);
```

* **Resaltado de `|>`**: `KEYWORD` (`#D4D4D4` o `#C586C0`).
* **Inlay Hints**: Al final de cada línea se muestra el tipo inferido:
  - `|> trim` ➔ `: str`
  - `|> toUpper` ➔ `: str`
  - `|> (s => ...)` ➔ `: str`
* **Hover sobre `|>`**:
````markdown
```varn
operator |> (receiver: T, transform: fn(arg: T): R): R
```
*Operador de encadenamiento secuencial hacia adelante.*
````

---

### 11.2. Null Coalescing (`??`), Coalescing Assignment (`??=`) y Optional Chaining (`?.`)

| Expresión | Resaltado | Hover Esperado |
| :--- | :--- | :--- |
| `let port = cfg.port ?? 8080` | `??` (`KEYWORD` / Operador) | *Tipo inferido de `port`: `int` (elimina el componente `null` de `cfg.port`).* |
| `config.timeout ??= 5000` | `??=` (`KEYWORD` / Operador) | *Asigna `5000` sólo si `config.timeout` evalúa a `null`.* |
| `let zip = user?.address?.zipCode`| `?.` (`KEYWORD` / Operador) | *Tipo inferido de `zip`: `str?` (propaga `null` si cualquier elemento previo es `null`).* |
| `let item = arr?.[0]` | `?.[]` (`KEYWORD` / Operador) | *Indexación segura; retorna `null` si `arr` es `null` o fuera de límites.* |
| `let res = callback?.()` | `?.()` (`KEYWORD` / Operador) | *Invocación segura; sólo llama si `callback` no es `null`.* |

---

## 12. Decoradores, Metadatos y Capacidades de Frontera Host

### 12.1. Decoradores Builtin

| Decorador | Aplicable a | Resaltado | Hover Esperado |
| :--- | :--- | :--- | :--- |
| `@inline` | Funciones, métodos | `#DCDCAA` (Oro) | ````varn\n@inline\n````\n*Instruye al compilador y JIT a expandir el cuerpo de la función en el sitio de llamada.* |
| `@deprecated("Use fetchV2()")`| Funciones, clases, campos | `#DCDCAA` (Oro) + Tachado | ````varn\n@deprecated(reason: "Use fetchV2()")\n````\n*Marca el símbolo como obsoleto. Emitirá diagnósticos de advertencia.* |
| `@test("Caso 1")` | Funciones | `#DCDCAA` (Oro) | ````varn\n@test(name: str = "")\n````\n*Registra la función como unidad de prueba para el test-runner `vn test`.* |
| `@pure` | Funciones | `#DCDCAA` (Oro) | ````varn\n@pure\n````\n*Garantiza ausencia de efectos secundarios; habilita optimizaciones de DCE y CSE agresivas.* |
| `@capability("fs:read")` | Módulos, funciones | `#DCDCAA` (Oro) | ````varn\n@capability(domain: "fs:read")\n````\n*Verifica que el isolate solicitante posea el permiso de acceso a la frontera del sistema host.* |

---

## 13. Biblioteca Estándar (`std:*`) y Módulos

### 13.1. Catálogo Completo de Módulos Oficiales

| Módulo | Resaltado | Hover de Módulo (`import ... from "std:*"`) |
| :--- | :--- | :--- |
| `std:http` | `STRING` (`#CE9178`) | ````varn\nmodule "std:http"\n````\n*Cliente HTTP estándar Web (`fetch`, `Request`, `Response`, `Headers`) y Servidor HTTP nativo de alto rendimiento.* |
| `std:fs` | `STRING` (`#CE9178`) | ````varn\nmodule "std:fs"\n````\n*Sistema de archivos (lectura/escritura síncrona y asíncrona con buffers y streams).* |
| `std:io` | `STRING` (`#CE9178`) | ````varn\nmodule "std:io"\n````\n*Flujos estándar de E/S (`stdin`, `stdout`, `stderr`) y utilidades de formateo.* |
| `std:task` | `STRING` (`#CE9178`) | ````varn\nmodule "std:task"\n````\n*Primitivas de concurrencia, `TaskGroup`, `parallel`, `spawnIsolate` y canales tipados.* |
| `std:crypto`| `STRING` (`#CE9178`) | ````varn\nmodule "std:crypto"\n````\n*Hashing criptográfico (SHA256, SHA512, BLAKE3), HMAC y generación aleatoria segura.* |
| `std:time` | `STRING` (`#CE9178`) | ````varn\nmodule "std:time"\n````\n*Temporizadores de precisión, `Duration`, `Instant`, `DateTime` y formateo ISO-8601.* |
| `std:json` | `STRING` (`#CE9178`) | ````varn\nmodule "std:json"\n````\n*Parsing y serialización JSON con optimización SIMD.* |
| `std:reflect`| `STRING` (`#CE9178`) | ````varn\nmodule "std:reflect"\n````\n*Introspección en tiempo de ejecución, metadatos y análisis de tipos.* |
| `std:sys` | `STRING` (`#CE9178`) | ````varn\nmodule "std:sys"\n````\n*Información del sistema operativo, variables de entorno, CPU cores y uso de memoria.* |
| `std:math` | `STRING` (`#CE9178`) | ````varn\nmodule "std:math"\n````\n*Funciones trigonométricas, constantes matemáticas (`PI`, `E`), logaritmos y raíces.* |
| `std:testing`| `STRING` (`#CE9178`) | ````varn\nmodule "std:testing"\n````\n*Framework de pruebas unitarias, aserciones (`assert`, `assertEquals`) y benchmarking.* |
| `std:result`| `STRING` (`#CE9178`) | ````varn\nmodule "std:result"\n````\n*Tipos algebraicos de manejo seguro de errores `Option<T>` y `Result<T, E>`.* |
| `std:collections`| `STRING` (`#CE9178`) | ````varn\nmodule "std:collections"\n````\n*Estructuras avanzadas de datos (`Deque`, `PriorityQueue`, `LinkedList`, `Trie`).* |
| `std:path` | `STRING` (`#CE9178`) | ````varn\nmodule "std:path"\n````\n*Manipulación de rutas multiplataforma (Windows y POSIX).* |
| `std:env` | `STRING` (`#CE9178`) | ````varn\nmodule "std:env"\n````\n*Acceso y mutación segura de variables de entorno y argumentos del proceso.* |
| `std:process`| `STRING` (`#CE9178`) | ````varn\nmodule "std:process"\n````\n*Lanzamiento de subprocesos, pipes y control de señales.* |
| `std:sync` | `STRING` (`#CE9178`) | ````varn\nmodule "std:sync"\n````\n*Mutexes, semáforos, barreras de sincronización y variables atómicas.* |
| `std:regex` | `STRING` (`#CE9178`) | ````varn\nmodule "std:regex"\n````\n*Motor de expresiones regulares compiladas.* |

---

## 14. Reglas de Desambiguación y Matriz Maestra de Contraste Visual

### 14.1. Reglas de Desambiguación Estricta

1. **Palabras Clave vs. Nombres de Miembros / Propiedades**:
   - `obj.type`, `obj.get()`, `record.default`, `config.class`:
   - El tokenizador léxico puede emitirlos inicialmente como `TokenKind::Type`, pero el resolvedor semántico del LSP (`classify.rs`) verifica el token anterior (`.` o `?.`). Si es acceso de miembro, se reclasifica obligatoriamente a `PROPERTY` o `FUNCTION`.
   - **Hover**: Muestra la firma de la propiedad o método, jamás la palabra reservada.

2. **Tipos Primitivos vs. Clases Definidas por el Usuario**:
   - `int`, `str`, `bool`: Se clasifican como `TYPE` (con documentación del primitivo).
   - `User`, `Server`: Se clasifican como `CLASS` (con firma de la clase y lista de miembros públicos).

3. **Sombreado Léxico (*Lexical Shadowing*)**:
   - Si una variable local `let count = 10` se declara dentro de una función donde ya existe un parámetro global o de closure llamado `count`, el hover dentro del bloque interior refleja exclusivamente la variable local más cercana.

4. **Símbolos Marcados con `@deprecated`**:
   - Reciben el modificador semántico `DEPRECATED` (renderizado con línea de tachado en el editor).
   - El hover incluye un bloque de alerta en Markdown `> ⚠️ **Deprecated**: [motivo]`.

---

### 14.2. Matriz Maestra de Contraste Rápido (Golden Master)

| # | Elemento Sintáctico / Semántico | Código de Muestra | Semantic Token (`Type`, `Modifiers`) | Color Visual Estándar | Hover Markdown Esperado |
|---|:---|:---|:---|:---|:---|
| 1 | **Palabra clave de control** | `if (condition) {` | `KEYWORD` | `#C586C0` (Magenta) | *(Sin hover)* |
| 2 | **Palabra clave de tipo** | `type UserId = int` | `KEYWORD` (`type`), `TYPE` (`UserId`, `int`)| `#569CD6` / `#4EC9B0` | ````varn\ntype UserId = int\n```` |
| 3 | **Entero Literal** | `let a = 42` | `NUMBER` (`42`) | `#B5CEA8` (Verde Salvia)| ````varn\n42: int\n```` |
| 4 | **Float Literal** | `let f = 3.14` | `NUMBER` (`3.14`) | `#B5CEA8` (Verde Salvia)| ````varn\n3.14: float\n```` |
| 5 | **Decimal Literal** | `let d = 19.99m` | `NUMBER` (`19.99m`) | `#B5CEA8` (Verde Salvia)| ````varn\n19.99m: decimal\n```` |
| 6 | **BigInt Literal** | `let b = 9999999999999999n` | `NUMBER` | `#B5CEA8` (Verde Salvia)| ````varn\n9999999999999999n: bigint\n```` |
| 7 | **Carácter Literal** | `let c = 'x'` | `STRING` (`'x'`) | `#CE9178` (Naranja) | ````varn\n'x': char\n```` |
| 8 | **String Literal** | `let s = "Varn"` | `STRING` (`"Varn"`) | `#CE9178` (Naranja) | ````varn\n"Varn": str\n```` |
| 9 | **Constante Inmutable** | `const MAX = 100` | `VARIABLE` + `READONLY` + `DECL` | `#4FC1FF` (Azul Eléctrico) | ````varn\nconst MAX: int = 100\n```` |
| 10 | **Variable Mutable** | `let mutVar = 10` | `VARIABLE` + `DECL` | `#9CDCFE` (Celeste) | ````varn\nlet mutVar: int\n```` |
| 11 | **Parámetro de Función** | `function f(x: int)` | `PARAMETER` (`x`) | `#9CDCFE` (Celeste) | ````varn\n(parameter) x: int\n```` |
| 12 | **Función Libre** | `function add(a: int): int`| `FUNCTION` + `DECL` (`add`) | `#DCDCAA` (Amarillo Oro) | ````varn\nfunction add(a: int): int\n```` |
| 13 | **Función Asíncrona** | `async function get(): Task<int>`| `FUNCTION` + `ASYNC` (`get`) | `#DCDCAA` (Amarillo Oro) | ````varn\nasync function get(): Task<int>\n```` |
| 14 | **Generador** | `function* g(): Generator<int>`| `FUNCTION` (`g`) | `#DCDCAA` (Amarillo Oro) | ````varn\nfunction* g(): Generator<int>\n```` |
| 15 | **Clase** | `class Server {` | `CLASS` + `DECL` (`Server`) | `#4EC9B0` (Verde Menta) | ````varn\nclass Server\n```` |
| 16 | **Struct** | `struct Point { x: int }` | `CLASS` + `DECL` (`Point`) | `#4EC9B0` (Verde Menta) | ````varn\nstruct Point\n```` |
| 17 | **Interfaz** | `interface Writer {` | `INTERFACE` + `DECL` (`Writer`) | `#B8D7A3` (Verde Claro) | ````varn\ninterface Writer\n```` |
| 18 | **Extension** | `extension Ext on str {` | `CLASS` (`Ext`) | `#4EC9B0` (Verde Menta) | ````varn\nextension Ext on str\n```` |
| 19 | **Método de Clase** | `s.listen(8080)` | `FUNCTION` (`listen`) | `#DCDCAA` (Amarillo Oro) | ````varn\n(method) Server.listen(port: int): void\n```` |
| 20 | **Método Estático** | `Math.sqrt(16)` | `FUNCTION` + `STATIC` (`sqrt`) | `#DCDCAA` (Amarillo Oro) | ````varn\n(static method) Math.sqrt(x: float): float\n```` |
| 21 | **Propiedad / Campo** | `user.name` | `PROPERTY` (`name`) | `#9CDCFE` (Celeste) | ````varn\n(property) User.name: str\n```` |
| 22 | **Enum** | `enum Status { Ok, Err }` | `TYPE` (`Status`) | `#4EC9B0` (Verde Turquesa)| ````varn\nenum Status\n```` |
| 23 | **Miembro de Enum** | `Status.Ok` | `ENUM_MEMBER` (`Ok`) | `#4FC1FF` (Azul Cobalto) | ````varn\n(enum member) Status.Ok = 0\n```` |
| 24 | **Variante ADT** | `Option.Some(42)` | `ENUM_MEMBER` (`Some`) | `#4FC1FF` (Azul Cobalto) | ````varn\nOption.Some(value: int): Option<int>\n```` |
| 25 | **Tupla** | `let t = (1, "a")` | `VARIABLE` (`t`) | `#9CDCFE` (Celeste) | ````varn\nlet t: (int, str)\n```` |
| 26 | **Record** | `let r = { id: 1 }` | `PROPERTY` (`id`) | `#9CDCFE` (Celeste) | ````varn\nlet r: { id: int }\n```` |
| 27 | **Type Parameter** | `class List<T> {` | `TYPE_PARAMETER` (`T`) | `#B8D7A3` (Verde Olivo) | ````varn\ntype T\n````\n*(generic type parameter)* |
| 28 | **Decorador** | `@inline` | `FUNCTION` (`inline`) | `#DCDCAA` (Oro Brillante)| ````varn\n@inline\n```` |
| 29 | **Operador Pipeline** | `val \|> f1 \|> f2` | `KEYWORD` (`\|>`) | `#D4D4D4` (Blanco Humo) | *(Inlay hint con tipo intermedio de retorno)* |
| 30 | **Palabra clave `this`**| `return this.id` | `VARIABLE` (`this`) | `#9CDCFE` (Celeste) | ````varn\nthis: CurrentClassName\n```` |
| 31 | **Módulo de Stdlib** | `from "std:http"` | `STRING` (`"std:http"`) | `#CE9178` (Naranja) | ````varn\nmodule "std:http"\n````\n*Cliente y Servidor HTTP nativos.* |
| 32 | **Símbolo de Stdlib** | `import { File }` | `CLASS` + `DEFAULT_LIBRARY` | `#4EC9B0` (Verde Menta) | ````varn\nclass File\n(from std:fs)\n```` |
| 33 | **Símbolo Deprecado** | `oldFunction()` | `FUNCTION` + `DEPRECATED` | Tachado | ````varn\nfunction oldFunction(): void\n````\n> ⚠️ **Deprecated** |
| 34 | **Tipo Nullable Narrowed**| `if (u != null) { u.` | `CLASS` (`User`) | `#4EC9B0` (Verde Menta) | ````varn\nlet u: User\n````\n*(narrowed: null eliminado)* |
| 35 | **Match Pattern Binding**| `case Ok(msg) =>` | `VARIABLE` (`msg`) | `#9CDCFE` (Celeste) | ````varn\n(pattern binding) msg: str\n```` |
