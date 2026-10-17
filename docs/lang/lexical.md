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

Lista exacta del lexer (`crates/varn-lexer/src/keywords.rs:12-78`):

```
let  const  var  function  class  struct  interface  type  enum
namespace  module  extension  on  if  else  switch  case  default
while  for  do  break  continue  return  throw  try  catch  finally
using  with  import  export  from  as  async  await  yield
new  this  super  delete  typeof  instanceof  in  of  void  is
true  false  null  public  private  protected  static  abstract
override  readonly  declare  native  extends  implements
get  set  constructor  destructor  match
```

NO son keywords: `record`, `spawn`, `parallel`, `dynamic`, `isIsolate`.

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
assertTrue("int add",      1 + 2 === 3)       // tests/01-arithmetic.vn
assertTrue("int power",    2 ** 10 === 1024)  // tests/01-arithmetic.vn
assertTrue("bitwise and",  (12 & 10) === 8)   // tests/01-arithmetic.vn
assertTrue("left shift",   1 << 4 === 16)     // tests/01-arithmetic.vn
```

Soporta notación decimal. Las operaciones son: `+`, `-`, `*`, `/`, `%`, `**` (potencia), `&`, `|`, `^`, `~`, `<<`, `>>`.

### 4.2 Literales flotantes (`float`)

```varn
assertTrue("float add",  0.1 + 0.2 > 0.29 && 0.1 + 0.2 < 0.31)  // tests/01-arithmetic.vn
assertTrue("float div",  1.0 / 3.0 > 0.333 && 1.0 / 3.0 < 0.334)  // tests/01-arithmetic.vn
```

Requiere punto decimal. `int / int` es `int` truncado
(`tests/120-int-division.vn`); la división `float` usa operandos `float`
(`tests/120-int-division.vn`).

### 4.3 Literales `decimal`

Sufijo `d`. Precisión exacta para cálculos financieros.

```varn
const d1: decimal = 1.5d    // tests/35-decimal-bigint.vn
const d2: decimal = 2.5d
assertTrue("decimal add", d1 + d2 === 4.0d)   // tests/35-decimal-bigint.vn
assertTrue("decimal toFixed 2", (3.14159d).toFixed(2) === "3.14")  // tests/35-decimal-bigint.vn
```

### 4.4 Literales `bigint`

Sufijo `n`. Enteros de precisión arbitraria.

```varn
const bi1: bigint = 100n     // tests/35-decimal-bigint.vn
assertTrue("bigint toString", bi1.toString() === "100")
assertTrue("bigint toInt",    bi1.toInt() === 100)
assertTrue("bigint lt",       1n < 100n)
```

### 4.5 Literales booleanos (`bool`)

```varn
assertTrue("and short-circuit", (false && (1/0 === 0)) === false)  // tests/02-boolean.vn
assertTrue("or short-circuit",  true  || (1/0 === 0) === true)    // tests/02-boolean.vn
assertTrue("not false",         !false === true)                   // tests/02-boolean.vn
assertTrue("triple eq null",    null === null)                     // tests/02-boolean.vn
```

### 4.6 Literales de cadena (`str`)

Entre comillas dobles. Admiten secuencias de escape estándar: `\n`, `\t`, `\r`, `\\`, `\"`.

```varn
const s = "  Hello, World!  "
assertTrue("trim",       s.trim() === "Hello, World!")   // tests/03-strings.vn
assertTrue("slice",      "hello world".slice(6, 11) === "world")
assertTrue("length",     "hello".length === 5)
```

### 4.7 Raw String Literals (`"""…"""`)

Cadenas delimitadas por triple comilla. No procesan secuencias de escape; el contenido se preserva tal cual.

```varn
let jsonStr = """{
  "name": "Varn",
  "path": "C:\Program Files\Varn",
  "escapes": "\n \t \r"
}""";
// tests/89-raw-strings-and-with-expressions.vn

let text = """Hello "world" and 'quotes' without escapes!""";
// tests/89-raw-strings-and-with-expressions.vn
```

- Pueden ser multilínea sin barras invertidas adicionales.
- Permiten comillas dobles internas sin escape.
- El tipo resultado es `str`.

### 4.8 Template Literals (`` ` `` `` ` ``)

```varn
const tval = 42
assertTrue("simple template",   `answer = ${tval}` === "answer = 42")   // tests/04-templates.vn
assertTrue("expr in template",  `${tval * 2}` === "84")                  // tests/04-templates.vn
assertTrue("nested template",   `prefix_${"mid"}_suffix` === "prefix_mid_suffix")
assertTrue("template + concat", `a${1 + 1}b` + "c" === "a2bc")
```

Las interpolaciones `${expr}` admiten cualquier expresión.

### 4.9 Literales de carácter (`char`)

Entre comillas simples. Tipo primitivo distinto de `str`.

```varn
const a: char = 'a'             // tests/34-char-type.vn
assertTrue("char code point", 'A'.charCodeAt() === 65)
assertTrue("char from code",  char.fromCode(65) === 'A')
assertTrue("char toString",   'x'.toString() === "x")
```

### 4.10 Literal `null`

```varn
const p3: Profile? = null    // tests/08-null-safety.vn
assertTrue("triple eq null", null === null)
```

---

## 5. Operadores

### 5.1 Aritméticos
| Operador | Descripción |
|----------|-------------|
| `+` | Suma / concatenación de strings |
| `-` | Resta / negación unaria |
| `*` | Multiplicación |
| `/` | División: `int / int` es `int` truncado hacia cero (`tests/120-int-division.vn`); `float / float` es `float` |
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
const maybe: int? = 42
assertTrue("?? left non-null", (maybe ?? 0) === 42)  // `??` sobre un no-nulo es error (VN3028)
assertTrue("?? left null",     (null ?? 99) === 99)   // tests/08-null-safety.vn
assertTrue("?? chained",       (null ?? null ?? 7) === 7)
```

### 5.6 Null-coalescing Assignment (`??=`)
```varn
let a: int? = null
a ??= 42          // a es ahora 42  (tests/87-primary-constructors-nullish-assign.vn)
a ??= 999         // no cambia: a sigue siendo 42
```

Asigna el valor derecho únicamente si la variable (o propiedad, o índice) es `null`.

### 5.7 Optional Chaining (`?.`)
```varn
assertTrue("?. non-null chain",  p1?.address?.city === "NY")   // tests/08-null-safety.vn
assertTrue("?. missing nested",  p2?.address?.city === null)
assertTrue("?. null root",       p3?.name === null)
```

### 5.8 Pipeline (`|>`)
```varn
assertTrue("pipe simple",       5 |> double2 === 10)             // tests/19-pipeline.vn
assertTrue("pipe placeholder",  7 |> addN(_, 3) === 10)          // tests/19-pipeline.vn
assertTrue("pipe multi _",      4 |> addN(_, _) === 8)
const piped = (3 |> double2) |> double2
assertTrue("pipe chained", piped === 12)
```

El `_` en el lado derecho actúa como marcador de posición del valor del lado izquierdo.

### 5.9 Ternary
```varn
function classify(n: int): str {
    return n < 0 ? "neg" : n === 0 ? "zero" : "pos"  // tests/09-control-flow.vn
}
```

### 5.10 Spread (`...`)
```varn
const spread = [...[1, 2], ...[3, 4]]          // tests/05-arrays.vn
const ext    = { ...base, z: 3 }               // tests/06-destructuring.vn
```

### 5.11 Instanceof y typeof
```varn
assertTrue("instanceof PoliceDog", pd instanceof PoliceDog)  // tests/12-classes.vn
assertTrue("processTypeof string", typeof "abc" === "str")   // tests/79-control-flow-narrowing.vn
```
// `typeof` verificado: `"str"`, `"int"`, `"float"`, `"class"` (ver `identifiers.md` §6).

### 5.12 Type Assertion (`is`)
```varn
function describeChar(c: char): str {
    if (c is char) return "yes"   // tests/34-char-type.vn
    return "no"
}
```

---

## 6. Secuencias de Escape en cadenas `str`

> NO-VERIFICADO como tabla: ningún test fija el conjunto exacto.

| Secuencia | Carácter |
|-----------|---------|
| `\n` | Nueva línea |
| `\t` | Tabulador |
| `\r` | Retorno de carro |
| `\\` | Barra invertida |
| `\"` | Comilla doble |
| `\'` | Comilla simple |

En **Raw Strings** (`"""…"""`), ninguna de estas secuencias se procesa; el texto es literal.

