# Visión de Evolución Sintáctica, Corrección de Errores Históricos de JS y Adaptación TC39 en Varn (`docs/LANGUAGE_DX_VISION.md`)

Este documento constituye la especificación de Experiencia de Desarrollo (DX) de **Varn**, ajustada tras auditar el compilador (`crates/varn-parser`, `crates/varn-checker`) y adaptar las mejores propuestas del comité **TC39** de JavaScript sin adoptar "magia negra" ni comprometer el rendimiento.

---

## 1. Corrección Sistemática de los Errores Históricos de JavaScript

Varn aprovecha su naturaleza **estáticamente tipada** y su motor compilado para erradicar las fallas de diseño de JavaScript:

| Error Histórico de JavaScript | Solución Arquitectónica en Varn | Estado / Implementación |
| :--- | :--- | :--- |
| **1. Coerciones Implícitas Bizarras** (`"5" + 3 = "53"`, `[] + {} = "[object]"`) | **Coerción Estricta**: No hay conversión implícita entre tipos no numéricos. Las coerciones matemáticas siguen reglas bit-idénticas (`numeric.rs`). | **Implementado** |
| **2. Pérdida de `this` en Métodos** (`const f = obj.fn; f();` pierde `this`) | **Auto-Bound Methods**: Extraer un método de un objeto crea automáticamente un `BoundMethod` que preserva a `this` de forma transparente. | **Implementado** |
| **3. Dualidad `null` vs `undefined`** (Confusión de centinelas de vaciado) | **Único Centinela `null`**: Un solo valor nulo. Tipado estricto `T | null` verificado por `varn-checker` sin NullPointerExceptions. | **Implementado** |
| **4. `switch` con Fall-Through Silencioso** (Olvidar `break` ejecuta el siguiente caso) | **Expresión `match` Exhaustivo**: `match` retorna valores, verifica exhaustividad en compilación y no permite fall-through silencioso. | **Implementado** |
| **5. Mutabilidad Implícita en `const`** (`const obj = {}` permite mutar sus campos) | **Registros Inmutables por Defecto**: Registros `#{...}` y Tuplas `#[...]` inmutables, junto con calificadores `readonly` en clases. | **En Hoja de Ruta** |
| **6. `throw` Invisible y Excepciones No Verificadas** | **Result/Option y Tipos de Suma**: Tipado explícito de errores con `Result<T, E>` y métodos seguros `.unwrapOr()`. | **Implementado** |
| **7. Ambivalencia `for...in` vs `for...of`** | **Único Bucle `for (x in iter)`**: Iteración unificada y clara sobre colecciones y rangos. | **Implementado** |

---

## 2. Propuestas de TC39 Tomadas Prestadas y Adaptadas a Varn

De las propuestas activas de TC39 (Stage 1 a 4), adaptamos aquellas que encajan limpiamente con la naturaleza compilada y estáticamente tipada de Varn:

### 2.1 Registros y Tuplas TC39 (`#{}` y `#[]`) con Igualdad Estructural Profunda
- **En JS**: Los objetos normales comparan por referencia (`{a:1} !== {a:1}`).
- **Adaptación en Varn**: Adoptar la sintaxis `#[]` para tuplas y `#{}` para registros inmutables. La comparación `===` realiza **igualdad estructural profunda por valor**:
  ```varn
  const a = #{ x: 10, y: 20 };
  const b = #{ x: 10, y: 20 };
  assert("igualdad estructural", a === b); // Evalúa a true
  
  const #[head, ...tail] = #[10, 20, 30];
  ```

### 2.2 Auto-Accessors en Clases (`accessor prop: type`)
- **De la Propuesta de Decoradores TC39 (Stage 3)**: La sintaxis `accessor` genera automáticamente el campo privado subyacente y sus respectivos getter y setter.
- **Adaptación en Varn**:
  ```varn
  class UserAccount {
      accessor balance: int = 100
  }
  ```
  Permite que decoradores (`@logChange`, `@validate`) intercepten lecturas/escrituras de propiedades sin escribir código repetitivo.

### 2.3 Desestructuración de Rest de Arreglo en `match` (`[head, ...tail]`)
- **Adaptación en Varn**: Permitir la extracción del primer elemento y el resto de la lista directamente dentro de un brazo de `match`:
  ```varn
  function sumList(nums: int[]): int {
      return match (nums) {
          [head, ...tail] => head + sumList(tail),
          [] => 0
      }
  }
  ```

### 2.4 Agrupación de Colecciones (`arr.groupBy()`)
- **De TC39 `Object.groupBy` / `Map.groupBy` (Stage 4)**:
- **Adaptación en Varn**: Método de extensión fuertemente tipado sobre colecciones:
  ```varn
  const users: User[] = getActiveUsers();
  const grouped: Map<str, User[]> = users.groupBy(u => u.role);
  ```

### 2.5 Iterator Helpers perezosos (`.take()`, `.drop()`, `.filter()`, `.map()`)
- **De TC39 Iterator Helpers (Stage 4)**: Procesar iteradores y rangos de forma **perezosa (*lazy*)** sin alocar arreglos intermedios:
  ```varn
  (0..1000).stepBy(2).take(10).forEach(print);
  ```

---

## 3. API de Concurrencia y Aislados (Basada en Librería Estándar)

Dado que la sintaxis declarativa de concurrencia fue descartada por el momento, la comunicación entre aislados se mantiene 100% explícita y robusta a través de la librería estándar `std:runtime`:

```varn
import { Isolate, Channel } from "std:runtime"

const channel = new Channel<int>()
Isolate.spawn(channel, (ch) => {
    ch.send(42);
})
```

---

## 4. Características Existentes Confirmadas en Varn

Auditoría verificada en el compilador (`varn-parser` / `varn-checker`):

1. **Guardas `if` en `match`**: Soportadas nativamente en `parse_match_case` (`MatchCase.guard`):
   ```varn
   match (n) {
       0 => "zero",
       val if val < 0 => "negative",
       _ => "positive"
   }
   ```
2. **Gestión de Recursos con `using` y `await using`**: Implementadas en `parse_using_stmt`. Invocan `.dispose()` al salir del ámbito local.
3. **Cadenas Multilínea e Interpoladas**: Soportadas mediante Template Literals `` `linea1\nlinea2 ${expr}` `` (sin requerir prefijos ambiguos `r"` o `f"`).

---

## 5. Expresiones `catch` en Pipeline y `.unwrapOr()` sin Sobrecargar `?`

El operador `?` se mantiene reservado para tipos anulables (`str?`), navegación opcional (`?.`) y ternarios. El manejo de errores en pipelines utiliza expresiones `catch`:

```varn
const config = configFile 
    |> readTextFile(_) 
    |> parseYaml(_) 
    |> catch (err => defaultConfig)
```

---

## 6. Slicing de Arreglos y Métodos de Rango (`arr[1..4]`, `.stepBy()`)

```varn
// 1. Slicing de arreglos con sintaxis de rango
const nums = [10, 20, 30, 40, 50];
const sub = nums[1..4]; // [20, 30, 40]

// 2. Método de extensión explícito sobre Range
for (i in (0..100).stepBy(5)) {
    print(i);
}
```

---

## 7. Resumen Sintáctico de Ajustes

| Característica | Adaptación TC39 / Varn | Ventaja en Varn |
| :--- | :--- | :--- |
| **Registros & Tuplas** | `#{ x: 10 }` y `#[1, 2]` | Igualdad estructural profunda `===` basada en contenido. |
| **Auto-Accessors** | `accessor balance: int = 100` | Cero código repetitivo en getters/setters interceptables. |
| **Pattern Matching** | `[head, ...tail]` en `match` | Desestructuración recursiva de arreglos. |
| **Agrupación** | `arr.groupBy(u => u.role)` | Agrupación tipada `Map<K, V[]>` en 1 línea. |
| **Iteradores** | `.stepBy(2).take(10)` | Evaluación perezosa (*lazy*) sin alocar arreglos intermedios. |
| **Concurrencia** | `std:runtime` (`Isolate.spawn`) | Basado en librería limpia sin cambios en la gramática. |
