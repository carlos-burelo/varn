# Sistema de módulos de Varn — diseño y rendimiento medido

Fecha: 2026-10-09. Binario `quick`. Todo número de aquí sale de ejecutar
el compilador, no de otros `.md`. Detalle de decisiones en
`docs/AUDIT_MODULES.md`.

## 0. Cifras

| Medida | Valor | Cómo |
|---|---|---|
| Módulos únicos en `tests/main.vn` | 280 (153 locales + std/core/runtime) | `vn debug -p graph` |
| `run` frío (caché limpia) | 5.25 s | `Measure-Command`, `VARN_CACHE_DIR` limpio |
| `run` caliente | 2.58 s | segunda corrida (×2.0 por cachés) |
| Ficheros de caché tras 1 corrida | 152 (147 interfaces + vncache + helpers) | conteo dir |
| Determinismo bytecode | idéntico bit a bit entre cachés limpias | sha256 `main.*.vncache` ×2 |
| Determinismo nombres de caché | idénticos (`fnv1a64`, no `DefaultHasher`) | diff lista `.vnm` ×2 |
| Suite | 2287/0 ×4 cuadrantes (+ release `-Fast` verde) | `verify.ps1 -Fast` |
| Divergencias de layout foráneo en suite+stdlib | 0 (2 cazadas en desarrollo, fijadas) | assert `verify_foreign_layouts` |
| Acceso campo/método local vs foráneo | 50ms vs 49ms / 5M iters (ruido) | bench `/tmp/bench_cross.vn` |
| Generadores (B0) | pingpong 100k=31ms, mapfilter 100k=44ms; checksums idénticos JIT/NO_JIT | `bench_generator.vn` |

## 1. Capas y especificadores

Capas (`varn-modules/src/layer.rs`): `Core` (`core:` + builtin, siempre en
ámbito), `Runtime` (`runtime:`, host, solo importable desde `Std`),
`Std` (`std:`), `User` (resto). `check_import` se aplica en binder
(imports y reexports) y la regla de seguridad `Runtime→solo-Std` además
en runtime (`varn-vm/src/exec/modules.rs`).

Especificadores (`ImportSpecifier::parse`): `Relative` (`./`, `/`,
absoluto), `Package` (`.vn/packages`, manifiesto + constraints),
`Stdlib`/`Core`/`Runtime` (provider embebido o árbol fuente).

Contención: los relativos no pueden escapar el project root
(`varn-modules/src/contain.rs`, raíz cacheada por proceso, solo si hay
manifiesto) — binder (diagnóstico `VN2010`), linkado y runtime (error).

## 2. Compile-time

```
texto → tokens → AST → Binder → Checker → emit(TIR) → link → from_tir → protos
```

- **Resolución.** Una sola implementación de join+exists+canonicalize:
  `resolve_specifier_path` (`varn-modules/src/package.rs:382`); el
  `DiskResolver` delega en ella y `module_precompile` también (antes
  duplicada). `ModuleResolver::resolve` (puro, sin IO) solo la usa el
  runtime.
- **Binder** (`varn-binder`): `bind_import` adopta la tabla foránea,
  clona símbolos con `origin_module` + `with_origin` + canonización a
  `Builtin`/`Primitive` (`varn-sem/src/rehome.rs`); reexports registran
  dep y pasan `check_import` + contención.
- **Exports** (`varn-resolver/src/exports.rs`): colector único por
  ramas (`Decl` con N declarators, `Named` ± source, `All` ± alias,
  `Default`); slots deterministas (`keys.sort`); `export *` copia
  ordenado, colisión = registro + first-wins; `export * as ns` crea
  SOLO el objeto (sin aplanar `ns.sub`).
- **Tipos como interfaz portable.** Los ids son contenido (`xxh3`),
  no índices: clonar entre módulos es seguro tras `absorb`. Función
  canónica `rehome_*` (`varn-sem/src/rehome.rs`); `classify_decl`
  con prioridad `SumType > Enum > Class` decide a qué colector va cada
  tipo (`Result`/`Option` → enum, nunca clase).
- **Clases/enums foráneos.** Solo clases raíz no-builtin no-lang
  (`ForeignClass`); la subclase local enlaza al padre foráneo
  (`Ancestry::Local`, sin duplicar); `instanceof` foráneo va por vía
  dinámica (patrón del codebase); `try ?` exige `Enum` (los sums van
  a enum, las clases nativas quedan opacas).
- **Caché de interfaces** (`.vnm`, sobrees `VARN`+`BUILD_FINGERPRINT`+
  productor+checksum): clave `fnv(id)+fnv(fuente)` determinista;
  envolvente con **hashes de dependencias** — si una dep cambia, miss
  (invalidez transitiva; antes servía binds rancios); `fsync` de
  fichero + directorio.
- **Chequeos de linkado** (fallan fuerte, nunca silencioso):
  `verify_synthetic_imports` (toda arista TIR no declarada en AST debe
  ser prelude), `report_export_collisions` (`E_EXPORT_COLLISION`),
  `verify_foreign_layouts` (`E_CLASS_LAYOUT_DIVERGENCE`: slots exactos
  + tipos por representación).
- **Imports por slot (F5a).** `resolve_import_slots` (entry, deps,
  stdlib, bench) rellena `TirImportSpec.slot` desde `ExportMap`;
  `build_imports` emite `LoadModule` + `ModuleSlot` + guarda
  (`GetProperty` + `Eq` + `Throw E_SLOT_MISMATCH`, solo insts
  existentes); sin slot → `GetProperty` (packages). TOCTOU: de
  corrupción silenciosa a error fuerte.
- **Grafo y precompilado.** Orden topo determinista (colas ordenadas);
  `graph_hash` con claves ordenadas; cada módulo se chequea+emite y su
  proto queda en el mapa precompilado. Caché `.vncache` valida
  `source_hashes` de TODOS los módulos + fingerprint de build.

## 3. Runtime

- **Carga** (`varn-vm/src/exec/ctx_modules.rs`): caché linker →
  módulos (thaw si congelado) → nativos no-puros → circular = TDZ →
  precompilado → `loader.load` (fallback JIT) → `eval_module_proto`
  (contrato `export_names` ordenados → `E_SLOT_MISMATCH` si no).
- **Puerta de capabilities:** evaluar una dependencia `Local`/`Package`
  exige `fs.read` (`E_PERMISSION_DENIED`); la entrada está exenta
  (el usuario la eligió). Núcleo/std/runtime no son ficheros: sin gate.
- **Aislamientos** heredan capabilities por valor (no `Rc` cruzando
  `spawn`); cada hilo tiene su heap y su registro de clases (sin
  memoria compartida: `SendValue`).
- **Identidad de clases:** objetos `ClassObj` con id global monótono +
  registro por hilo; `instanceof` camina `superclass` comparando ids.
  Creación única por VM (linker); congelados preservan el `Rc`.
  `MakeClass` siempre fresco (la frescura por evaluación es load-bearing
  para REPL: redefinir una clase debe crear objeto nuevo).
- **Backstop nativo:** `check_native_op_capability` en `CallNativeOp`
  (intérprete) y `jit_call_native_window` para single-cap ciertos
  (`ffi`→`sys.ffi`, `sys:{env,setEnv}`→`sys.env`,
  `process:{exec,spawnProcess}`→`sys.exec`); fs/net se gobiernan por
  checks con contexto de argumento en cada fn (una tabla estática no
  puede expresar modos read/write).

## 4. Cachés y determinismo (Ley 4)

- Ningún `FxHashMap` decide orden de binding/export/slots/topo:
  sorts explícitos o `BTreeMap` en cada punto.
- `source_fingerprint`/`module_key` con `fnv1a64` (nunca `DefaultHasher`,
  cuyas claves aleatorias rompían la caché entre procesos).
- `TySlice`/`slice` ya usaba `BTreeMap` (sin cambio).
- Resultado: mismo input ⇒ mismo bytecode bit a bit (§0).

## 5. Límites conocidos (medidos, no pendientes de romper)

- Sin ids globales de clase en linkado ni `LoadModule` tipado: medido
  sin gap (JIT converge local/foráneo), y TOCTOU cubierto por guardas.
  Reabrir solo con bench que muestre pérdida (Ley 10).
- `vn run` por defecto permisivo: sandbox total es incompatible con
  multi-fichero (el propio gate lo denegaría); requeriría semántica
  allow-entry-tree (decisión de producto).
- `varn_rt_str_concat` filtra en AOT (sin dueño; liberar sería UAF).
- `resolve()` conserva su `panic` como fail-loud; `display` y el resto
  de fronteras ya resuelven dual (interner → tabla).
- Contadores `PASSED/FAILED` globales a propósito (agregación de
  `vn test`); `print_silent` sí es por-VM. `precompiled` no se hereda
  a isolates (`Rc:!Send`; fallback por loader).
