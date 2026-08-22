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

| Ámbito | Convención | Ejemplo |
|--------|-----------|---------|
| Variables y funciones | `camelCase` | `makeCounter`, `forOfSum` |
| Clases, interfaces, enums, tipos | `PascalCase` | `Animal`, `Direction`, `TokenKind` |
| Constantes de módulo | `UPPER_SNAKE_CASE` o `camelCase` | `str.EMPTY`, `tval` |
| Parámetros | `camelCase` | `firstName`, `maxRetries` |
| Campos privados | `_camelCase` | `_celsius`, `_next` |

---

## 3. Palabras Reservadas

Las siguientes palabras son reservadas y **no pueden** usarse como identificadores:

### Declaraciones
```
let  const  var  function  async  class  interface  enum
extension  type  abstract  record  export  import  from
```

### Control de Flujo
```
if  else  match  switch  for  while  do  break  continue
return  throw  try  catch  finally  using
```

### Operadores y Expresiones
```
new  this  super  null  true  false
instanceof  typeof  is  in  of  with  override  static
```

### Concurrencia y Runtime
```
await  yield  isIsolate  dynamic
```

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
assert("arr destr skip", skip1 === 100 && skip3 === 300)

// Placeholder en pipeline
assert("pipe placeholder", 7 |> addN(_, 3) === 10)
assert("pipe multi _",     4 |> addN(_, _) === 8)
```

---

## 6. Nombres de Tipos en `typeof`

El operador `typeof` devuelve uno de estos strings canónicos:

```varn
typeof "abc"          // "str"
typeof 42             // "int"
typeof 3.14           // "float"
typeof true           // "bool"
typeof 'a'            // "char"
typeof null           // "null"
typeof MyClass        // "class"
typeof someObject     // "object"
typeof someArray      // "array"
typeof someFunction   // "function"
```

> [!IMPORTANT]
> Los nombres de tipo retornados por `typeof` usan los nombres canónicos de Varn: `"str"` nunca `"string"`, `"int"` nunca `"integer"`, `"bool"` nunca `"boolean"`.
