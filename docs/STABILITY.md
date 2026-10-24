# Estabilidad y versionado

Varn está en **pre-1.0**. Mientras la versión mayor sea `0`:

- Un cambio incompatible sube la versión *minor* (`0.1` → `0.2`). Se acepta cuando trae una ganancia medible (ver Ley 10 de [`AGENTS.md`](../AGENTS.md)).
- Un fix compatible sube la *patch*.
- Todo cambio incompatible aparece en [`CHANGELOG.md`](../CHANGELOG.md) bajo `Changed` o `Removed`, con la migración.

## Qué cubre la compatibilidad (desde 1.0)

| Superficie | Compromiso |
|---|---|
| Sintaxis y semántica del lenguaje ([`docs/lang/`](lang/README.md)) | Estable dentro de una *edición* |
| Biblioteca estándar `std:*` | Estable salvo módulos marcados `experimental` |
| CLI (`vn`) | Subcomandos y flags documentados en [`CLI_REFERENCE`](CLI_REFERENCE.md) |
| Formatos internos (bytecode, `std.vnb`, caché JIT) | **No** estables: se invalidan por versión del binario |
| Crates de Rust | **No** son API pública |

## Ediciones

Los cambios incompatibles del lenguaje posteriores a 1.0 se agrupan en ediciones
nombradas por año. Un proyecto declara su edición en su manifiesto y el compilador
acepta las anteriores. Cada edición nueva pasa por el proceso de [`rfcs/`](../rfcs/README.md).
