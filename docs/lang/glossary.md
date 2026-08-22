# Varn Language — Glosario

Este glosario define los términos clave del lenguaje Varn. Las entradas están ordenadas alfabéticamente.

---

## A

**`abstract`**  
Modificador de clase o método. Una clase abstracta no puede instanciarse directamente; un método abstracto no tiene cuerpo y debe ser sobreescrito por subclases.

**`Array<T>` / `T[]`**  
Colección mutable y ordenada de elementos del tipo `T`. Las dos sintaxis son equivalentes e intercambiables.

**`as`**  
Operador de cast explícito (narrowing). Ejemplo: `(obj as dynamic).field`.

**`assert(label, condition)`**  
Función de testing. Lanza un error si `condition` es `false`.

**`async`**  
Modificador de función. Convierte la función en asíncrona; el valor de retorno se envuelve en una `Future`. Permite usar `await` dentro del cuerpo.

**`await`**  
Operador que pausa la ejecución de la función asíncrona actual hasta que la `Future` se resuelva. También válido a nivel de módulo (top-level await).

---

## B

**`bigint`**  
Tipo primitivo de entero con precisión arbitraria. Literal: `100n`. Sin overflow.

**`bool`**  
Tipo primitivo booleano. Valores: `true`, `false`.

**`break`**  
Sentencia que sale del bucle más cercano o del bloque `switch`.

---

## C

**`catch`**  
Bloque que captura errores lanzados en el bloque `try` correspondiente.

**`channel<T>(capacity)`**  
Función que crea un canal tipado `{ tx: Sender<T>, rx: Receiver<T> }` para comunicación entre tareas e isolates.

**`char`**  
Tipo primitivo de carácter Unicode. Literal: `'a'`. Distinto de `str`.

**`ChannelClosed`**  
Error lanzado al intentar enviar o recibir en un canal que ha sido cerrado.

**`class`**  
Declaración de tipo con campos, métodos, constructores y herencia. Soporta modificadores `public`, `private`, `abstract`, `override`, `static`.

**`const`**  
Declaración de variable inmutable (sinónimo de `const` y equivalente a JavaScript's `const`).

**`continue`**  
Salta a la siguiente iteración del bucle más cercano.

---

## D

**`decimal`**  
Tipo primitivo de número decimal con precisión exacta. Literal: `1.5d`. Para uso financiero.

**`dynamic`**  
Tipo que acepta cualquier valor en tiempo de ejecución. Desactiva el chequeo estático de tipos para ese valor.

---

## E

**`enum`**  
Declaración de tipo enumerado. Puede ser simple (sin payload), con payload (ADT), con campos compartidos, con constructor, con métodos y con implementación de interfaces.

**`export`**  
Marca una función o símbolo como accesible desde fuera del módulo. Requerido para usar con `spawnIsolate`.

**`extends`**  
Especifica herencia de una clase base. Solo herencia simple de clase.

**`extension`**  
Declara métodos adicionales en un tipo existente sin modificar su definición original.

---

## F

**`finally`**  
Bloque que se ejecuta siempre después de `try/catch`, independientemente de si hubo error o `return`.

**`float`**  
Tipo primitivo de número flotante de 64 bits (IEEE 754 doble precisión). Resultado de división `int/int`.

**`for`**  
Bucle de tres partes estilo C: `for (init; condition; step) { }`.

**`for...in`**  
Itera sobre las claves de un objeto.

**`for...of`**  
Itera sobre elementos de arrays, rangos, generadores e iterables.

**`for await...of`**  
Itera asincrónicamente sobre canales, async generators u otros iterables asíncronos.

**`function`**  
Declaración de función. Con `*` se convierte en generador. Con `async` se convierte en función asíncrona.

---

## G

**`get`**  
Modificador de propiedad de solo lectura (getter) en una clase o extensión.

**Generator**  
Función marcada con `*` que puede hacer `yield` de valores. Devuelve `{ value, done }` en cada llamada a `.next()`.

---

## H

**HIR (High-Level IR)**  
Representación intermedia de alto nivel usada durante la fase de compilación, posterior al AST y anterior al SSA.

---

## I

**`if / else`**  
Sentencia condicional. Puede usarse con narrowing de tipos.

**`implements`**  
Indica que una clase o enum implementa una o más interfaces.

**`import`**  
Importa símbolos de otro módulo o de la stdlib (`"std:name"`).

**`in`**  
Operador usado en `for...in` para iterar claves de objeto.

**`instanceof`**  
Operador de verificación de tipo en tiempo de ejecución. Devuelve `bool`. También estrecha el tipo en bloques condicionales.

**`int`**  
Tipo primitivo de entero de 48 bits con signo y wrapping en overflow.

**`interface`**  
Contrato estructural sin implementación. Los campos opcionales se marcan con `?`. Varn usa tipado estructural.

**`is`**  
Operador de verificación de tipo (type assertion). Ejemplo: `v is decimal`.

**`isIsolate`**  
Variable booleana global del runtime. `true` cuando el módulo corre dentro de un isolate hijo.

---

## J

**JIT (Just-In-Time Compilation)**  
Backend de Cranelift x86-64 que compila funciones calientes a código nativo.

---

## L

**`let`**  
Declaración de variable mutable.

---

## M

**`match`**  
Expresión de pattern matching. Puede coincidir con valores literales, enums, payload de enums y wildcards (`_`). También puede usarse con multi-valor (`2 | 3`).

**`MetaKey`**  
API de metadatos tipados (`std:reflect`). Asocia valores arbitrarios a clases a través de claves únicas.

---

## N

**`new`**  
Crea una instancia de clase o de colección (`new Map<int>()`, `new Box<str>()`).

**`null`**  
Valor que representa ausencia de un valor. Sólo asignable a tipos nulables (`T?`).

**Null Safety**  
Sistema de tipos que distingue `T` (no-nulo) de `T?` (nulable). El operador `?.` accede de forma segura y `??` provee valores por defecto.

---

## O

**`of`**  
Operador usado en `for...of` y `for await...of`.

**`override`**  
Modificador de método que indica explícitamente que sobreescribe un método de la clase padre.

---

## P

**Pipeline (`|>`)**  
Operador que pasa el valor de la izquierda como argumento a la función de la derecha. El marcador `_` indica la posición del valor.

**Primary Constructor**  
Sintaxis compacta de constructor declarada en la firma de la clase: `class User(public id: int, public name: str)`.

---

## R

**`raw string` (`"""…"""`)**  
Literal de cadena delimitado por triple comilla. No procesa secuencias de escape.

**Record (`#{…}`)**  
Tipo inmutable de objeto con igualdad estructural profunda (`==`).

**Record<K, V>**  
Tipo de mapa clave→valor con claves de tipo `K` y valores de tipo `V`.

**`return`**  
Devuelve un valor desde una función y termina su ejecución.

---

## S

**`set`**  
Modificador de propiedad de escritura (setter) en una clase o extensión.

**Spread (`...`)**  
Operador que expande elementos de un array u objeto en otro array u objeto.

**SSA (Static Single Assignment)**  
Forma de representación intermedia usada por el compilador donde cada variable se asigna exactamente una vez.

**`static`**  
Modificador de campo o método de clase que pertenece a la clase misma, no a instancias individuales.

**`str`**  
Tipo primitivo de cadena de texto inmutable. **Nunca** usar alias como `string` o `String` (excepto la clase stdlib).

**`super`**  
Referencia a la clase padre. Usado para llamar al constructor (`super(args)`) o métodos padre (`super.method()`).

---

## T

**Tagged Template**  
Template literal prefijado con una función: `tag\`texto ${expr}\``. La función recibe `strings: str[]` y `...values: dynamic[]`.

**`this`**  
Referencia a la instancia actual dentro de métodos de clase o extensiones.

**`throw`**  
Lanza un error o excepción.

**Tuple (`#[…]`)**  
Colección heterogénea inmutable con igualdad estructural profunda (`==`).

**`try`**  
Inicia un bloque de código protegido para captura de errores.

**`type`**  
Declaración de alias de tipo. Ejemplo: `type StringOrInt = str | int`.

**TypeScript Parameter Properties**  
Parámetros de constructor con `public`/`private` que se convierten automáticamente en campos: `constructor(public id: int)`.

---

## U

**Union Type**  
Tipo que puede ser uno de varios tipos posibles. Ejemplo: `str | int`, `T | null`.

**`using`**  
Declaración de recurso con disposición automática al salir del bloque o función. Equivalente a `using` de C# 8.

---

## V

**`var`**  
Declaración de variable mutable (sinónimo de `let`).

**`void`**  
Tipo de retorno de funciones que no devuelven ningún valor significativo.

---

## W

**Widening**  
Coerción implícita de un tipo numérico a otro más amplio: `int → float`, `int → decimal`, `int → bigint`. No requiere cast.

**`with` Expression**  
Expresión que clona un objeto o instancia sobreescribiendo solo los campos especificados, manteniendo el original inmutable. Ejemplo: `u1 with { name: "Bob" }`.

**`while`**  
Bucle que repite mientras la condición sea `true`.

---

## Y

**`yield`**  
Sentencia dentro de un generador que produce un valor y suspende la ejecución hasta el próximo `.next()`.
