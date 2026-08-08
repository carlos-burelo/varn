# Visión de Sintaxis y Experiencia de Desarrollo (DX)

Este documento esboza la visión estratégica de evolución sintáctica y ergonometría del lenguaje **Varn**, enfocada en maximizar la productividad del desarrollador (DX) manteniendo el rendimiento en tiempo de ejecución.

---

## Tabla de Contenidos

- [1. Objetivos DX](#1-objetivos-dx)
- [2. Guardas y Patrones Avanzados en `match`](#2-guardas-y-patrones-avanzados-en-match)
- [3. Enlazado Asíncrono de Pipelines (`|>`)](#3-enlazado-asíncrono-de-pipelines-)
- [4. Ergonometría del Sistema de Tipos](#4-ergonometría-del-sistema-de-tipos)

---

## 1. Objetivos DX

1. **Sintaxis Expresiva y Fluida**: Reducir el código repetitivo (*boilerplate*) sin perder la claridad estática.
2. **Mensajes de Error Instructivos**: Diagnósticos comprensibles con subrayado de código y sugerencias contextuales.
3. **Tooling Integrado de Primera Clase**: LSP reactivo con SemanticDB y formateador automático.

---

## 2. Guardas y Patrones Avanzados en `match`

Propuesta para añadir guardas condicionales `if` dentro de las ramas de un `match`:

```Varn
const clasificacion = match (numero) {
    n if n < 0 => "Negativo",
    n if n % 2 === 0 => "Par positivo",
    _ => "Impar positivo"
}
```

---

## 3. Enlazado Asíncrono de Pipelines (`|>`)

Permitir que el operador pipeline maneje implícitamente promesas `async`:

```Varn
const datos = await "https://api.ejemplo.com/data"
    |> http.get(_)
    |> json.parse(_)
    |> procesarRespuesta(_)
```

---

## 4. Ergonometría del Sistema de Tipos

- **Constructores Simplificados**: Inyección automática de propiedades en constructores:
  ```Varn
  class Usuario(pub nombre: str, pub edad: int) {}
  ```
- **Type Guards Definidos por el Usuario**:
  ```Varn
  function esCadena(v: unknown): v is str {
      return v instanceof str
  }
  ```
