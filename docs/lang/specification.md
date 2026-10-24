# Varn Language Specification (índice verificado)

Fuente única: `tests/*.vn` (220 archivos; `tests/main.vn` importa 218). Código manda.

| Área | Doc | Test canónico |
|---|---|---|
| Panorama + regla sin sintaxis no probada | `overview.md` | `main.vn` |
| Sentencias, control, `match`, `using` | `statements.md` | `09-control-flow.vn`, `10-match.vn`, `11-errors.vn`, `18-ranges.vn` |
| Expresiones, `\|>`, templates | `expressions.md` | `19-pipeline.vn`, `04-templates.vn` |
| Tipos, nulables, uniones, genéricos, enums | `types.md` | `08-null-safety.vn`, `30-nullable-types.vn`, `15-unions.vn`, `14-generics.vn`, `36-advanced-generics.vn`, `16-enums.vn`, `41-advanced-enums.vn`, `34-char-type.vn`, `35-decimal-bigint.vn` |
| Funciones, closures, named-args, HOF | `functions_methods.md` | `07-closures.vn`, `37-complex-closures.vn`, `40-named-arguments.vn`, `38-advanced-higher-order.vn` |
| Clases, interfaces, enums avanzados | `classes_objects.md` | `12-classes.vn`, `13-interfaces.vn`, `41-advanced-enums.vn` |
| Módulos e imports | `modules_packages.md` | `main.vn`, `31-stdlib-migration-test.vn`, `42-stdlib-comprehensive-test.vn` |
| Async, isolates, canales | `async_concurrency.md` | `21-async.vn`, `33-globals-async-coherence.vn`, `77-async-generators.vn`, `47-isolates-multithread.vn`, `54-channels.vn` |
| Generadores | `generators.md` | `20-generators.vn`, `77-async-generators.vn` |
| Errores | `error_handling.md` | `11-errors.vn`, `109-try-operator.vn`, `90-multi-catch.vn` |
| Gramática formal (EBNF) | `grammar.md` | derivada de `varn-lexer` y `varn-parser`; verificada con `vn debug -p ast` |
| Léxico y literales | `lexical.md` | `01-arithmetic.vn`, `02-boolean.vn`, `03-strings.vn`, `04-templates.vn`, `34-char-type.vn`, `35-decimal-bigint.vn` |
| Identificadores y keywords | `identifiers.md` | `crates/varn-lexer/src/keywords.rs` |
| Glosario | `glossary.md` | términos con su test |
| Ejemplos que corren | `examples.md` | cada bloque cita su `tests/*.vn` |
| Runtime | `runtime_behavior.md` | links a `../internals/VM_ARCHITECTURE.md`, `../internals/RUNTIME_ARCHITECTURE.md` |
| Núcleo y plataforma | `SPEC_NUCLEO_Y_PLATAFORMA.md` | índice, propuesta histórica parcial |

Lo no citado con su `tests/*.vn` no es contrato.
