# Varn Language — Identificadores y Palabras Reservadas

> Fuentes: todos los archivos `tests/*.vn`.

---

## 1. Reglas de Identificadores

- Deben comenzar con una **letra Unicode** o **guión bajo** (`_`).
- Pueden contener letras, dígitos y `_`.
- **No** pueden comenzar con un dígito.
- El identificador `_` solitario se usa como **marcador de descarte** en destructuring y placeholders de pipeline.
- Son sensibles a mayúsculas: `Foo` ≠ `foo`.

---

## 2. Convenciones de Nomenclatura

> NO-VERIFICADO como regla: solo estilo observado en `tests/*.vn`.

| Ámbito | Convención | Ejemplo |
|--------|-----------|---------|
| Variables y funciones | `camelCase` | `makeCounter`, `forOfSum` |
| Clases, interfaces, enums, tipos | `PascalCase` | `Animal`, `Direction`, `TokenKind` |
| Constantes de módulo | `UPPER_SNAKE_CASE` o `camelCase` | `str.EMPTY`, `tval` |
| Parámetros | `camelCase` | `firstName`, `maxRetries` |
| Campos privados | `_camelCase` | `_celsius`, `_next` |

---

## 3. Palabras Reservadas

Lista exacta del lexer (`crates/varn-lexer/src/keywords.rs:12-78`;
tokens en `crates/varn-core/src/token/kind.rs:85-155`):

```
let  const  var  function  class  struct  interface  type  enum
namespace  module  extension  on
if  else  switch  case  default  while  for  do  break  continue
return  throw  try  catch  finally  using  with
import  export  from  as
async  await  yield
new  this  super  delete  typeof  instanceof  in  of  void  is
true  false  null
public  private  protected  static  abstract  override  readonly
declare  native  extends  implements  get  set  constructor  destructor
match
```

`_` es token `Placeholder`, no identificador. NO son keywords:
`record`, `spawn`, `parallel` (imports de `std:task`), `dynamic` (nombre
de tipo contextual).

---

## 4. Identificadores Predefinidos (no reservados)

Estos identificadores tienen significado especial pero **no** son palabras reservadas (pueden usarse como nombres en otros contextos, aunque no se recomienda):

| Identificador | Descripción |
|--------------|-------------|
| `str` | Tipo primitivo de cadena |
| `int` | Tipo primitivo entero |
| `float` | Tipo primitivo flotante |
| `bool` | Tipo primitivo booleano |
| `char` | Tipo primitivo carácter |
| `decimal` | Tipo numérico de precisión exacta |
| `bigint` | Tipo entero de precisión arbitraria |
| `void` | Tipo de retorno sin valor |
| `null` | Valor nulo (sí es reservado) |
| `assert` | Función de testing integrada |
| `print` | Función de salida integrada |
| `Range` | Clase integrada de rangos |
| `Array` | Alias de tipo array |
| `Map` | Colección de clave-valor |
| `Set` | Colección de valores únicos |
| `Error` | Clase base de errores |
| `TypeError` | Error de tipo |
| `RangeError` | Error de rango |

---

## 5. Marcador de Descarte (`_`)

En destructuring y en el operador pipeline, `_` tiene significado especial:

```varn
// Descarte en destructuring de array
const [skip1, _, skip3] = [100, 200, 300]
assertTrue("arr destr skip", skip1 === 100 && skip3 === 300)

// Placeholder en pipeline
assertTrue("pipe placeholder", 7 |> addN(_, 3) === 10)
assertTrue("pipe multi _",     4 |> addN(_, _) === 8)
```

---

## 6. Nombres de Tipos en `typeof`

Verificado en tests: `"str"`, `"int"`, `"float"`, `"class"`
(`tests/79-control-flow-narrowing.vn`,
`tests/122-numeric-conversions.vn`, `tests/111-network-apis-overhaul.vn`).
Nombres canónicos: `"str"` nunca `"string"`, `"int"` nunca `"integer"`.
Otros valores (`"bool"`, `"char"`, `"null"`, `"object"`, `"array"`,
`"function"`): NO-VERIFICADOS.

