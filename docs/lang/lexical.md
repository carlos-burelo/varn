# Varn Language — Lexical Syntax

> Fuentes: `tests/01-arithmetic.vn`, `tests/02-boolean.vn`, `tests/03-strings.vn`, `tests/04-templates.vn`, `tests/34-char-type.vn`, `tests/35-decimal-bigint.vn`, `tests/89-raw-strings-and-with-expressions.vn`.

---

## 1. Identificadores

Los identificadores siguen la convención *camelCase* (variables, funciones) y *PascalCase* (clases, interfaces, enums). Reglas:

- Deben comenzar con letra o guión bajo (`_`).
- Pueden contener letras Unicode, dígitos y `_`.
- El guión bajo solitario (`_`) actúa como identificador de descarte en destructuring.

```varn
let count = 0
const _ignored = 42
const [head, _] = [1, 2]
```

---

## 2. Palabras Reservadas

```
let  const  var  function  async  await  yield
class  interface  enum  extension  record  type  abstract  override
if  else  match  switch  for  while  do  break  continue  return
try  catch  finally  throw  new  this  super  instanceof  typeof  is  in  of
true  false  null
import  export  from
using  isIsolate  dynamic
with
```

---

## 3. Comentarios

```varn
// Comentario de una sola línea

/* Comentario
   multilínea */
```

---

## 4. Literales

### 4.1 Literales enteros (`int`)

```varn
assert("int add",      1 + 2 === 3)       // tests/01-arithmetic.vn:1
assert("int power",    2 ** 10 === 1024)  // tests/01-arithmetic.vn:7
assert("bitwise and",  (12 & 10) === 8)   // tests/01-arithmetic.vn:14
assert("left shift",   1 << 4 === 16)     // tests/01-arithmetic.vn:18
```

Soporta notación decimal. Las operaciones son: `+`, `-`, `*`, `/`, `%`, `**` (potencia), `&`, `|`, `^`, `~`, `<<`, `>>`.

### 4.2 Literales flotantes (`float`)

```varn
assert("float add",  0.1 + 0.2 > 0.29 && 0.1 + 0.2 < 0.31)  // tests/01-arithmetic.vn:8
assert("float div",  1.0 / 3.0 > 0.333 && 1.0 / 3.0 < 0.334)  // tests/01-arithmetic.vn:9
```

Require punto decimal. La división `int / int` produce `float`.

### 4.3 Literales `decimal`

Sufijo `d`. Precisión exacta para cálculos financieros.

```varn
const d1: decimal = 1.5d    // tests/35-decimal-bigint.vn:3
const d2: decimal = 2.5d
assert("decimal add", d1 + d2 === 4.0d)   // tests/35-decimal-bigint.vn:5
assert("decimal toFixed 2", (3.14159d).toFixed(2) === "3.14")  // tests/35-decimal-bigint.vn:17
```

### 4.4 Literales `bigint`

Sufijo `n`. Enteros de precisión arbitraria.

```varn
const bi1: bigint = 100n     // tests/35-decimal-bigint.vn:47
assert("bigint toString", bi1.toString() === "100")
assert("bigint toInt",    bi1.toInt() === 100)
assert("bigint lt",       1n < 100n)
```

### 4.5 Literales booleanos (`bool`)

```varn
assert("and short-circuit", (false && (1/0 === 0)) === false)  // tests/02-boolean.vn:1
assert("or short-circuit",  true  || (1/0 === 0) === true)    // tests/02-boolean.vn:2
assert("not false",         !false === true)                   // tests/02-boolean.vn:4
assert("triple eq null",    null === null)                     // tests/02-boolean.vn:10
```

### 4.6 Literales de cadena (`str`)

Entre comillas dobles. Admiten secuencias de escape estándar: `\n`, `\t`, `\r`, `\\`, `\"`.

```varn
const s = "  Hello, World!  "
assert("trim",       s.trim() === "Hello, World!")   // tests/03-strings.vn:3
assert("slice",      "hello world".slice(6, 11) === "world")
assert("length",     "hello".length === 5)
```

### 4.7 Raw String Literals (`"""…"""`)

Cadenas delimitadas por triple comilla. No procesan secuencias de escape; el contenido se preserva tal cual.

```varn
let jsonStr = """{
  "name": "Varn",
  "path": "C:\Program Files\Varn",
  "escapes": "\n \t \r"
}""";
// tests/89-raw-strings-and-with-expressions.vn:11-15

let text = """Hello "world" and 'quotes' without escapes!""";
// tests/89-raw-strings-and-with-expressions.vn:23
```

- Pueden ser multilínea sin barras invertidas adicionales.
- Permiten comillas dobles internas sin escape.
- El tipo resultado es `str`.

### 4.8 Template Literals (`` ` `` `` ` ``)

```varn
const tval = 42
assert("simple template",   `answer = ${tval}` === "answer = 42")   // tests/04-templates.vn:3
assert("expr in template",  `${tval * 2}` === "84")                  // tests/04-templates.vn:4
assert("nested template",   `prefix_${"mid"}_suffix` === "prefix_mid_suffix")
assert("template + concat", `a${1 + 1}b` + "c" === "a2bc")
```

Las interpolaciones `${expr}` admiten cualquier expresión.

### 4.9 Literales de carácter (`char`)

Entre comillas simples. Tipo primitivo distinto de `str`.

```varn
const a: char = 'a'             // tests/34-char-type.vn:3
assert("char code point", 'A'.charCodeAt() === 65)
assert("char from code",  char.fromCode(65) === 'A')
assert("char toString",   'x'.toString() === "x")
```

### 4.10 Literal `null`

```varn
const p3: Profile? = null    // tests/08-null-safety.vn:13
assert("triple eq null", null === null)
assert("triple neq null", null !== 0)
```

---

## 5. Operadores

### 5.1 Aritméticos
| Operador | Descripción |
|----------|-------------|
| `+` | Suma / concatenación de strings |
| `-` | Resta / negación unaria |
| `*` | Multiplicación |
| `/` | División (produce `float` cuando operandos son `int`) |
| `%` | Módulo |
| `**` | Potencia |

### 5.2 Bit a bit
`&`, `|`, `^`, `~`, `<<`, `>>`

### 5.3 Comparación
`===`, `!==`, `<`, `<=`, `>`, `>=`  
`==` realiza igualdad estructural profunda (tuplas, records).

### 5.4 Lógicos
`&&`, `||`, `!`  
Con cortocircuito: `&&` no evalúa el lado derecho si el izquierdo es falso; `||` no evalúa si el izquierdo es verdadero.

### 5.5 Null-coalescing
```varn
assert("?? left non-null", (42 ?? 0) === 42)     // tests/08-null-safety.vn:18
assert("?? left null",     (null ?? 99) === 99)   // tests/08-null-safety.vn:19
assert("?? chained",       (null ?? null ?? 7) === 7)
```

### 5.6 Null-coalescing Assignment (`??=`)
```varn
let a: int? = null
a ??= 42          // a es ahora 42  (tests/87-primary-constructors-nullish-assign.vn:75)
a ??= 999         // no cambia: a sigue siendo 42
```

Asigna el valor derecho únicamente si la variable (o propiedad, o índice) es `null`.

### 5.7 Optional Chaining (`?.`)
```varn
assert("?. non-null chain",  p1?.address?.city === "NY")   // tests/08-null-safety.vn:15
assert("?. missing nested",  p2?.address?.city === null)
assert("?. null root",       p3?.name === null)
```

### 5.8 Pipeline (`|>`)
```varn
assert("pipe simple",       5 |> double2 === 10)             // tests/19-pipeline.vn:10
assert("pipe placeholder",  7 |> addN(_, 3) === 10)          // tests/19-pipeline.vn:11
assert("pipe multi _",      4 |> addN(_, _) === 8)
const piped = (3 |> double2) |> double2
assert("pipe chained", piped === 12)
```

El `_` en el lado derecho actúa como marcador de posición del valor del lado izquierdo.

### 5.9 Ternary
```varn
function classify(n: int): str {
    return n < 0 ? "neg" : n === 0 ? "zero" : "pos"  // tests/09-control-flow.vn:3
}
```

### 5.10 Spread (`...`)
```varn
const spread = [...[1, 2], ...[3, 4]]          // tests/05-arrays.vn:62
const ext    = { ...base, z: 3 }               // tests/06-destructuring.vn:33
```

### 5.11 Instanceof y typeof
```varn
assert("instanceof PoliceDog", pd instanceof PoliceDog)  // tests/12-classes.vn:55
assert("processTypeof string", typeof "abc" === "str")   // tests/79-control-flow-narrowing.vn:47
```

### 5.12 Type Assertion (`is`)
```varn
function describeChar(c: char): str {
    if (c is char) return "yes"   // tests/34-char-type.vn:38
    return "no"
}
```

---

## 6. Secuencias de Escape en cadenas `str`

| Secuencia | Carácter |
|-----------|---------|
| `\n` | Nueva línea |
| `\t` | Tabulador |
| `\r` | Retorno de carro |
| `\\` | Barra invertida |
| `\"` | Comilla doble |
| `\'` | Comilla simple |

En **Raw Strings** (`"""…"""`), ninguna de estas secuencias se procesa; el texto es literal.
