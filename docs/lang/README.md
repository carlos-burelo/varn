# Guía del lenguaje Varn

Documentación del lenguaje escrita **desde la suite**: cada rasgo cita el test de
`tests/` que lo ejercita. Lo que no tiene test citado no es contrato.

- [`overview.md`](overview.md) — panorama de la suite y rasgos probados.
- [`specification.md`](specification.md) — índice por área con su test canónico.
- [`examples.md`](examples.md) — programas cortos que corren.
- [`glossary.md`](glossary.md) — términos y palabras clave.

Convención de los ejemplos: usan `assertTrue("etiqueta", condición)` y
`assertEqual(a, b)` de `std:test`; se asume
`import { assertTrue, assertEqual } from "std:test"`. Varios bloques continúan
al anterior (reutilizan sus clases y funciones).

Suite: 220 archivos `tests/*.vn`; `tests/main.vn` importa 218 (todos salvo sí
mismo y `75-async-shapes-lib.vn`, auxiliar de `75-async-shapes.vn`) y cierra con
`summary()`. Huecos de numeración: 39, 81, 91, 104–108.
