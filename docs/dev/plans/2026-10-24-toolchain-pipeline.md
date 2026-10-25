# Toolchain: pipeline único, proyecto y comandos delgados — Plan de implementación

> **Para agentes:** SUB-SKILL REQUERIDA: usar superpowers:subagent-driven-development (recomendado) o superpowers:executing-plans para ejecutar este plan tarea por tarea. Los pasos usan casillas `- [ ]`.

**Meta:** que `vn` compile por un único camino (`Session::compile`), conozca su proyecto por un único modelo (`varn-project`) y que cada comando sea una proyección delgada de ambos.

**Arquitectura:** `Failure` tipado (sin ANSI ni exit codes en el pipeline) → crate hoja `varn-project` → `Session` dueña de `Project` + `ModuleResolver` → `Source` → `lower_module` único → `Compilation` por etapas (`Goal`/`Mode`/`CachePolicy`) que sustituye a `DebugSink` → `Executable` que lleva los `@test` → `Policy` → CLI reestructurado → `vn task` → bench/inspect → LSP.

**Stack:** Rust workspace (`crates/*`), clap 4, postcard, PowerShell para los scripts.

**Diseño:** `docs/dev/plans/2026-10-24-toolchain-pipeline-design.md`. Es la fuente de las decisiones; este plan no las repite, las secuencia.

## Restricciones globales

Valen para todas las tareas.

- **Ley 11:** cero comentarios de código, cero tests unitarios. La verificación es la suite `.vn`, `scripts/bytecode-hash.ps1` y `scripts/cli-smoke.ps1`. Los tests Rust existentes en `crates/varn-cli/tests/` se migran, no se amplían.
- **Ley 9:** un cambio, un commit. Un paso «Commit» crea exactamente un commit; las tareas con varios commits lo dicen. Mensaje terminado en `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Después de cada commit: `tgrep index .` en la raíz.
- **Ley 8 y regla de migración:** borrar, no puentear. Ningún adaptador, flag dual ni alias hacia la API vieja. El compilador marca los sitios pendientes; se migran todos en la misma tarea.
- **Ley 4:** `BTreeMap`/`IndexMap` o `Vec` ordenado donde el orden decida algo. Ley 7: ningún `_ =>` nuevo.
- **Ley 16:** exit codes y nombres de identidad en una sola tabla; sin literales repetidos.
- **Ley 13:** se itera con `cargo build --profile quick --bin vn`. La matriz completa se corre una sola vez, al final (Tarea 14), y antes de cualquier push.
- **Límite de archivo:** 400 líneas por archivo; se divide por dominio.
- **Raíz limpia:** nada suelto en la raíz del repo. Temporales en `target/tmp/`.
- **Nunca `git add` de directorios:** el usuario puede tener cambios sin commitear. Se añaden archivos por nombre.
- **Naming:** la salida compilada se llama `Executable` (la `Program` del diseño choca con `varn_core::ast::Program`).

**Gate de tarea (G1)**, tras cada tarea y antes de su commit:

```powershell
cargo build --profile quick --bin vn
.\scripts\verify.ps1 -Quick
.\scripts\bytecode-hash.ps1 -Compare
.\scripts\cli-smoke.ps1
```

Esperado: build sin warnings nuevos; `verify.ps1 -Quick` con `FAILED: 0`; `bytecode-hash` con `IDENTICAL`; `cli-smoke` con `ALL CHECKS PASSED`. Las tareas que cambian el bytecode a propósito lo dicen (ninguna debería).

## Review Focus

Entradas que el diseño implica y que ninguna tarea cubriría sin pedirlo. Cada una se fija como un check con nombre en `scripts/cli-smoke.ps1`, en la tarea dueña.

1. `vn task a` donde la tarea `a` invoca `task a`: debe dar un error de recursión, no un desbordamiento de pila (Tarea 12).
2. Varios workers de `vn test` escribiendo la misma entrada de caché a la vez: ninguna lectura debe ver un archivo a medias (Tarea 8).
3. `varn.toml` con una clave desconocida: error que nombra la clave y el archivo (Tarea 3).
4. `vn eval "let ="` (error de sintaxis sin archivo): el diagnóstico se muestra con el nombre virtual y exit code 3 (Tarea 2).
5. Ejecutable `--target exe` con trailer `VEXE` pero grafo corrupto: error explícito con exit code distinto de 0, no arranque silencioso del `vn` normal (Tarea 11).

---

## Tarea 0: Línea base ejecutable

**Archivos:**
- Crear: `scripts/bytecode-hash.ps1`, `scripts/cli-smoke.ps1`
- Crear: `target/tmp/baseline/` (no versionado)

**Interfaces:**
- Produce: `bytecode-hash.ps1 [-Record | -Compare]`; `cli-smoke.ps1` que imprime una línea `PASS <nombre>` o `FAIL <nombre>` por check y termina con `ALL CHECKS PASSED` o exit code 1.

- [ ] **Paso 1: `bytecode-hash.ps1`.** Recorre `tests/**/*.vn` y `examples/**/*.vn`, ejecuta `target\quick\vn.exe build <archivo> -o target\tmp\bytecode\<ruta-plana>.vnc`, y escribe `ruta<TAB>hash` ordenado por ruta. `-Record` guarda en `target/tmp/baseline/bytecode.tsv`; `-Compare` lo regenera y compara, imprimiendo `IDENTICAL` o cada diferencia. Los archivos que no compilan hoy se registran como `ERROR` en la base y se comparan como tales.
- [ ] **Paso 2: comprobar determinismo del script.** Correr `-Record` dos veces seguidas sobre el mismo binario y comparar. Esperado: `IDENTICAL`. Si no lo es, el bytecode actual no es determinista (Ley 4): se registra con repro mínimo y se arregla antes de continuar, porque el resto del plan depende de esta señal.
- [ ] **Paso 3: `cli-smoke.ps1`.** Esqueleto con una función `Check <nombre> <bloque>` y los checks de hoy que no dependen del diseño: `run` de un `.vn` con salida conocida, `check` de un archivo con error (exit code 3), `eval "1+1"`, `fmt --check` sobre un archivo formateado, `test` sobre un directorio con un archivo que pasa y otro que falla (exit code distinto de 0).
- [ ] **Paso 4: registrar la base.** `cargo build --profile quick --bin vn`, `.\scripts\verify.ps1 -Quick`, `.\scripts\bytecode-hash.ps1 -Record`. Anotar en el mensaje del commit el `PASSED: N` de `-Quick` y el número de archivos hasheados. Además registrar en `target/tmp/baseline/test.txt` la salida de `vn test` (pasadas por unidad) y el tiempo de pared de tres corridas.
- [ ] **Paso 5: Commit** `scripts/bytecode-hash.ps1`, `scripts/cli-smoke.ps1`: `scripts: executable baseline for bytecode determinism and CLI behavior`.

---

## Tarea 1: `Failure` tipado y render en el borde

**Archivos:**
- Modificar: `crates/varn-pipeline/src/error.rs`, `lex.rs`, `parse.rs`, `check.rs`, `execute.rs`, `lib.rs`
- Crear: `crates/varn-cli/src/render.rs`, `crates/varn-cli/src/exit.rs`
- Modificar: `crates/varn-cli/src/error.rs`, `main.rs`, todos los `commands/*.rs` que usen `PipelineError`/`CliError`
- Modificar: `crates/varn-dap/src/driver.rs`, `crates/varn-lsp` donde use `PipelineError`

**Interfaces:**
- Produce en `varn-pipeline`:
  ```rust
  pub enum Failure {
      Diagnostics(DiagnosticReport),
      Runtime(RuntimeFailure),
      Io(String),
      Internal(String),
  }
  pub struct DiagnosticReport { pub file: Arc<str>, pub source: Arc<str>, pub diagnostics: DiagnosticBag }
  pub struct RuntimeFailure { pub message: String, pub frames: Vec<FrameInfo> }
  ```
  `PipelineError` desaparece.
- Produce en `varn-cli`: `exit.rs` con `pub enum ExitCode { Fatal = 1, Usage = 2, Diagnostics = 3 }` y `impl From<&Failure> for ExitCode`; `render.rs` con `pub fn failure(f: &Failure) -> String`, que contiene el único pie «could not compile `{file}` due to {n} previous error(s)».

- [ ] **Paso 1:** Definir `Failure` y reemplazar `PipelineError` en `varn-pipeline`. Los errores de lexer y parser pasan a acumularse en un `DiagnosticBag` (Ley 5); `check::report_diagnostics` deja de formatear y devuelve el `DiagnosticReport` cuando hay errores. `format_runtime_error` de `execute.rs` pasa a `render.rs` y `execute` devuelve `Failure::Runtime`.
- [ ] **Paso 2:** Crear `exit.rs` y `render.rs` en el CLI. `CliError` se reduce a `{ failure: Failure | Usage(String), exit: ExitCode }` o desaparece si `Failure` basta; la regla es que solo el CLI conozca los exit codes. `main.rs` imprime con `render::failure` y sale con el `ExitCode`; la comparación `e.exit_code == 3` se borra.
- [ ] **Paso 3:** Migrar `varn-dap` y `varn-lsp` a `Failure`. Migrar `crates/varn-cli/tests/{error_corpus,debug_golden}.rs`: el formato de salida no cambia, así que deben pasar sin editar el esperado; si algún golden cambia, el diff se revisa y se declara cuántos eran.
- [ ] **Paso 4: check `eval-syntax-error`** en `cli-smoke.ps1`: `vn eval "let ="` termina con exit code 3 y su salida contiene `(eval)` y `could not compile`.
- [ ] **Paso 5:** G1. Commit: `pipeline: typed Failure, diagnostics rendered and exit codes decided at the CLI edge`.

---

## Tarea 2: Crate `varn-project`

**Archivos:**
- Crear: `crates/varn-project/{Cargo.toml,src/lib.rs,src/manifest.rs,src/root.rs,src/env.rs}`
- Modificar: `Cargo.toml` (workspace), `crates/varn-pm/src/manifest.rs` (queda solo `DepOrigin`), `crates/varn-modules/src/artifact.rs` (se va `find_project_root`), `crates/varn-cli/src/env_file.rs` (se borra), `crates/varn-modules/src/std_root.rs`, `crates/varn-resolver/src/graph.rs`, `crates/varn-cli/src/commands/cache.rs`, `crates/varn-cli/src/doctor_impl.rs`, `crates/varn-pipeline/src/cache.rs`, `crates/varn-lsp/src/pipeline/mod.rs`

**Interfaces:**
- Produce:
  ```rust
  pub enum Anchor<'a> { File(&'a Path), Cwd }
  pub struct Project { pub root: PathBuf, pub manifest: Manifest, pub env: BTreeMap<String, String> }
  impl Project { pub fn discover(anchor: Anchor<'_>) -> Result<Project, ProjectError> }
  #[serde(deny_unknown_fields)]
  pub struct Manifest {
      pub name: Option<String>, pub version: Option<String>, pub main: Option<String>,
      pub std: Option<String>,
      pub exports: BTreeMap<String, String>,
      pub dependencies: BTreeMap<String, String>,
      pub dev_dependencies: BTreeMap<String, String>,
      pub peer_dependencies: BTreeMap<String, String>,
      pub workspaces: Vec<String>,
      pub tasks: BTreeMap<String, Vec<String>>,
      pub fmt: FileFilter,
      pub test: FileFilter,
  }
  pub struct FileFilter { pub include: Vec<String>, pub exclude: Vec<String> }
  ```
  `ProjectError` incluye la clave desconocida y el archivo.
- Consume: nada (hoja; `serde`, `serde_json`, std).

- [ ] **Paso 1:** Inventario. `tgrep -n -F -- "varn.toml" .` y `tgrep -n -- "find_project_root|project_root_cached|ProjectManifest"` sobre `crates/`. El repo no contiene manifiestos; el manifiesto se parsea hoy en `varn-pm::ProjectManifest`, `varn-modules::package` y `std_root::project_std_override`, y esos tres lectores se reemplazan por `Manifest`. `vn init` deja de escribir `[package]`.
- [ ] **Paso 2:** Crear el crate. La regla de descubrimiento es la actual de `find_project_root` (mismos marcadores), parametrizada por `Anchor`; `Anchor::File` parte del directorio del archivo y `Anchor::Cwd` del cwd. El `.env` se lee del `root` con la lógica de `env_file.rs`. La sección `package` desaparece; los campos planos son los únicos.
- [ ] **Paso 3:** Reemplazar los seis sitios de búsqueda de raíz por `Project`. `varn-modules`, `varn-pm`, `varn-pipeline`, `varn-resolver` y `varn-cli` dependen de `varn-project`. `ProjectManifest` de `varn-pm` se borra y `varn-pm` usa `Manifest`.
- [ ] **Paso 4: check `manifest-unknown-key`**: en un directorio temporal, `varn.toml` con la línea `nmae = "x"` y un `.vn`; `vn run` debe fallar y mostrar `nmae` y la ruta del `varn.toml`.
- [ ] **Paso 5:** G1. Commit: `project: one manifest shape and one root discovery in a leaf crate`.

---

## Tarea 3: `Session`

**Archivos:**
- Crear: `crates/varn-pipeline/src/session.rs`
- Modificar: `crates/varn-resolver/src/module_resolver.rs` (constructor), todos los sitios que llaman `ModuleResolver::new()`: `varn-cli/src/{tester.rs,commands/build.rs,commands/std_bundle.rs,bench/source/mod.rs}`, `varn-pipeline/src/{lib.rs,stdlib_loader.rs}`, `varn-dap/src/driver.rs`, `varn-cli/src/inspect_lsp/*`
- Modificar: `crates/varn-cli/src/main.rs` (abre la `Session` y la pasa)

**Interfaces:**
- Consume: `Project`, `Anchor`, `Failure`.
- Produce:
  ```rust
  pub struct Session { /* project, resolver */ }
  impl Session {
      pub fn open(anchor: Anchor<'_>) -> Result<Session, Failure>;
      pub fn project(&self) -> &Project;
      pub fn resolver(&self) -> &ModuleResolver;
  }
  ```
  `ModuleResolver::new(project: &Project) -> ModuleResolver`; sin constructor sin proyecto.

- [ ] **Paso 1:** Cambiar el constructor de `ModuleResolver` y dejar que el compilador marque los sitios. Cada uno recibe `&Session` en vez de construir.
- [ ] **Paso 2:** `main` abre la `Session` una vez por proceso con el ancla del comando (`Anchor::File` para los comandos con archivo, `Anchor::Cwd` para los de proyecto). `tester.rs` deja de crear un resolver por unidad: lo comparte entre workers (`&Session` debe ser `Sync`; si el resolver no lo es, cada worker abre su `Session` con el mismo `Project` y la tarea lo declara).
- [ ] **Paso 3:** `PipelineLoader::new()` de `stdlib_loader.rs` recibe la `Session`.
- [ ] **Paso 4:** G1. Commit: `pipeline: Session owns the Project and the ModuleResolver`.

---

## Tarea 4: `Source`

**Archivos:**
- Crear: `crates/varn-pipeline/src/source.rs`
- Modificar: `lib.rs` (`read_source`, `canonicalize_path`, `read_source_file` se borran), `varn-cli/src/commands/{build,run,eval,repl,check,debug}.rs`, `varn-cli/src/tester.rs`, `varn-dap/src/driver.rs`

**Interfaces:**
- Produce:
  ```rust
  pub enum Source {
      File { path: Arc<str>, text: Arc<str> },
      Virtual { name: Arc<str>, text: Arc<str> },
  }
  impl Source {
      pub fn file(session: &Session, path: &str) -> Result<Source, Failure>;
      pub fn virtual_(name: &str, text: String) -> Source;
      pub fn path(&self) -> &str;
      pub fn text(&self) -> &str;
  }
  ```
  `Source::file` canonicaliza una sola vez y lee por el registro de loaders (el camino que hoy usa `read_source`).

- [ ] **Paso 1:** Implementar y reemplazar `std::fs::read_to_string` + `canonicalize` repetidos en `build`, `dap`, `tester` y `lib.rs`. Los nombres virtuales `"(eval)"` y `"(repl)"` pasan a constantes de una tabla en `varn-cli` (Ley 16).
- [ ] **Paso 2:** G1. Commit: `pipeline: Source is the single identity and reader of what gets compiled`.

---

## Tarea 5: `lower_module` único

**Archivos:**
- Crear: `crates/varn-pipeline/src/lower.rs`
- Modificar: `compile.rs` (pierde `emit_tir`, `inject_star_reexports`, `resolve_import_slots`, `sorted_export_names`, `report_export_collisions`, `tir_import_sources`, `compile_tir`, que pasan a `lower.rs`), `module_precompile.rs:206-249`, `stdlib_loader.rs:115-153`

**Interfaces:**
- Produce:
  ```rust
  pub struct LowerInput<'a> {
      pub program: &'a ast::Program, pub arena: &'a AstArena, pub check: &'a varn_sem::output::CheckResult,
      pub exports: &'a ExportMap, pub resolver: &'a dyn ImportResolver, pub source: &'a str,
  }
  pub struct Lowered { pub tir: TirModule, pub proto: FunctionProto }
  pub fn lower_module(input: LowerInput<'_>) -> Result<Lowered, Failure>
  ```

- [ ] **Paso 1:** Comparar las tres secuencias lado a lado (`compile.rs:165-185`, `module_precompile.rs:224-249`, `stdlib_loader.rs:141-153`) y anotar cada paso que exista en una y no en otra (`verify_synthetic_imports` solo está en la entrada, por ejemplo). Cada diferencia es una de dos: un bug de la copia que carece del paso, o una diferencia legítima. Las legítimas se expresan con un campo de `LowerInput` con tipo, no con un flag; las otras se arreglan en la misma tarea (Ley 15), y si cambian el bytecode se declara y se re-registra la base.
- [ ] **Paso 2:** Implementar `lower_module` y sustituir las tres secuencias. Borrar las funciones antiguas de `compile.rs` y sus re-exports de `lib.rs`.
- [ ] **Paso 3:** G1. Commit: `pipeline: one module lowering used by the entry, the module graph and the stdlib loader`.

---

## Tarea 6: `Compilation` por etapas

Es la tarea más grande. Tiene cuatro commits.

**Archivos:**
- Crear: `crates/varn-pipeline/src/{request.rs,compilation.rs,executable.rs}`
- Modificar: `lex.rs`, `parse.rs`, `check.rs`, `compile.rs`, `lib.rs`, `cache.rs`
- Borrar: `debug_sink.rs`, `varn-cli/src/debug_sink.rs` (la parte `CliDebugSink`)
- Modificar: `varn-cli/src/commands/{run,check,eval,repl,debug,build}.rs`, `varn-cli/src/debug_print.rs` (nuevo), `varn-dap/src/driver.rs`

**Interfaces:**
- Consume: `Session`, `Source`, `Failure`, `lower_module`.
- Produce:
  ```rust
  #[derive(PartialEq, Eq, PartialOrd, Ord)]
  pub enum Goal { Tokens, Ast, Checked, Tir, Executable }
  pub enum Mode { Strict, Tooling }
  pub enum CachePolicy { Use, Bypass }
  pub struct Request { pub goal: Goal, pub mode: Mode, pub cache: CachePolicy }
  pub enum Stage { Lex, Parse, Check, Lower, Link }
  pub struct Compilation {
      pub source: Source,
      pub tokens: Option<Tokens>,
      pub parsed: Option<Parsed>,
      pub checked: Option<CheckResult>,
      pub tir: Option<TirModule>,
      pub executable: Option<Executable>,
      pub diagnostics: DiagnosticBag,
      pub timings: Vec<(Stage, Duration)>,
      pub cache_hit: bool,
  }
  impl Compilation {
      pub fn failure(&self) -> Option<Failure>;
      pub fn into_executable(self) -> Result<Executable, Failure>;
  }
  impl Session { pub fn compile(&self, source: Source, request: Request) -> Compilation }
  pub struct Executable {
      pub entry: FunctionProto,
      pub precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
      pub graph: ModuleGraphArtifact,
      pub graph_build: ModuleGraphBuild,
      pub tests: Vec<TestEntry>,
  }
  ```
  `Executable` sustituye a `CompileOutput`. `Mode::Strict` usa `CheckOptions::compile` y `varn_parser::parse`; `Mode::Tooling` usa `CheckOptions::tooling` y `parse_partial`. `tests` queda vacío hasta la Tarea 9.

- [ ] **Paso 1 (commit 1):** `Compilation`, `Request`, `Executable` y `Session::compile` para `Goal::Tokens..=Goal::Checked`. Las funciones `lex`, `parse`, `check` pasan a ser etapas internas que no imprimen ni llaman sinks; el `verbose` desaparece del pipeline. Commit: `pipeline: Session::compile with staged Compilation up to the checker`.
- [ ] **Paso 2 (commit 2):** `Goal::Tir` y `Goal::Executable` (lower + grafo + verificación de clases, hoy en `compile::compile`). Commit: `pipeline: Compilation reaches TIR and Executable`.
- [ ] **Paso 3 (commit 3):** Migrar `run`, `check`, `eval`, `repl`, `build` y `dap`. `run`/`check` ya no pasan por `RunOpts`: arman un `Request` y, para ejecutar, llaman a `Runtime` (que se queda en `execute.rs` con la firma `Runtime::boot(&Executable, &Policy)`; la `Policy` real llega en la Tarea 10, aquí se pasa `CapabilitySet::allow_all()` como hoy). `RunOpts`, `compile_source_for_build` y `execute` se borran. Commit: `pipeline: commands compile through Request; RunOpts removed`.
- [ ] **Paso 4 (commit 4):** `vn debug` y `--verbose`. `varn-cli/src/debug_print.rs` recibe `&Compilation` y `&DebugFlags` y llama a las funciones de `varn_debug` que hoy invoca `CliDebugSink`; imprime incluso cuando una etapa falló, con lo producido hasta ahí. `debug.rs` pide el `Goal` mínimo que cubra las fases pedidas y `CachePolicy::Bypass`. Los mensajes `--verbose` salen de `timings`, `tokens` y `cache_hit`. Borrar `DebugSink`, `NullSink`, `CliDebugSink`. Verificar a mano `vn debug -p tokens|ast|symbols|check|bytecode` contra la base de `debug_golden`. Commit: `pipeline: debug output is a consumer of Compilation; DebugSink removed`.
- [ ] **Paso 5:** G1 tras cada commit (los goldens de `debug_golden.rs` deben pasar sin cambios).

---

## Tarea 7: Caché uniforme

**Archivos:**
- Modificar: `crates/varn-pipeline/src/cache.rs`, `compilation.rs`

**Interfaces:**
- Consume: `Request.cache`, `Source::File`.
- Produce: `Session::compile` consulta la caché cuando `goal == Executable && cache == Use && source es File`, y escribe tras un fallo de caché. La clave incluye ruta canónica, identidad de la std, `cache_key()` y el `Mode`.

- [ ] **Paso 1:** Mover `compile_source_cached` dentro de `Session::compile`; `Compilation.cache_hit` se rellena. `run`, `test`, `build` y `dap` pasan a usar la caché sin cambios por su lado. La escritura es atómica: a un archivo temporal en el mismo directorio y `rename`, para que ninguna lectura vea un archivo a medias.
- [ ] **Paso 2: check `cache-hit-on-second-run`**: `vn run -v` sobre un `.vn` en un directorio temporal dos veces; la segunda salida contiene `compile cache hit`.
- [ ] **Paso 3:** G1. Además, `bytecode-hash.ps1 -Compare` con caché fría (`VARN_CACHE_DIR=<temp>`) y caliente: ambas `IDENTICAL`. Commit: `pipeline: the compile cache lives in Session::compile for every command`.

---

## Tarea 8: Escritura de caché concurrente

Va separada de la 7 porque es la comprobación de un modo de fallo, con su propio commit si exige cambios.

- [ ] **Paso 1: check `cache-concurrent-writers`** en `cli-smoke.ps1`: lanzar 8 procesos `vn run` simultáneos sobre el mismo `.vn` con caché fría y comprobar que todos terminan con el mismo stdout y exit code 0.
- [ ] **Paso 2:** Si el check falla, corregir la causa en `write_artifact_file` (de `varn-modules`) y registrar la evidencia. Si pasa a la primera, no hay commit y se anota en el mensaje de la Tarea 9.

---

## Tarea 9: `Executable` lleva los tests

**Archivos:**
- Modificar: `crates/varn-pipeline/src/{executable.rs,check.rs,execute.rs}`, `crates/varn-cli/src/tester.rs`, `crates/varn-cli/src/commands/test.rs`
- Borrar: `collect_test_targets`, el campo `append`, `test_units` y la concatenación de texto

**Interfaces:**
- Produce:
  ```rust
  pub struct TestEntry { pub name: Arc<str>, pub is_async: bool }
  impl Runtime { pub fn run_test(&mut self, entry: &TestEntry) -> Result<(), Failure> }
  ```
  `Executable.tests` se rellena desde `CheckResult::test_targets` en la etapa de enlace.

- [ ] **Paso 1:** Investigar cómo invocar una función de módulo por nombre desde `Runtime` (`varn_vm::debug::drive_main` y la API de `Vm`); si los `@test` no son exportables hoy, lo correcto es que `lower_module` los registre en una tabla del módulo (cimiento, Ley 14), no que el runner los busque por nombre en texto.
- [ ] **Paso 2:** Reescribir `tester.rs`: por archivo, `Session::compile(Goal::Executable)` una vez; la unidad de archivo ejecuta el módulo; cada unidad `@test` arranca una máquina, ejecuta el módulo e invoca `run_test`. La semántica de unidades se conserva. Los workers se crean con `stack_size(VM_STACK_BYTES)`.
- [ ] **Paso 3:** Medir contra la base de la Tarea 0: mismas pasadas por unidad en `vn test`, tiempo de pared menor o igual. Declarar el número en el mensaje.
- [ ] **Paso 4:** G1. Commit: `test: the compiler registers @test entries; vn test compiles each file once`.

---

## Tarea 10: `Policy` y `PermissionArgs`

**Archivos:**
- Crear: `crates/varn-cli/src/policy.rs`
- Modificar: `cli.rs` (`RunArgs` pierde los siete `allow_*` y la bandera `--sandbox`, que pasan a `PermissionArgs`), `commands/{run,eval,repl,test,debug,bench}.rs`, `varn-pipeline/src/execute.rs`

**Interfaces:**
- Produce:
  ```rust
  #[derive(Args)]
  pub struct PermissionArgs { /* allow_read, allow_write, allow_net, allow_env, allow_ffi, allow_ui, allow_all, sandbox */ }
  pub struct Policy { pub capabilities: CapabilitySet, pub trace: bool, pub gc: bool, pub silent: bool }
  impl PermissionArgs { pub fn capabilities(&self) -> CapabilitySet }
  ```
  `Runtime::boot(&Executable, &Policy)`.

- [ ] **Paso 1:** Mover `build_capabilities` de `run.rs` a `policy.rs` sin cambiar su semántica (sin ninguna bandera se concede todo; con alguna, solo lo pedido). `#[command(flatten)]` en `run`, `eval`, `repl`, `test`, `debug`, `bench`.
- [ ] **Paso 2: check `eval-honors-permissions`**: `vn eval "…lectura de archivo…" --allow-net` falla por falta de `--allow-read`, y con `--allow-read` pasa.
- [ ] **Paso 3:** G1. Commit: `cli: one Policy and one set of permission flags for every executing command`.

---

## Tarea 11: Estructura del CLI y `--target exe`

Cinco commits.

**Archivos:**
- Crear: `crates/varn-cli/src/{bootstrap.rs,standalone.rs}`, `crates/varn-cli/src/discover.rs`
- Modificar: `main.rs`, `cli.rs`, `Cargo.toml` (features), `commands/{build,fmt,test,debug}.rs`, `formatter/mod.rs`, `tester.rs`
- Mover: la búsqueda de linker de `commands/build.rs` a `crates/varn-jit/src/aot/link.rs`

**Interfaces:**
- Produce: `discover::vn_files(root: &Path, filter: &FileFilter) -> Result<Vec<PathBuf>, Failure>` (un recorrido para `fmt` y `test`, ordenado, que respeta `include`/`exclude` del manifiesto y salta `.`-dirs, `target`, `node_modules`); `#[derive(ValueEnum)] enum Target { Bytecode, Exe, Native }`; `varn_jit::aot::link::link_executable(obj: &Path, out: &Path) -> Result<(), LinkError>`; `standalone::try_run() -> Option<ExitCode>`.

- [ ] **Paso 1 (commit):** `main.rs` queda en hilo + `bootstrap::init()` + dispatch. El volcado `VARN_DEBUG_OPS` pasa a `vn debug -p natives`. `implicit_run` consulta `Cli::command().get_subcommands()`. Commit: `cli: main is only thread, bootstrap and dispatch; subcommand list comes from clap`.
- [ ] **Paso 2 (commit):** Features `lsp`, `dap`, `pm` fuera; se borran los `cfg` y los stubs de `dispatch`. Quedan `clif`, `embed-std`, `dhat-heap`. Commit: `cli: lsp, dap and pm are always built in`.
- [ ] **Paso 3 (commit):** `discover::vn_files` reemplaza `discover_vn_files` y `discover_test_files`; `fmt` usa el módulo `terminal` en vez de escapes ANSI. Commit: `cli: one file discovery for fmt and test honoring the manifest filters`.
- [ ] **Paso 4 (commit):** `Target` como `ValueEnum`; se borra `--native`. El linker se muda a `varn_jit::aot::link`, descubre las librerías por entorno (`LIB`, `vswhere`) en vez de versiones fijas del SDK, y en plataformas sin linker soportado devuelve un `LinkError` que lo dice. Commit: `build: Target enum, linker discovery lives with the AOT backend`.
- [ ] **Paso 5 (commit):** `Target::Exe`: copia `current_exe`, añade el grafo serializado (`ModuleGraphArtifact` con `write_artifact`), `payload_len` (u64 LE) y `MAGIC_VEXE`. `standalone::try_run` lee los últimos 12 bytes; trailer ausente → `None`; trailer presente y grafo ilegible → `Some(ExitCode::Fatal)` con mensaje. **Check `exe-corrupt-trailer`**: tomar un exe generado, truncar el grafo conservando el trailer y comprobar exit code distinto de 0. **Check `exe-runs-elsewhere`**: ejecutar el exe desde otro cwd con argumentos. Commit: `build: --target exe writes the VEXE trailer the standalone reader already expected`.
- [ ] **Paso 6:** G1 tras cada commit.

---

## Tarea 12: `vn task`

**Archivos:**
- Crear: `crates/varn-cli/src/commands/task.rs`
- Modificar: `cli.rs`, `commands/mod.rs`, `Cargo.toml` (`shlex`)

**Interfaces:**
- Consume: `Manifest.tasks: BTreeMap<String, Vec<String>>`, `Session`.
- Produce: `vn task` lista las tareas ordenadas; `vn task <nombre> [args…]` ejecuta cada línea en orden, cada línea es una línea de comando de `vn` (sin el `vn`), partida con `shlex`, parseada con `Cli::try_parse_from` y despachada en proceso con la misma `Session`. La primera falla corta la secuencia y su `ExitCode` es el de `vn task`.
- Un `execute` de comando devuelve `Result<(), Failure>`; la pila de tareas en curso viaja en un parámetro y una tarea ya presente en la pila da `Failure::Usage("task cycle: a → b → a")`.

- [ ] **Paso 1:** Implementar. Una tarea desconocida lista las existentes. Los argumentos extra de `vn task <n> …` se añaden a la última línea.
- [ ] **Paso 2: checks** `task-runs-in-order`, `task-stops-on-failure` y `task-cycle-detected` (la tarea `a` con la línea `task a`) en `cli-smoke.ps1`, con un `varn.toml` temporal.
- [ ] **Paso 3:** G1. Commit: `cli: vn task runs manifest tasks in-process`.

---

## Tarea 13: `bench` e `inspect_lsp`

Dos commits.

**Archivos:**
- Modificar: `crates/varn-cli/src/bench/source/{mod.rs,e2e.rs,bench_pipeline.rs}`, `bench/harness.rs`
- Mover: `crates/varn-cli/src/inspect_lsp/*` → `crates/varn-lsp/src/inspect/*`

**Interfaces:**
- Consume: `Compilation.timings`.
- Produce: `varn_lsp::inspect::run_for(file: &str, eval: Option<&str>, debug: &DebugFlags)`.

- [ ] **Paso 1 (commit):** `bench/source` deja de llamar `phase_lex`, `phase_parse`, `Checker::check_with`, `emit_tir`, `compile_tir`; lee los tiempos de `Compilation` con `CachePolicy::Bypass` y `Goal::Executable`. Los reportes conservan sus columnas. Commit: `bench: compile-phase timings come from Compilation, no reimplemented phases`.
- [ ] **Paso 2 (commit):** Mover `inspect_lsp` a `varn-lsp`; `vn debug -p lsp` llama a `varn_lsp::inspect::run_for`. `varn-cli` deja de depender de `varn-resolver` y `tower-lsp-f` directamente. Commit: `lsp: the inspector lives with the LSP it inspects`.
- [ ] **Paso 3:** G1 tras cada commit; `docs/CLI_INSPECT.md` se actualiza si cita la ruta.

---

## Tarea 14: LSP sobre `Session`, y cierre

**Archivos:**
- Modificar: `crates/varn-lsp/src/pipeline/mod.rs`, `crates/varn-lsp/src/workspace/mod.rs`
- Modificar: `docs/CLI_REFERENCE.md`, `CHANGELOG.md`, `docs/dev/plans/2026-10-16-plan-unificado.md` (estado)

**Interfaces:**
- Consume: `Session::compile` con `Mode::Tooling`, `Goal::Checked`, `CachePolicy::Bypass`.

- [ ] **Paso 1:** Medir antes: memoria residente del LSP con el workspace del repo abierto y latencia de diagnósticos de `vn debug -p lsp` sobre `tests/main.vn` (mediana de 10). Guardar en `target/tmp/baseline/lsp.txt`.
- [ ] **Paso 2:** `workspace` guarda una `Session` por workspace y `run_pipeline` pide `Compilation`. Los datos del documento (`DocumentAnalysis`) se construyen desde `tokens`, `parsed` y `checked` retenidos. Si `Session` obliga a torcer el LSP (documentos abiertos versionados, resolver compartido por `Arc`), el defecto está en `Session`: se rediseña la API en esta misma tarea y se corrigen todos los consumidores. Si tras eso el LSP no puede adoptarla, se detiene aquí, se registra con causa y se deja el LSP sin migrar.
- [ ] **Paso 3:** Medir después, mismo método. Aceptar solo si memoria y latencia no empeoran; si empeoran, soltar artefactos intermedios por etapa en `Compilation` antes de continuar.
- [ ] **Paso 4:** G1. Commit: `lsp: documents are analyzed through Session::compile in Tooling mode`.
- [ ] **Paso 5: cierre.** Correr la matriz completa una vez: `.\scripts\verify.ps1 -Fast`. Esperado: `PASSED: N`, `FAILED: 0`, `ALL TESTS PASSED` en los 4 cuadrantes. Comparar `vn test` y tiempos con la base de la Tarea 0 y anotarlos en el mensaje.
- [ ] **Paso 6:** Actualizar `docs/CLI_REFERENCE.md` (`task`, `--target exe`, permisos, manifiesto único), `CHANGELOG.md` (los incompatibles: `--native`, sección `package`, features) y el estado del plan unificado. Commit: `docs: CLI reference and changelog for the toolchain pipeline`.

---

## Auto-revisión

- **Cobertura del diseño:** Failure (T1), Project/manifiesto/raíz (T2), Session (T3), Source (T4), `lower_module` (T5), Compilation/Goal/Mode/DebugSink fuera/tiempos (T6), caché (T7–T8), tests en `Executable` y `append` fuera (T9), Policy (T10), `main`/features/`Target`/linker/recorrido/`Exe` (T11), `task` (T12), bench e inspect (T13), LSP y DAP (T6 paso 3 para DAP, T14 para LSP).
- **No cubierto, por diseño:** `lint`, `doc`, `upgrade`, REPL con estado, permisos restrictivos por defecto.
- **Riesgos que el plan mide, no supone:** determinismo del bytecode (T0), goldens de salida (T1, T6), tiempo de `vn test` (T9), memoria y latencia del LSP (T14).
- **Dependencias entre tareas:** T1 antes que todo (tipo de error); T2 antes de T3; T3 antes de T4; T5 antes de T6; T6 antes de T7, T9, T10 y T13; T10 antes de T12.

---

## Estado final y desviaciones (2026-10-24)

Todas las tareas están ejecutadas; los pasos marcados `[ ]` arriba son el texto original del
plan. Lo que cambió respecto a él, con la razón (el ledger de ejecución no se versiona):

- **El manifiesto es TOML.** El diseño asumía JSON por un mensaje obsoleto del LSP; el código
  tenía tres parsers, el alias `vn.toml` y tres formas de nombre.
- **Cache dirs sin raíz.** `get_cache_dir` ignoraba su argumento; se borró el argumento y la
  raíz perezosa del grafo en vez de pasar un `Project` a `ModuleResolver::new`.
- **`Source::file` sin `Session`** (no lo necesita); lee por el registro y canonicaliza una vez.
- **`vn check`** usa `Goal::Checked`: tipos de la entrada y de todas sus dependencias
  (`check_module_graph`), sin generar código; los errores de emisión y de enlace quedan en
  `run`/`build`. Acepta varios archivos y directorios. No hay `Goal::Tir` (sin consumidor).
- **`Mode::Tooling` se sustituyó por `Session::analyze`**, que devuelve un `Analysis` tipado en
  lugar de una `Compilation` con `Option` en cada etapa.
- **El renderizado vive en `varn-core`** (`render_report`), con `Display` para `Failure`; el CLI
  decide los exit codes.
- **Medición del optimizador opt-in** (`Request::measured`), usada por `vn bench`.
- **`varn-inspect`** como crate propio: el inspector usa `varn-lsp` y `varn-debug`.
- **Ejecutable `--target exe`** lleva la imagen de la std que usa y no lee `.env`; el trailer es
  `[graph_len u64][image_len u64][VEXE]`.
- **Tests invocados por el runtime** (Tarea 9) se hicieron en una segunda pasada: grafo con
  `tests`, `Vm::call_main_global`, `execute_unit`.
- **Arranque anclado en el cwd** (`.env` y `std` de `varn.toml`), ver «Decisión» en el diseño.
- **Deuda aceptada:** la fábrica de máquinas de `vn bench` repite parte de `boot_machine`; los
  comandos `pkg` buscan el manifiesto con `project_manifest` (salta paquetes instalados) y no
  reciben `Session`; el LSP usa `Session::detached()` (el `Project` no se usa en su análisis);
  un manifiesto inválido en un directorio ancestro se ignora al resolver paquetes.
