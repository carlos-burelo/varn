# ADR 0028: Inlining entre módulos mediante paquetes TIR

## Estado
Implementado (2026-11-13). Alcance parcial: ver Límites.

## Contexto
Cada módulo se compilaba aislado y el módulo de entrada se compilaba antes que sus
dependencias. Una llamada a una función importada cargaba el valor de función desde un
global y hacía una llamada indirecta con comprobación de firma, y el compilador no podía
inlinarla.

Medición (binario `release`, 50M iteraciones con dos funciones pequeñas importadas,
mediana):

| Caso | Antes | Después | Mismas funciones en el archivo |
|---|---|---|---|
| `inc` y `clamp` importadas | 303 ms | 34 ms | 31 ms |
| Helper que lee una constante del módulo | 397 ms | 73 ms | 74 ms |
| Cadena de tres helpers (`hyp2` → `addSq` → `sq`) | no medido | 123 ms | 114 ms |

El grafo de módulos ya es acíclico: un ciclo de imports es el error
`circular dependency detected` (`vn-pipeline/src/module_precompile.rs`). Un import es
siempre un enlace a un módulo ya inicializado.

## Decisión
1. El pipeline compila las dependencias antes que el módulo que las importa
   (`compile.rs::build_executable`: `discover`, `lower_dependencies`, módulo de entrada).
2. Cada módulo bajado publica `ModuleTemplates` (`vn-tir/src/package.rs`):
   - un `InlinePackage` por función exportada elegible: la función y las hermanas que
     llama directamente, con sus tipos re-internados en una `TyTable` portable;
   - las constantes exportadas cuyo valor es un literal;
   - la bandera `inert`.
3. `vn_tir::link::link` (`link.rs`), entre `emit` y `compile`, instala los paquetes usados
   en el `TirModule` del importador como funciones del módulo con un global
   `__inl<huella>::nombre`, remapea tipos y firmas, y reescribe las llamadas a `DirectFn`.
   Si el compilador no inlina, la llamada sigue siendo una llamada directa correcta.
4. La elegibilidad la decide un único predicado, `vn_tir::inlinable::body_inlinable`,
   compartido con el inliner del compilador (Ley 6). Además, solo entran tipos sin ids
   locales (`TyTable::import_ty` rechaza clases, enums, firmas y formas) y resoluciones
   `Local`, `Param`, `Intrinsic`, `NativeOp` con argumentos, `NativeDecode` y llamadas a
   hermanas.
5. Las exportaciones son constantes por regla del lenguaje (`VN4021`). Un global asignado
   una sola vez con un literal en el nivel superior se pliega en el importador y dentro
   de los paquetes.
6. Un módulo es inerte si su nivel superior solo asigna literales y no tiene clases,
   imports relativos ni reexports. Si todos los especificadores de un import quedan
   enlazados y sin referencias y el módulo es inerte, el import se elimina y el módulo no
   se carga.
7. `std.vnb` lleva una sección `templates` por módulo (postcard); la `TemplateLibrary` del
   pipeline la decodifica bajo demanda.

## Alternativas descartadas
- **Reescribir el texto fuente antes de parsear** (copiar la función al importador y quitar
  el especificador del import). Se implementó y se midió con el mismo rendimiento
  (`inc`/`clamp` 32 ms), y se retiró: invierte el orden de fases (consulta interfaces antes
  de parsear), sustituye `Compilation.source` por texto sintetizado que aparece en los
  diagnósticos, parsea cada módulo dos veces y mantiene un segundo criterio de
  elegibilidad sobre el AST.
- **Enlace estático sin inlining** (llamada directa a la función de otro módulo): no
  elimina el coste que se mide y exige una unidad de compilación común para el código
  máquina.

## Consecuencias
- El formato de `std.vnb` cambia (13 palabras por entrada de módulo); las imágenes de otro
  formato se reconstruyen. Los formatos internos no son estables
  ([`STABILITY.md`](../STABILITY.md)).
- Cada importador compila su propia copia del helper, acotada por el presupuesto de 96
  nodos del predicado.
- Un módulo inerte totalmente enlazado ya no se carga: lo observable es que no ejecuta
  nada, porque no tiene efectos al cargarse.
- La identidad de función entre módulos no es observable: la igualdad de funciones no
  existe en el lenguaje (`VN3010`).

## Límites
No cubre: métodos y clases importados (los ids de clase son locales a cada módulo),
funciones que leen estado de módulo no constante, llamadas nativas sin argumentos dentro de
un paquete, genéricos, `import * as ns` y paquetes, ni el enlace entre módulos de `std` al
construir la imagen.

## Referencias comparadas
- Rust (`#[inline]`) y Swift (`@inlinable`): el IR del compilador viaja en el artefacto del
  módulo y se instancia en el importador. Es el modelo que se adopta.
- Kotlin (`inline fun`): el compilador copia el cuerpo al llamador a nivel de bytecode.
- C# y Java: el JIT inlinea entre ensamblados en tiempo de ejecución, sin ayuda del
  compilador.
