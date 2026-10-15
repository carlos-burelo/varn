# Maduración M1–M6 — cuello de botella para UI (2026-10-13)

Método: medir antes de romper (Ley 12/14). Binario `quick`, esta máquina. Re-medir antes de citar.

## Baselines

| caso | JIT | intérprete |
|---|---|---|
| loop 200k `float[i]` sum | 1 ms | 23 ms |
| template 50k `${t}ab${i}` | 10184 ms → **5 ms** (fix) | 2438 ms |
| `+` concat 50k | 5 ms | — |
| frames 500×20k floats (10M stores) | total 71 ms, peor frame 1 ms | — |
| closure por llamada 200k | 1 ms | 23 ms |
| field read 100k instancias | 0 ms | 9 ms |
| suite | PASSED 3226, FAILED 0 | — |

## Segunda oleada (M7–M10)

| caso | JIT | intérprete |
|---|---|---|
| map 50k `set` (con key alloc) | 11 ms | — |
| map 50k `get` + `?? 0` | 6 ms | — |
| closure con captura alocada por iter 100k | 14 ms | 31 ms |
| call-site 2 shapes + `new` por iter 100k | 12 ms | — |

- **M7 maps: sano** (~0.2µs/op). `Map.get` devuelve nullable: aritmética exige `??` — el checker lo impone, bien.
- **M8 closure-alloc: vigilar, no actuar.** 140ns/iter, escape analysis no cubre closures (solo `AllocInstance` mismo bloque). Cientos de handlers/frame ≈ µs. Puerta futura: si un frame UI supera budget por esto, escape de closures es el fix (declarar ganancia entonces).
- **M9 polimorfismo: sano.** 2 shapes + alloc = 120ns/iter. IC aguanta.
- **M10 deuda de diseño (evidencia código, sin bench):**
  - triple mecanismo excepciones (`try_handlers` + `exception_table` + `pure_try`) — `exec/exceptions.rs:137-178`. Unificar a una tabla (Ley 8). Solo higiene: M11 mide `try` sin throw a 5ns/iter, no es cuello.
  - CORREGIDO: `fn_name`, `PAIR_MIGRATION_*`, `stack_roots::{register,unregister}`, `stats::record` **no** están muertos — viven tras `#[cfg(feature="clif")]` (`varn-jit/src/lib.rs:157-161`, usados en `lower.rs:117-131,243` y `lib.rs:210,233`). El warning sale solo sin la feature. No borrar.
  - warnings `collapsible_if` (~30, reales, estilo): bajan el suelo de ruido; fix mecánico en lote aparte.
  - `now()` resolución ms entera en `std:time` — obliga a escalar loops para medir. Papercut tooling.

## Tercera oleada (M11–)

- **M11 try sin throw 200k: 1 ms vs 0 ms sin try.** Push/pop de handler ≈5ns/iter. No cuello; unificación pendiente solo por Ley 8.
- **M12 `clippy --fix` mecánico** (`collapsible_if` → let-chains, edición 2024): 9 archivos (`std_manifest`, `artifact`, `contain`, `package`, `std_root`, `lines`, `loop_analysis`, `class` + tocados previos). Solo colapsos, cero semántica. Suite verde tras el pase.
- **M13 corrección de auditoría:** los supuestos muertos (`fn_name`, `PAIR_MIGRATION_*`, `stack_roots::{register,unregister}`, `stats::record`) viven tras `#[cfg(feature="clif")]`. Regla: un warning bajo features parciales no autoriza borrar — verificar con la feature puesta.
- **M14 fusión `varn-contract`+`varn-op-macros`→`varn-macros`.** Dos crates proc-macro sin razón arquitectónica. Un crate con `varn_contract!`+`#[jit_slow]`+`jit_helper_table!`; renombre mecánico en 57 ficheros (`builtins`+`vm`+scaffold `xtask`); `Cargo.toml` swap; dirs viejos borrados. 29→28 members. Suite verde, fmt+clippy limpios.
- **M15 `resolver`/`modules`: NO fusionar.** Capas limpias, una dirección, `modules` sin frontend. Limpieza: 26 constantes `STD_*`/`RUNTIME_*` muertas en `ids.rs` (stale, sin `gpu/text/ui`); resolución por provider dinámico.
- **M16 `varn-rt` ABI mínima + chicos auditados.** Borrados `print_int`/`print_bool` (AOT solo importa `print`+`str_concat`); quedan `main`, `print`, `str_concat`, `panic`. `shadow` todo vivo. `macros` sin duplicación entre orígenes. `jit-abi` contrato puro, sin poda accionable.
- **M17 suelo clippy cero.** `--fix` workspace (~300 archivos) + a mano: `Check::param` muerto, `depth` fantasma en `reshapes`, `unsafe` innecesario, `contains_key`. Restan solo `wildcard_enum_match_arm` en enums gigantes — decisión: el warning ES el mecanismo Ley 7 (variante nueva ⇒ revisión obligada); expandir 100 brazos a mano = churn+riesgo, cero ganancia.

## Reglas para no repetir

1. Medir antes de romper. M1/M3/M4/M5/M7/M9 eran fantasmas; M2 era real. Sin número no hay rewrite.
2. Todo desugar con identidad algebraica (`""+s`, `s+""`, `[].concat`) se audita por copias fantasma con `debug -p bytecode` antes de darlo por bueno.
3. Tip-reuse solo si `is_tip` (buffer len == view len) + crecimiento 2x. Condición documentada en `heap/str.rs:190-195`.
4. Arrays numéricos: no mezclar tipos en caliente o migran a boxed O(n) (`migrate_to_boxed`, cold).
5. Fast paths con umbral llevan test estructural (patrón `engine()` en `214`), no asserts de backend fijo — el test debe pasar con y sin GPU.
6. Perfil `panic=abort` vuelve `catch_unwind` inútil: validación por construcción (flags de uso, tamaños>0), nunca por rescate. Lección `wgpu`.
7. Cero warnings clippy en archivos tocados; deuda ajena se cita, no se arrastra.
8. Wildcards: expandir a exhaustivo, nunca ignorar. Futura variante = error de compilación en cada sitio (mejor que warning). Ver M18.
9. Los errores se adjuntan al plan de corrección, nunca se posponen. Deuda ignorada se vuelve impagable: 85 fixtures stale + harness sin CI se extirparon en el mismo lote que los encontró (M20).

## Quinta oleada (M18): wildcards a cero

- 26 sitios expandidos a brazos explícitos (4 agentes por área, verificación global propia): `binder` 1, `sem` 2, `checker` 7, `compiler` 2, `emit` 3, `jit` 11. Incluye los 4 gigantes `SsaOp` (~85 brazos) y `TirExprKind`/`TypeKind`/`BackendTy`/`SsaBinOp`/`SsaTerm`/`SymbolKind`.
- Comportamiento idéntico, solo listas explícitas. `clippy --workspace`: 0 wildcard. Suite JIT + NOJIT: 3239/0 en ambas.

## Sexta oleada (M19): varianza medida, cero sintaxis

- `varn-sem/src/compat/variance.rs` nuevo: walker de posiciones + `measure_params` (solo-salidas→covariante, solo-entradas→contravariante, ambas→invariante, cero-usos→covariante/phantom).
- `BindResult.type_param_variance` (serializado, viaja entre módulos) calculado al final del bind en orden determinista (`binder/src/variance.rs`); lookup con origen en `generic_arms` (local o bind foráneo).
- Swap P2: muere la allowlist quemada. Contravarianza chequea invertido.
- Adjudicación: `Generator` invariante CORRECTO (`return(value?: T)` + driver que escribe al frame sin check = hueco estilo Dart; test 199 pedía lo insano). `Task` covariante honesto vía `interface Task<T> {}` phantom en `iteration.vn`. Test 199 reescrito + fixture negativo `tests/errors/generator-narrowing-rejected.vn` (`expect: error[VN3001]`, exit 3).
- Deuda encontrada: 85 fixtures `tests/errors` con cabecera stale + CI no corre `error_corpus`. Anotado, fuera de lote.
- Puertas: suite JIT + NOJIT 3239/0, clippy cero (incl. 2 wildcards del propio cambio, expandidos), fmt ok. Docs: `lang/types.md` §8.2.

## Séptima oleada (M20): deuda error_corpus extirpada en el lote

- 85 fixtures `tests/errors` con cabecera stale (`// VNxxxx`, el harness exige `// expect:`): triage mecánico (100% con token `error[VN...]` limpio, 0 discrepancias stale-vs-medido) → 1 línea cada una. Harness `cargo test -p varn-cli --test error_corpus` verde (206 fixtures).
- CI no lo corría: añadido paso `Error Corpus` a `test-matrix` en `ci.yml` (tras bench). La puerta que no corre no existe.

## Octava oleada (M21): raster de glifos (cierra C1)

- `varn-text/src/font.rs`: DB y `sans_bytes()` compartidos (shape y atlas usan la misma carga).
- `varn-text/src/atlas.rs`: cache `(glyph, size)` + `ScaleContext` persistente, `Render(Outline).format(Alpha)` → `{w,h,bx,by,advance,bitmap}`; sin outline (espacio) = bitmap vacío con avance.
- `runtime:text` + `std:text`: `rasterGlyph` (bitmap como `Bytes`), `glyphCacheSize`.
- Puerta: `216` (+7 asserts: `bitmap.length == w*h`, determinismo, cache estable en repetición). Suite 3246/0, fmt+clippy limpios.

## Novena oleada (M22): ventana real F2

- `varn-ui` nuevo (hoja `winit`+`wgpu`+`pollster`): `EventLoop` no es `Send` → loop por pump en `thread_local`, cero hilos (portable incl. macOS-main-thread a futuro); afinidad de hilo con error explícito; superficie efímera por `present` (cero lifetimes compartidos; persistente en F2b); `AutoVsync` = pacing natural.
- Decisiones contra fácil: teclas por `to_text` (sin matchear `NamedKey` ajeno de 150 variantes), `WindowEvent` exhaustivo a mano (~28 brazos), `any_thread` solo Windows + `catch_unwind` en todo (real que falla = headless, nunca crash; `panic=abort` obliga).
- Backend real tras `VARN_UI_REAL=1` (default headless: CI determinista); `real()` lo reporta. `poll` drena OS primero; `size` refresca; `present` dibuja clear + cuenta log.
- Puerta: ventana real abre/presenta/cierra (`real()=true`, `present()=2`), `215` verde en ambos modos (paridad API), suite 3246/0, clippy cero.

## Décima oleada (M23): compositing real F2b

- `DrawCmd` estructurado (`draw.rs`: `Clear/Rect/Text`, `format`, `assemble`); el log pasa a ser vista, no fuente. `text` gana `color` (breaking registrado + `215` actualizado).
- `comp.rs`: 2 pipelines WGSL (rect instanciado + texto con atlas/UV), quads por `vertex_index` sin vertex buffers, blend alpha, `AutoVsync`; `present_draw` = superficie + una submit. Atlas R8 1024 shelf por present (persistente en F2c).
- Layout texto en `assemble`: baseline `y+ascent` (`font_ascent`), draw en `pen+ox+bx/baseline-by` (semántica `BottomLeft` verificada en `zeno`), avance por shape; vacíos avanzan sin quad.
- Cazados: buffer uniforme necesita uso `UNIFORM` (no `VERTEX`), `RenderPipeline`/`BindGroupLayout` no son `Clone` (Arc), `ImageCopy*` no existe en wgpu22 (`ImageCopyTexture`/`ImageDataLayout`).
- Puerta: rect+texto pintados sin abort (`present()=3`), `215` ambos modos, suite 3247/0, clippy cero salvo `unused_crate_dependencies` preexistente en builtins. Siguiente F2c: pools persistentes + wrap.

## Veredictos

- **M1 arrays: no rediseñar.** Backing unboxed `I64/F64` (`vm_array.rs:24-33`) + fast paths JIT con view cacheada (`varn-jit/.../arrays.rs:128-215`). Limpieza: borrado `VmArray::borrow()` muerto + panic stale "Task A.4" (cero llamadores). Riesgo anotado: `push` heterogéneo migra a boxed O(n).
- **M2 strings: FIX APLICADO.** Templates desugarn a 4 `StrConcat` con `""` de bordes; `""+t` copiaba el acumulador (O(n²)) y `BuildStr` siempre alocaba fresco. Unificado en `str_concat_parts` (`varn-vm/src/exec/strings.rs`): SSO-pack → tip-reuse → inline → fresh (`Inline ≤37`/`Ext 2x`), usado por `StrConcat`+`BuildStr` en intérprete y JIT. Identidad `""+s`/`s+""`. Resultado: 10184 → 5 ms, checksum idéntico.
- **M3 alloc/GC: no tocar.** 10M stores sin picos (peor frame 1 ms). Umbral young 786k + sweep O(born) barato. Major GC bajo árbol retenido grande queda como puerta futura cuando exista UI retenida.
- **M4 closures en loop: sano** (1 vs 23 ms, inline cubre).
- **M5 props: sano** (0 vs 9 ms, shapes+IC cubren).
- **M6 canales: decisión de diseño, no fix.** Bomba de eventos se medirá con UI real; batch por frame ya previsto en `std:ui`.

## Puerta de cada cambio

Suite `tests/main.vn` verde + `cargo fmt` + cero warnings clippy nuevos en tocados. Resto de warnings preexistentes, no tocar en este lote.
