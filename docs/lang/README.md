# Varn Language Specification

Especificación completa del lenguaje Varn, generada a partir de la suite de pruebas `tests/*.vn` (89 suites, 1139+ tests).

---

## Índice de Contenidos

| # | Documento | Descripción |
|---|-----------|-------------|
| 1 | [overview.md](overview.md) | Visión general, filosofía y mapa de la documentación |
| 2 | [lexical.md](lexical.md) | Sintaxis léxica: identificadores, literales, raw strings, operadores, comentarios |
| 3 | [identifiers.md](identifiers.md) | Identificadores, palabras reservadas, convenciones de nomenclatura |
| 4 | [types.md](types.md) | Sistema de tipos: primitivos, arrays, tuplas, records, enums, uniones, generics, null safety, coerción |
| 5 | [expressions.md](expressions.md) | Expresiones: literales, operadores, calls, destructuring, closures, match, pipeline, with, tagged templates |
| 6 | [statements.md](statements.md) | Sentencias y control de flujo: if/else, match, bucles, break/continue, try/catch/finally, using, throw |
| 7 | [functions_methods.md](functions_methods.md) | Funciones: declaración, named args, default values, genérics, HOF, closures, recursión, async, generators |
| 8 | [classes_objects.md](classes_objects.md) | Clases: herencia, abstract, static, primary constructors, extension methods, decoradores, with expressions |
| 9 | [modules_packages.md](modules_packages.md) | Módulos: import/export, stdlib, paquetes disponibles |
| 10 | [async_concurrency.md](async_concurrency.md) | Concurrencia: async/await, spawn, parallel, TaskGroup, isolates, canales tipados |
| 11 | [generators.md](generators.md) | Generadores: function*, yield, async generators, valores complejos |
| 12 | [error_handling.md](error_handling.md) | Manejo de errores: throw, try/catch/finally, errores personalizados, salidas tempranas |
| 13 | [standard_library.md](standard_library.md) | Biblioteca estándar: todos los métodos de tipos primitivos, colecciones y módulos stdlib |
| 14 | [runtime_behavior.md](runtime_behavior.md) | Runtime: intérprete, JIT, GC generacional, modelo de memoria, isIsolate |
| 15 | [examples.md](examples.md) | Ejemplos integrados por tema, extraídos directamente de los tests |
| 16 | [glossary.md](glossary.md) | Glosario de términos del lenguaje |

---

## Cobertura por Test Suite

| Tests | Característica | Documento principal |
|-------|---------------|---------------------|
| 01 | Aritmética y operadores bit a bit | [lexical.md](lexical.md) |
| 02 | Booleanos y cortocircuito | [lexical.md](lexical.md) |
| 03 | Métodos de `str` | [standard_library.md](standard_library.md) |
| 04 | Template literals | [expressions.md](expressions.md) |
| 05 | Arrays, métodos, semantic identity `Array<T>` ↔ `T[]` | [types.md](types.md) |
| 06 | Destructuring de objetos y arrays, spread | [expressions.md](expressions.md) |
| 07 | Closures y composición | [functions_methods.md](functions_methods.md) |
| 08 | Null safety (`?.`, `??`) | [expressions.md](expressions.md) |
| 09 | Control de flujo: `if`, `while`, `for`, `for-of`, `for-in`, `break`, `continue` | [statements.md](statements.md) |
| 10 | Match expressions, wildcards, multi-valor | [statements.md](statements.md) |
| 11 | Manejo de errores, `try/catch/finally`, clases de error personalizadas | [error_handling.md](error_handling.md) |
| 12 | Clases: getters, setters, herencia, `override`, `super`, `abstract`, `static` | [classes_objects.md](classes_objects.md) |
| 13 | Interfaces, tipado estructural, campos opcionales | [types.md](types.md) |
| 14 | Genéricos: clases, funciones, métodos | [types.md](types.md) |
| 15 | Tipos unión (`\|`), `instanceof` | [types.md](types.md) |
| 16 | Enums simples, `rawValue` | [types.md](types.md) |
| 17 | `Map` y `Set` | [standard_library.md](standard_library.md) |
| 18 | Rangos (`..`, `..=`), `step`, `toArray` | [types.md](types.md) |
| 19 | Operador pipeline (`\|>`) con placeholder `_` | [expressions.md](expressions.md) |
| 20 | Generadores (`function*`, `yield`, `.next()`) | [generators.md](generators.md) |
| 21 | Async/await, `spawn`, `parallel`, `TaskGroup` | [async_concurrency.md](async_concurrency.md) |
| 22 | Recursión directa y mutua | [functions_methods.md](functions_methods.md) |
| 23 | Clases complejas: Builder pattern, LinkedList genérica | [classes_objects.md](classes_objects.md) |
| 24 | `Record<K, V>` como mapa tipado | [types.md](types.md) |
| 25 | Extension methods en `str`, clases e `int` | [classes_objects.md](classes_objects.md) |
| 26 | Coerción numérica implícita (widening) | [types.md](types.md) |
| 27 | Decoradores de clase y método, `MetaKey` | [classes_objects.md](classes_objects.md) |
| 28 | Extension methods comprehensive | [classes_objects.md](classes_objects.md) |
| 29 | Enum variant destructuring en `match` | [types.md](types.md) |
| 30 | Tipos nulables complejos: `T?`, `T?[]`, `T[]?`, `T?[]?` | [types.md](types.md) |
| 34 | Tipo `char`: literales, métodos, clasificación | [types.md](types.md) |
| 35 | `decimal` y `bigint`: literales, aritmética, métodos | [types.md](types.md) |
| 36 | Genéricos avanzados: `Either`, `Cons`, HOF genéricas, memoización | [functions_methods.md](functions_methods.md) |
| 40 | Named arguments, parámetros por defecto | [functions_methods.md](functions_methods.md) |
| 41 | Enums avanzados: payload, shared fields, métodos, genéricos, interfaces | [types.md](types.md) |
| 47 | Isolates, canales tipados, protocolo enum | [async_concurrency.md](async_concurrency.md) |
| 54 | Canales: `close`, `for await`, `using`, cross-isolate | [async_concurrency.md](async_concurrency.md) |
| 69 | Tuplas `#[…]` y Records `#{…}`: igualdad estructural | [types.md](types.md) |
| 76 | Generadores: valores complejos, estado global, return | [generators.md](generators.md) |
| 77 | Async generators: `await` en yield, `for await`, captura de errores | [generators.md](generators.md) |
| 78 | Salidas tempranas de `try/catch` (`break`/`continue`/`return`) | [error_handling.md](error_handling.md) |
| 79 | Control-flow type narrowing: `!== null`, truthy, `instanceof` | [types.md](types.md) |
| 80 | Capacidades del host: `std:fs`, `std:sys` | [modules_packages.md](modules_packages.md) |
| 82 | `std:csv` (RFC 4180), `std:json` | [modules_packages.md](modules_packages.md) |
| 83 | `std:regex` (stateless), `std:crypto` (UUID v4/v7), `std:time` (DateTime) | [modules_packages.md](modules_packages.md) |
| 84 | `std:process`, `std:compress`, `std:cli`, `std:env`, `std:path` | [modules_packages.md](modules_packages.md) |
| 85 | `std:sqlite`, `std:compress` (Tar/Zip), `std:ws` | [modules_packages.md](modules_packages.md) |
| 86 | Tagged template functions, `db.sql` parametrizado | [expressions.md](expressions.md) |
| 87 | Primary constructors (C# y TS style), operador `??=` | [classes_objects.md](classes_objects.md), [expressions.md](expressions.md) |
| 88 | Range indexing en strings y arrays (`arr[a..b]`, `str[a..=b]`) | [types.md](types.md) |
| 89 | Raw String Literals (`"""…"""`), `with` expressions | [lexical.md](lexical.md), [classes_objects.md](classes_objects.md) |

---

## Generación

Esta especificación fue generada el **2026-08-22** a partir de los 89 archivos de prueba en `tests/*.vn`, que representan el comportamiento observable y verificado del compilador, VM e intérprete de Varn.

Para ejecutar la suite completa y verificar que todos los tests pasan:

```bash
vn run ./tests/main.vn
vn bench ./tests/main.vn -v

# Verificar parity intérprete/JIT:
VARN_NO_JIT=1 vn run ./tests/main.vn

# Verificar bundle embebido:
VARN_STD=@embedded vn run ./tests/main.vn
```
