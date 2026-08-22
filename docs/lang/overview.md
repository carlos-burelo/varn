# Varn Language Specification – Overview

Este documento sirve como punto de partida para la especificación completa del lenguaje Varn. Cada sección posterior refuerza los conceptos descritos aquí con ejemplos extraídos de la suite de pruebas ubicada en `tests/*.vn`.

## Contenido de la especificación

| Archivo | Tema | Descripción breve |
|---|---|---|
| [`overview.md`](overview.md) | Visión general | Filosofía del lenguaje, modelo mental y organización de la documentación. |
| [`lexical.md`](lexical.md) | Sintaxis Léxica | Identificadores, literales (numéricos, `str`, `int`, `float`, `bool`, `char`, **Raw Strings**), comentarios y operadores. |
| [`identifiers.md`](identifiers.md) | Identificadores y Keywords | Reglas de nombres, palabras reservadas y convenciones de estilo. |
| [`types.md`](types.md) | Sistema de tipos | Tipos primitivos, estructuras, clases, enums, genéricos, uniones, tipos nulos, operador `??=` y tipos intrínsecos. |
| [`expressions.md`](expressions.md) | Expresiones | Literales, variables, llamadas, objetos, arrays, tuplas, rangos, spreads, **`with`**, destructuring, null‑coalescing, `match`/`switch`, plantillas etiquetadas, etc. |
| [`statements.md`](statements.md) | Sentencias y control de flujo | `if/else`, `switch`, bucles (`while`, `for`, `do‑while`), `break`, `continue`, `return`, `throw`, manejo de excepciones, `try/catch/finally`, `async/await`, generadores y async‑generadores. |
| [`functions_methods.md`](functions_methods.md) | Funciones y métodos | Declaración, parámetros nombrados, valores por defecto, sobrecarga, `async`, generadores, constructores y destructores. |
| [`classes_objects.md`](classes_objects.md) | Clases y objetos | Definición de clases, visibilidad, campos, métodos, herencia, `record`‑like semantics y expresión `with`. |
| [`modules_packages.md`](modules_packages.md) | Módulos y paquetes | Sistema de importación/exportación, organización de código y stdlib. |
| [`async_concurrency.md`](async_concurrency.md) | Concurrencia asíncrona | Isolates, canales, `Future`, `Task`, `await`. |
| [`generators.md`](generators.md) | Generadores | `yield`, iteradores y uso en bucles. |
| [`error_handling.md`](error_handling.md) | Manejo de errores | Tipos de excepción, propagación y bloques `try/catch/finally`. |
| [`standard_library.md`](standard_library.md) | Biblioteca estándar | Funcionalidades usadas en los tests (`print`, `assert`, `std:csv`, `std:process`, `std:math`, etc.). |
| [`runtime_behavior.md`](runtime_behavior.md) | Comportamiento en tiempo de ejecución | Modelo de memoria, GC generacional, JIT vs intérprete. |
| [`examples.md`](examples.md) | Ejemplos integrados | Fragmentos de código tomados directamente de los archivos de prueba, organizados por tema. |
| [`glossary.md`](glossary.md) | Glosario | Definiciones de palabras clave y conceptos relevantes. |

## Organización de los archivos
Los archivos se encuentran bajo `docs/lang/` en la raíz del proyecto y están enlazados desde `docs/lang/README.md` para una navegación sencilla.

---

*Esta especificación se genera sin utilizar comandos Git, en cumplimiento con la normativa del proyecto.*
