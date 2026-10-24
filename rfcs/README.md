# RFCs

Proceso para cambiar el lenguaje o su biblioteca estándar.

1. Copia [`0000-template.md`](0000-template.md) a `rfcs/NNNN-titulo-corto.md` (siguiente número libre).
2. Abre un PR con el RFC. La discusión ocurre ahí.
3. El mantenedor lo marca `accepted`, `rejected` o `postponed` en el encabezado y lo mergea.
4. La implementación referencia el RFC. Si el RFC cambia una decisión de arquitectura, se cierra además con un ADR.

Un RFC debe nombrar la ganancia, cómo se mide y qué se borra. "JS lo hace así" no es argumento (Ley 17 de [`AGENTS.md`](../AGENTS.md)).
