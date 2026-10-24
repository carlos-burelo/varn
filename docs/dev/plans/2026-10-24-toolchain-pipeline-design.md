# Toolchain: un pipeline, una sesión, comandos delgados

Estado: borrador para aprobación. Alcance: `varn-pipeline` y `varn-cli` (más lo que ambos
arrastran en `varn-dap`, `varn-lsp` y `varn-lsp::inspect`).

## Por qué

`vn` creció comando a comando. Cada comando llegó a la compilación por el camino que
tenía a mano, y hoy el compilador se invoca por al menos seis rutas distintas que repiten
la misma secuencia `lex → parse → check → emit_tir → inject_star_reexports →
resolve_import_slots → compile_tir`:

| Ruta | Dónde |
|---|---|
| `compile_source` / `compile_source_cached` | `varn-pipeline/src/lib.rs` |
| `compile_source_inner` (stdlib) | `varn-pipeline/src/stdlib_loader.rs` |
| `build_module_graph` (dependencias) | `varn-pipeline/src/module_precompile.rs` |
| `collect_test_targets` | `varn-pipeline/src/check.rs` |
| fases reimplementadas a mano para medir | `varn-cli/src/bench/source/{mod,e2e}.rs` |
| `parse_partial` + `check_with(tooling)` | `varn-lsp/src/pipeline/mod.rs` |

Consecuencias observadas en el código, no supuestas:

- `RunOpts` es una bolsa de banderas. `check` es `run` con `no_run: true`; `eval` y `repl`
  fingen un archivo (`"(eval)"`, `"(repl)"`); `test` inyecta código concatenando texto
  (`append`) y por eso recompila y re-ejecuta el archivo entero una vez por cada `@test`.
- La caché solo vale para `run` sin eval, sin append y sin debug. `build` y `test` no la usan.
- El pipeline formatea diagnósticos con ANSI y exit codes (`1`, `2`, `3`) dentro de
  `PipelineError`; el pie «could not compile … due to N errors» está copiado en `lex.rs`,
  `parse.rs` y `check.rs`. `main.rs` compara `exit_code == 3` para no imprimirlo dos veces.
- `DebugSink` mete hooks de impresión dentro de las fases: `vn debug` ve un camino y `vn run`
  otro.
- `ModuleResolver::new()` se construye en ocho sitios. Nadie es dueño de la sesión.
- Los permisos (`--allow-*`) solo existen en `run`. `eval`, `repl`, `test` y `check` fijan
  `allow_all` cada uno por su cuenta.
- `main.rs` mezcla arranque, volcado de operaciones nativas (`VARN_DEBUG_OPS`), detección de
  ejecutable standalone y una lista de subcomandos (`implicit_run`) que repite la del enum de
  clap.
- `build --native` busca el linker con rutas del Windows SDK escritas en el código y acepta dos
  formas de pedir lo mismo (`--native` y `--target native`).
- Los workers de `test` se crean con `std::thread::spawn` sin `VM_STACK_BYTES`, mientras que
  `main` y `isolate_spawn` sí lo usan.
- `fmt` y `test` tienen cada uno su recorrido de directorios; `fmt` imprime ANSI crudo.
- `inspect_lsp` (cerca de 1000 líneas) solo consume `varn_lsp` y vive en el CLI de usuario.
- `main` abre el propio ejecutable en cada arranque buscando un trailer `VEXE`, y nadie en el
  repo escribe ese trailer: es un lector sin productor.
- La raíz del proyecto se busca por separado en seis sitios (`std_root`, `env_file`, `cache`,
  `pipeline/cache`, `resolver/graph`, `doctor`), unos partiendo del cwd y otros del directorio
  del archivo de entrada, así que pueden resolver raíces distintas en una misma ejecución.
- El manifiesto es TOML y se parsea en tres sitios (`varn-pm::ProjectManifest`,
  `varn-modules::package::RawManifest` y la mini estructura `StdKey` de `std_root`). Acepta
  dos nombres de archivo (`varn.toml` y `vn.toml`, con las constantes repetidas en
  `artifact.rs` e `ids.rs`) y hasta tres formas de declarar el nombre (`[package]`,
  `[project]` y campos planos). Hay además tres buscadores de raíz (`find_project_root`,
  `project_root_cached` y `find_project_manifest`) con reglas ligeramente distintas.

Todo esto es una violación de la Ley 8 (varios caminos para el mismo hecho) y de la Ley 5
(el pipeline guarda estado y formato que pertenecen a otra capa).

## Qué se gana, cómo se verifica, qué se borra (Ley 10)

**Ganancia**

1. Un único camino de compilación: lo que `vn debug` muestra es lo que `vn run` ejecuta.
2. `vn test` compila cada archivo una vez, no una vez por `@test`.
3. Caché uniforme en `run`, `test`, `build` y `dap`.
4. Permisos y salida de errores definidos en un solo sitio.
5. Añadir un comando (lint, doc) es escribir una proyección delgada, no otra ruta.

**Verificación**

- Determinismo (Ley 4): `vn build` sobre `tests/**/*.vn` y `examples/**/*.vn` produce
  bytecode idéntico byte a byte antes y después de cada paso. Es la prueba de que el
  refactor no cambia la compilación.
- Matriz de la sección 5 de `AGENTS.md`, 4 cuadrantes, `FAILED: 0`.
- `vn test` sobre `tests/` y `vn run` con caché fría y caliente, mismo perfil `quick`,
  base contra cambio: el tiempo de `vn test` baja; el de `run` no sube.
- `vn debug -p …` y los goldens existentes en `crates/varn-cli/tests/`.

**Se borra**

`DebugSink`, `NullSink`, `CliDebugSink`; `RunOpts.{no_run, append, eval, print_silent}`;
`compile_source_for_build`; `PipelineError` con ANSI y exit codes; las tres copias del pie de
error; la bandera `--native`; la lista `SUBCOMMANDS` de `main.rs`; las fases reimplementadas
en `bench/source`; las canonicalizaciones y lecturas de fuente repetidas; el wrapper
`execute` y los parámetros `_source`, `_path`; las rutas del SDK escritas en `build.rs`;
los dos recorridos de directorios.

## Modelo

### `Session`

Dueña de todo lo que hoy se reconstruye suelto: el `ModuleResolver`, el registro de loaders,
el proyecto (raíz y manifiesto, vía `find_project_root`), la caché y la identidad de la std.
Se crea una vez por proceso (CLI) o por workspace (LSP). Los comandos no construyen
resolvers.

### `Source`

Identidad de lo que se compila, canonicalizada una sola vez al construirla:
`File(CanonicalPath)` o `Virtual { name, text }` para eval y repl. Desaparece el nombre falso
`"(eval)"` pasado como ruta.

### `Compilation`

Una sola función, `Session::compile(source, Request) -> Compilation`.

`Request` = `{ goal, mode, cache }`:

- `goal`: hasta dónde llegar. `Tokens`, `Ast`, `Checked`, `Tir`, `Executable`.
- `mode`: `Strict` (`CheckOptions::compile`, parse que falla al primer error) o `Tooling`
  (`CheckOptions::tooling`, `parse_partial`).
- `cache`: `Use` o `Bypass`. La caché solo aplica a `goal == Executable`; `vn debug` pide
  `Bypass` porque necesita los artefactos intermedios.

`Compilation` conserva, inmutables, los productos de cada etapa alcanzada (tokens, AST,
resultado del checker, TIR, `Executable`), los diagnósticos acumulados en un `DiagnosticBag`
(Ley 5) y el tiempo de cada etapa. Si una etapa falla, devuelve igualmente lo producido hasta
ahí. No hay `Result` que descarte el trabajo parcial.

Consecuencias:

- `vn debug -p tokens|ast|symbols|…` pide el goal correspondiente e imprime el artefacto. No
  hay sink dentro de las fases.
- `bench` lee los tiempos por etapa de `Compilation` y no reimplementa nada.
- Imprimir diagnósticos es trabajo de quien consume (CLI, LSP), con el `SourceMap` de la
  compilación. El pipeline no sabe de terminales.

### Un solo lowering de módulo

`emit_tir → inject_star_reexports → resolve_import_slots → compile_tir` pasa a ser una
función, `lower_module`, que llaman la entrada, `module_precompile` y la carga de la std. Las
tres copias actuales se borran.

### `Executable` lleva los tests

El compilador ya conoce los `@test` (`CheckResult::test_targets`). `Executable` los registra
como entradas con nombre y `is_async`. El runner de `vn test` compila una vez, y por cada
unidad arranca una máquina, ejecuta el módulo e invoca la entrada. `append` desaparece.
Se mantiene la semántica actual: la unidad de archivo ejecuta el módulo; cada unidad de test
ejecuta el módulo y llama a su función.

### Ejecución

`Runtime::boot(program, Policy)` y `Runtime::run`. `Policy` es la renombrada `CapabilitySet`
más `trace`, `gc`, `silent`. Un solo recorrido de errores de runtime (`format_runtime_error`)
que devuelve datos y deja el formato al borde.

### Errores

El pipeline devuelve `Failure`: `Diagnostics`, `Io`, `Internal`. El CLI los renderiza y
decide el exit code en un único `ExitCode` (Ley 16: ningún `3` suelto). `main` deja de
comparar números.

## Proyecto

El formato es TOML (el que ya usa `varn-pm`); el diseño original asumía JSON por un mensaje de
error desactualizado del LSP, y el código manda.

Un toolchain es comandos sobre un proyecto, así que el proyecto es cimiento (Ley 14) y no un
añadido posterior.

- **`Project`** = `{ root, manifest, env, cache_dir }`, construido una vez por `Session`. La
  raíz se descubre con una regla única: los comandos que reciben un archivo (`run`, `check`,
  `build`, `debug`, `bench`) parten del directorio de ese archivo; los comandos de proyecto
  (`test`, `fmt`, `lint`, `task`, `pkg`, `init`, `cache`) parten del cwd. Los seis
  `find_project_root` sueltos se reemplazan por esa construcción.
- **Crate `varn-project`**, hoja: manifiesto, descubrimiento de raíz y lectura de `.env`.
  `varn-modules`, `varn-pm`, `varn-pipeline` y `varn-cli` dependen de él (Ley 1: la
  definición de proyecto no vive arriba, en `varn-pm`).
- **Un solo manifiesto, `varn.toml`.** Se borra el alias `vn.toml` y las secciones `[package]`
  y `[project]`; quedan los campos planos (`name`, `version`, `main`, `exports`, `dependencies`, `dev_dependencies`,
  `peer_dependencies`, `workspaces`) más las secciones nuevas. Es un cambio incompatible: la
  ganancia es que un campo tiene una sola ubicación. Los manifiestos del repo (`std/`,
  `examples/`, `tests/`) se migran en el mismo paso.
- **Secciones por comando, tipadas.** Cada comando es dueño de su sección y la deserializa a
  una estructura propia, con campos desconocidos como error (no se ignoran en silencio).
  Esta tanda define `tasks` y `include`/`exclude` para `fmt` y `test`. `lint`, `doc` y
  `permissions` añaden la suya cuando existan, sin tocar el resto.
- **Workspaces**: el campo ya existe en el manifiesto. `Project` conoce la raíz del workspace
  y la de cada miembro; la resolución entre miembros es de `varn-resolver`, y esta tanda no
  la cambia.

## Comandos

Cada comando es un módulo con `Args` y `execute(&Session, Args) -> ExitCode`, que pide una
`Compilation` con un `goal` y la proyecta. No hay trait ni registro: la lista es el enum de
clap (Ley 8, una sola lista).

| Comando | Qué pide | Estado |
|---|---|---|
| `run`, `eval`, `repl` | `Executable` + `Runtime` | existe |
| `check` | `goal = Checked` | existe |
| `build` (`bytecode`, `exe`, `native`) | `Executable` | existe; `exe` nuevo |
| `test` | `Executable` con tests | existe, reescrito |
| `fmt` | `Ast`, sin checker | existe |
| `bench` | `Executable` + tiempos | existe, reescrito |
| `debug` | cualquier goal, `cache = Bypass` | existe |
| `task` | manifiesto, ejecuta otros comandos | **nuevo en esta tanda** |
| `pkg`, `init`, `cache`, `doctor`, `completions`, `lsp`, `dap`, `std-bundle` | `Project` | existe |
| `lint`, `doc`, `upgrade` | `Checked` / `Ast` / red | admitidos, no se construyen |

`task` entra porque es lo que convierte el manifiesto en algo que se usa a diario
(`vn task build`), y porque obliga a que un comando pueda invocar a otro sin lanzar un
proceso: `task` llama a `execute` de los demás con la misma `Session`.

## CLI

- `main.rs` solo crea el hilo con `VM_STACK_BYTES`, llama a `bootstrap` y despacha.
  `bootstrap`, `standalone` y el volcado de nativos (que pasa a `vn debug -p natives`) son
  módulos propios.
- `Target::Exe` completa el lector `VEXE` que ya existe: copia el ejecutable `vn` actual,
  le añade el grafo de módulos serializado y el trailer. `standalone` lee solo los últimos
  12 bytes. Es el equivalente a `deno compile`. Si el trailer no está, el arranque sigue
  normal; si está pero el grafo no se puede leer, es un error, no un fallo silencioso como
  hoy.
- Los nombres de subcomando salen del enum de clap; `implicit_run` consulta
  `Cli::command()` en vez de repetir la lista.
- Un toolchain tipo deno/bun es un binario con todo dentro: las features `lsp`, `dap` y `pm`
  dejan de ser opcionales y se borran junto con sus stubs `cfg(not(...))`. Quedan como
  features solo las de construcción (`clif`, `embed-std`, `dhat-heap`).
- `PermissionArgs` con `#[command(flatten)]` en `run`, `eval`, `repl`, `test`, `debug`;
  `Policy` se construye en un solo sitio. Sin ninguna bandera se concede todo, como hoy; que
  el valor por defecto sea restrictivo pasa a ser un cambio de una línea en `Policy`, no una
  reescritura.
- `build`: `--target` es un `ValueEnum { Bytecode, Exe, Native }`; se borra `--native`. La
  búsqueda del linker y de las librerías del sistema sale de `build.rs` y vive junto a
  `varn_jit::aot`, descubriendo el SDK por entorno y no por versiones fijas. En plataformas
  sin linker soportado el error lo dice, en vez de fallar buscando `lld-link`.
- Un recorrido de archivos `.vn` compartido por `fmt` y `test`. `fmt` usa el módulo
  `terminal`, sin escapes ANSI a mano.
- Los workers de `test` usan `VM_STACK_BYTES`.
- `inspect_lsp` se mueve a `varn-lsp` (solo consume sus tipos). El CLI conserva
  `vn debug -p lsp` como llamada.
- `compare_tiers` sigue lanzando procesos hijo: es una decisión de aislamiento por tier, no
  una ruta de compilación.

## LSP y DAP

- `varn-dap` deja de armar su propia sesión: usa `Session` y `Compilation` con `goal ==
  Executable`.
- `varn-lsp` se migra al final, pidiendo `mode == Tooling`. El LSP es la herramienta que
  hoy funciona mejor. Si `Session` no le sirve sin torcer su API (documentos abiertos
  incrementales, versiones), el defecto está en `Session`, no en el LSP: se rediseña la API
  y no se le añade un adaptador (Ley 8). Si tras ese intento el LSP no puede adoptarla, se
  registra con causa y se detiene la migración ahí.

## Orden de migración

Cada paso es un commit verde y atómico (Ley 9), y cada uno forma parte del diseño final:

1. `Failure` tipado y render en el borde; se borran las copias del pie de error.
2. `varn-project`: manifiesto único, raíz única, `.env`; el repo no contiene ningún manifiesto; solo `vn init` escribe la forma antigua y se corrige en el mismo paso.
3. `Session` dueña del resolver y del `Project`; todos los `ModuleResolver::new()` y
   `find_project_root` pasan a recibirla.
4. `Source` y lectura/canonicalización únicas.
5. `lower_module` único.
6. `Compilation` por etapas; `run`, `check`, `build`, `dap` migran; se borra `DebugSink`.
7. Caché uniforme.
8. `Executable` lleva los tests; runner de `vn test` reescrito; se borra `append`.
9. `Policy` y `PermissionArgs`.
10. Estructura del CLI: `main`, `bootstrap`, `standalone`, features fuera, `Target` con
    `Exe`, linker, recorrido de archivos.
11. `vn task`.
12. `bench` consume tiempos de `Compilation`; `inspect_lsp` pasa a `varn-lsp`.
13. LSP sobre `Session` en modo `Tooling`.

Tras cada paso: `verify.ps1 -Quick` y el hash del bytecode de `tests/` y `examples/`.
La matriz completa se corre antes de cada commit que cierra un paso de la lista, según la
nota de validación de `AGENTS.md`.

## Metas, no metas, anti-metas

**Metas**: los puntos de «Ganancia».

**No metas** (no se hacen en esta tanda, el diseño no los bloquea):

- `vn lint`, `vn doc`, `vn upgrade`. Necesitan su propio motor (reglas, extracción de
  documentación, distribución de releases). Se añaden como un módulo de comando más, con su
  sección en `varn.toml`.
- Cambiar la resolución de dependencias entre miembros de un workspace.
- REPL con estado entre líneas. Hoy cada línea es un programa aparte y `let x = 1` seguido de
  `x` falla. Arreglarlo exige un entorno incremental en el checker; es otro diseño.
- Cambiar los permisos por defecto a denegar (queda como cambio de una línea).
- Cambiar el formato de `.vnc`, `.vnb` o de la caché.

**Anti-metas**: un adaptador entre el `RunOpts` viejo y `Request`; conservar `append` o
`DebugSink` «hasta migrar»; que un test existente fije una conducta incorrecta de la CLI y
condicione el diseño (Ley 18: se rehace el test).

## Riesgos

- Los goldens de `crates/varn-cli/tests/` (`debug_golden`, `error_corpus`) dependen del
  formato actual de salida. Se migran en el mismo paso que el render pasa al borde; se
  declara cuántos son y se revisa el diff.
- `Compilation` retiene tokens, AST y TIR hasta que se descarte. En el CLI de un solo
  archivo es irrelevante; en el LSP puede importar. Se mide antes de adoptar `Compilation` en
  el LSP, y los artefactos intermedios se sueltan por etapa si hace falta.
- `Rc` en `CompileOutput`/`precompiled` impide enviar un `Executable` entre hilos. El runner
  de test compila una vez por archivo en el hilo que lo ejecuta y arranca las máquinas allí;
  si el reparto por archivo deja un hilo ocioso, se decide con una medición, no a ojo.

## Decisiones mías, a revisar

- Features `lsp`, `dap`, `pm` fuera: un toolchain es un binario con todo.
- Permisos por defecto sin cambio (todo concedido).
- `varn.toml` con campos planos y sin sección `package`.
- Crate nuevo `varn-project` como hoja.
- `Target::Exe` completa el trailer `VEXE`; el lector sin productor no se deja como está.
- `vn task` entra en esta tanda; `lint`, `doc` y `upgrade` no.
- `Session` como nombre del dueño único (podría ser `Toolchain` o `Workspace`).
- `inspect_lsp` en `varn-lsp` y no en `varn-debug`.
- El linker junto a `varn_jit::aot` y no en un crate nuevo.
- Orden de los pasos: errores y sesión primero porque todo lo demás los consume.
- La semántica de unidades de `vn test` se conserva tal cual (el módulo corre una vez por
  unidad).

## Pendiente de diseño: tests invocados por el runtime

El runner de `vn test` sigue ejecutando cada `@test` con `Source::appended`
(el módulo se compila con una llamada al final). Sustituirlo por
`Executable.tests` + `Runtime::run_test` exige tres piezas que hoy no existen
y que son un cimiento propio (Ley 14), no un detalle del CLI:

1. `varn_sem::output::TestTarget` debe llevar el `slot_idx` del símbolo (el
   checker ya lo conoce) para que el runtime ubique la función en la región de
   globales del módulo de entrada.
2. `Vm` necesita una llamada pública desde el host (`invoke` es `pub(crate)`)
   que ejecute una función global y, si devuelve una tarea, la espere con
   `settle_awaited`.
3. Un valor lanzado por un test asíncrono (`Err(thrown)`) debe convertirse en
   un `RuntimeError` con el mismo mensaje y pila que produce hoy el `await` de
   nivel superior; esa conversión vive hoy dentro del bucle de `drive_main`.

Verificación al implementarlo: mismas unidades aprobadas y fallidas en
`vn test`, mismos mensajes de fallo, y menos compilaciones por archivo con
`@test` (hoy 1 + N).
