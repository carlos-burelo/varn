# Plan de corrección: auditoría de leyes y rendimiento (2026-11-11)

Complementa a `2026-10-16-plan-unificado.md`; no lo sustituye. Donde un item
de aquí toca algo que aquel plan da por cerrado o reservado, se indica y
**requiere autorización del dueño** antes de ejecutarse. Si este documento
contradice al código, manda el código (Ley 12): toda cifra se vuelve a medir
antes de actuar.

- Commit auditado: `51b368fa` (último que toca `crates/`: `54cb85fb`).
- Binario: `target\release\vn.exe` recompilado sobre ese commit.
- Host: Intel Core Ultra 7 255H, Windows 11, Bun y Node 26.13.0.

## 0. Reglas de ejecución

- Cada item declara (a) ganancia esperada, (b) cómo se mide, (c) qué se borra (Ley 10).
- Un commit atómico por item (Ley 9). Si el mensaje necesita "y", son dos items.
- Ninguna ganancia se da por buena sin `vn bench X --baseline base.json` con
  `delta%` mayor que `ci95%`. `same` significa que no se probó nada y el item se aparca.
- Un mejor conteo de opcodes no es una ganancia de tiempo.
- Orden obligatorio: Fase M (medición) antes que cualquier item de Fase R.
- Puerta por item: suite en JIT y `VN_NO_JIT=1`, `scripts\verify.ps1 -Fast`
  antes de push (Ley 19); matriz completa al tocar JIT, caché JIT o cargador de std.
- Las causas marcadas **hipótesis** no están probadas: el primer paso del item es probarlas.

## 1. Línea base medida

### 1.1 Corrección

| Comprobación | Resultado |
|---|---|
| `vn run tests\main.vn` (JIT) | `PASSED: 5542`, `FAILED: 0` |
| `VN_NO_JIT=1` | `PASSED: 5542`, `FAILED: 0`, salida idéntica a la de JIT |
| Tres volcados `debug -p bytecode tests\main.vn` | mismo SHA-256 (11 865 811 caracteres) |

### 1.2 Contra rivales (`xtask compare`, 7 corridas, 2 de calentamiento)

| Carga | Varn | Bun | Node | Contra Bun |
|---|---|---|---|---|
| http_routing | 290.2 ms | 80.5 | 94.9 | 3.57x más lento |
| json_pure | 396.3 ms | 232.8 | 280.0 | 1.69x más lento |
| json_api_payloads | 21.8 ms | 13.5 | 15.3 | 1.61x más lento |
| fib | 56.8 ms | 40.1 | 61.0 | 1.41x más lento |
| json_native | 26.9 ms | 25.0 | 22.6 | 1.19x más lento |
| str_ops | 64.2 ms | 80.4 | 54.5 | gana a Bun, 1.18x más lento que Node |
| csv_etl | 17.5 ms | 17.2 | 16.8 | empate |
| collection_pipeline | 10.5 ms | 24.9 | 25.6 | 2.37x más rápido |
| new_retained | 7.5 ms | 11.7 | 17.4 | 1.56x más rápido |
| gc_alloc | 25.8 ms | 45.9 | 36.8 | 1.43x más rápido |
| matrix | 5.8 ms | 10.6 | 8.1 | 1.40x más rápido |
| class_fields | 1.6 ms | 2.1 | 6.4 | 1.31x más rápido |
| dto, csv_pipeline | — | — | — | `OUTPUT MISMATCH`, sin valor |

Arranque con programa vacío: Varn 22.8 ms, Bun 19.6 ms, Node 74.6 ms.

Estos números contradicen la línea "No tocar: fib/dto/json_pure/gc_alloc
(ganan o empatan)" de §2.4 del plan unificado: hoy fib y json_pure pierden.

### 1.3 Perfiles de CPU (build `profiling`, 1000 Hz)

| Carga | En código compilado | En GC | Mayores `self` |
|---|---|---|---|
| http_routing | 15.6 % | 21.9 % | `mi_malloc_aligned` 6.0, `Route.match` 5.6, `Router.dispatch` 5.0, memcpy 4.6, `take_cell` 4.0, `alloc_map_vm` 3.2, `calls_static::settle` 2.9, `alloc_array_repr` 2.5, `prepare_call` 2.1, `invoke_pushing` 2.1 |
| json_pure | 0.4 % | 11.3 % | `JsonParser::parse_value` 17.9, memcpy 14.4, `ryu::format64` 11.7, `decode::walk` 5.2, `read_string_text` 4.6, `take_cell` 4.5, `conform::fit_in` 4.1 (6.9 total) |
| json_api_payloads | 7.4 % | 0.0 % | memcpy 16.3, `parse_value` 9.8, `mi_malloc_aligned` 6.5, `alloc_object_cell` 4.7, `take_cell` 4.5, `fit_in` 3.8, `alloc_array_repr` 3.7, `decode::walk` 3.4 |
| fib | 81.9 % | 0.0 % | `fib` 99.8 |
| str_ops | 8.8 % | 9.0 % | `split_ranges` 9.6, memcpy 8.3, `HeapInner::alloc` 8.3, `alloc_str_view_of` 7.3, `jit_str_char_code_at` 5.5, `jit_str_split` 5.5, `str_concat_unchecked` 5.0, `strbuf::itoa` 4.3, `HeapStr::as_str` 3.1 |

### 1.4 Otras mediciones

| Medición | Valor |
|---|---|
| `http_routing` con `VN_GC_TRACE=1` | 4 menores y 1 mayor; cada menor entra con 530 000–670 000 celdas y 42–55 MB, tarda 9.4–15.4 ms; `external` ≈ 16.8 MB en cada una |
| fib: JIT / `VN_NO_JIT=1` (proceso completo) | ≈ 100 ms / 3177 ms |
| matrix: JIT / sin JIT | 32 ms / 229 ms |
| json_pure: caché on / caché off / sin JIT | 415 / 385 / 402 ms (el JIT no influye) |
| Suite: primera corrida tras recompilar / siguientes | 112 s / 40 s |
| Opcodes del corpus (179 810) | `Move` 42 149 (23 %), `LoadNull` 18 212 (10 %) |
| Ruido de `vn bench` | fib cv 51.9 %, str_ops cv 30.0 % |

### 1.5 Cobertura estática

| Par tipado / genérico | Corpus | Benchmarks |
|---|---|---|
| GetFixedField / GetProperty | 1856 / 1471 (56 %) | 92 / 22 (81 %) |
| EqInt / Eq | 512 / 216 (70 %) | 11 / 11 (50 %) |
| InvokeVirtual / CallMethod | 7369 / 202 (97 %) | 40 / 0 |
| AddInt, SubInt, MulInt, LtInt, ArrayGetIndex, ArrayLength | 95–99 % | 100 % |

`debug -p typeloss` sobre `bench_generator.vn`: 11 `Eq` genéricos repartidos en
`nested_outer`, `nested_inner`, `guarded`, `pingpong`, `range_gen`, `fib_gen` y un closure.

---

## 2. Fase M — que la medición diga la verdad

Nada de la Fase R empieza hasta cerrar M1–M5.

### M1. `xtask compare` encuentra el `bun.exe` real

- **Defecto:** `find_binary` (`crates/xtask/src/run.rs:63`) devuelve el primer
  `dir.join(cmd)` que sea archivo. Con fnm ese archivo es un shim sin extensión,
  y `Command::new` falla con `Os { code: 193 }`. La comparación solo corrió
  anteponiendo `...\node_modules\bun\bin` al `PATH`.
- **Cambio:** en Windows probar `cmd.exe` primero; si solo hay shim (`.cmd`,
  `.ps1` o sin extensión), resolver `node_modules\<cmd>\bin\<cmd>.exe` junto al
  shim, igual que hace `Resolve-Executable` en `tests/benchmarks/compare.ps1`.
  Validar el candidato ejecutando `--version`; si falla, omitir el runtime con aviso.
- (a) La comparación verificada corre sin preparar el entorno. (b) `cargo run -q
  --profile quick -p xtask -- compare --only fib` termina con código 0 en una
  consola limpia. (c) Se borra la rama que acepta el nombre sin extensión en Windows.

### M2. `dto` deja de dar `OUTPUT MISMATCH`

- **Defecto:** `bench_dto.vn` imprime dos líneas (`43332` y `3245034.0`);
  `js/dto.js` imprime una suma (`3288366`).
- **Cambio:** que las tres variantes (`vn`, `js`, `py`) impriman el mismo
  checksum con el mismo formato. Decidir una sola convención de checksum entero
  para todas las cargas y aplicarla.
- (a) Una carga más con veredicto. (b) `compare --only dto` sin mismatch.
  (c) La salida divergente.

### M3. `csv_pipeline` deja de dar `OUTPUT MISMATCH`

- **Defecto:** no existe `tests/benchmarks/js/csv_pipeline.js`; `xtask` toma
  el equivalente de otro lugar y la salida no coincide con
  `CSV Pipeline: Completed Sales = 30000, Total Revenue = $27930000.0, Export bytes = 2328999`.
- **Primer paso:** localizar en `crates/xtask/src/benches.rs` qué archivo corre
  para Bun/Node y comparar su salida línea a línea con la de Varn.
- (a), (b), (c) como M2.

### M4. Un solo arnés de comparación

- **Defecto:** `tests/benchmarks/compare.ps1` mide tiempo de proceso, pero los
  `js/*.js` repiten la carga 13 veces dentro del proceso y los `vn/*.vn` una.
  Reporta fib 82.1 ms contra Node 723.4 ms; lo verificado es 56.8 contra 61.0.
  Tampoco comprueba checksums. Dos arneses con respuestas distintas chocan con
  la Ley 6 y la Ley 8.
- **Cambio:** tras M1, borrar `compare.ps1`. Lo que aportaba (resolver el
  ejecutable real, tiempo menos arranque) pasa a `xtask compare`.
- (a) Una sola fuente de verdad para números contra rivales. (b) `xtask compare`
  muestra la columna de estado estacionario. (c) `compare.ps1` y su mención en
  `tests/benchmarks/README.md`.
- **Requiere decisión del dueño:** el script es de ayer; si se prefiere
  conservarlo, entonces debe igualar el trabajo por proceso y verificar checksums.

### M5. Ruido de medición acotado

- **Defecto:** cv de 30–52 % en fib y str_ops. **Hipótesis:** el planificador
  alterna núcleos P y E del 255H.
- **Primer paso:** medir fib 20 corridas con y sin afinidad a núcleos P
  (`Start-Process` + `ProcessorAffinity`). Si la afinidad baja el cv por debajo
  de 10 %, `vn bench` y `xtask compare` la fijan por defecto.
- (a) Deltas de 5 % pasan a ser detectables. (b) cv < 10 % en fib, str_ops y
  json_pure. (c) Nada.

### M6. Líneas base versionadas

- Registrar `vn bench X --runs 15 --warmup 3 --format json` de las 25 cargas en
  `target\tmp\baseline\bench\` con el binario de `51b368fa`, después de M5.
- Cada item de Fase R compara contra esa base y contra el resto de cargas.

### M7. Mapa del repo al día

- `.claude/skills/varn-repo-map/SKILL.md` cita `crates/varn-regalloc` y
  `crates/varn-runtime`, que no existen, y omite `vn-emit`, `vn-sem`,
  `vn-binder`, `vn-tir`, `vn-resolver`.
- Corregir la lista contra `ls crates/`. Actualizar también la línea "No tocar"
  de §2.4 del plan unificado con la tabla de §1.2.

---

## 3. Fase R — rendimiento del runtime

Orden por ganancia medida disponible. R1–R4 atacan `http_routing`; R5–R6 JSON;
R7–R9 llamadas; R10–R11 strings; R12–R14 intérprete y arranque.

### R1. El presupuesto del vivero se respeta

- **Evidencia:** `NURSERY_BYTES = 16 << 20` (`crates/vn-vm/src/settings.rs:22`),
  pero las colecciones menores entran con 42–55 MB de celdas. El límite se
  guarda en número de celdas: `recompute_young_limit`
  (`crates/vn-vm/src/heap/gc.rs:79`) divide `nursery_bytes` entre el tamaño
  medio de la generación anterior, o entre `UNKNOWN_CELL_BYTES = 24` si está
  vacía. En `http_routing` la celda real pesa unos 80 bytes, así que el vivero
  crece 3.3 veces sobre su presupuesto.
- **Hipótesis a probar primero:** el disparo real es el contador externo
  (`external` ≈ 16.8 MB en cada línea de la traza), no el de celdas.
- **Cambio:** contabilizar el vivero en bytes reales (celdas más externo)
  contra un único presupuesto, y disparar la menor al cruzarlo. Barrer el
  presupuesto entre 1, 2, 4, 8 y 16 MiB con `VN_GC_TRACE=1` y elegir por
  tiempo total de `http_routing`, `gc_alloc`, `new_retained`, `json_pure`.
- (a) Menos fallos de caché por asignación y pausas menores más cortas;
  objetivo: `in gc` de `http_routing` de 21.9 % a menos de 12 %. (b) `vn bench
  bench_http_routing.vn --baseline`, sin regresión en `gc_alloc` ni
  `new_retained`. (c) `UNKNOWN_CELL_BYTES`, `limit_cells` y el cálculo por promedio.
- **Coordinación:** el heap viejo acaba de cambiar (`54cb85fb`) y ADR-0027
  gobierna el GC; revisar que el cambio no contradiga ese diseño.

### R2. Asignación joven por puntero

- **Evidencia:** `CellSpace::take_cell` 4.0–4.5 %, `HeapInner::alloc` 1.2–8.3 %,
  `note_new_cell` 0.9–1.3 % en las cuatro cargas con asignación.
- **Estado:** es el item O1 ("bump-alloc nursery") de §2.4 del plan unificado,
  reservado a otra sesión. **No ejecutar sin coordinar.** Este plan solo aporta
  la medición actual y el criterio de éxito.
- (a) `take_cell` + `alloc` + `note_new_cell` por debajo de 3 % combinado en
  `http_routing`. (b) Perfil antes y después. (c) El camino de lista libre para
  celdas jóvenes.

### R3. Los objetos efímeros no llegan al heap

- **Evidencia:** `ReqContext.constructor` 9.2 % total, `jsonRes` 16.1 % total y
  el closure de ruta 19.5 % total en `http_routing`; `alloc_map_vm` 3.2 % más
  `host::alloc_map` 1.6 % más `Map::constructor` 0.8 %.
- **Primer paso:** `vn debug` sobre `bench_http_routing.vn` para ver qué
  asignaciones marca hoy `crates/vn-compiler/src/passes/escape.rs` como no
  escapantes y cuáles no, y por qué (paso a un closure, a un `Map`, retorno).
- **Cambio:** extender el análisis de escape a los casos que salgan de ese
  inventario, empezando por el objeto construido y consumido dentro del mismo
  `dispatch`. Reemplazo escalar, sin celda.
- (a) Menos celdas por petición; se mide en `in=` de la traza de GC. (b) Traza
  y `--baseline`. (c) Nada salvo que un caso sustituya una ruta previa.
- **Límite:** no introducir un segundo camino de construcción (Ley 8).

### R4. Los búferes de arreglos y mapas no pasan por `malloc`

- **Evidencia:** `mi_malloc_aligned` 6.0 % en `http_routing` y 6.5 % en
  `json_api_payloads`; `alloc_array_repr` 2.5–3.7 %; `external` ≈ 16.8 MB por ciclo menor.
- **Hipótesis:** cada arreglo y mapa joven reserva su respaldo fuera del heap.
- **Primer paso:** `VN_ALLOC_PROFILE` sobre `http_routing` para contar
  reservas externas por sitio y tamaño.
- **Cambio (si se confirma):** respaldo en línea para colecciones pequeñas,
  dentro de la celda o del vivero; `malloc` solo sobre un umbral.
- (a) `mi_malloc_aligned` por debajo de 2 %. (b) Perfil y `--baseline` en
  `http_routing`, `json_api_payloads`, `collection_pipeline`. (c) La reserva
  externa incondicional.
- **Decisión cerrada que no se toca:** "Map SmallVec" (§4 del plan unificado).

### R5. JSON tipado en una sola pasada

- **Evidencia:** el camino tipado hace tres recorridos:
  1. `FastJsonParser::parse_value` (`crates/vn-vm/src/exec/ctx_json/fast.rs:47`)
     construye el valor genérico.
  2. `decode::project` → `walk` (`crates/vn-vm/src/exec/decode.rs:55`) lo
     reconstruye según el tipo; `rebuild` vuelve a asignar los objetos.
  3. `conform_to` → `fit_in` (`crates/vn-vm/src/exec/conform.rs:143`) lo valida.

  El tipo viaja como texto: `parse_descriptor` busca el `TypeDesc` en un
  `thread_local` `FxHashMap<String, Rc<TypeDesc>>` en cada llamada. En
  `json_pure` los pasos 2 y 3 suman 12.1 % y arrastran parte del 11.3 % de GC y
  del 14.4 % de memcpy.
- **Ley:** Ley 6 (el hecho que el checker ya calculó se re-deriva en runtime) y
  Ley 16 (descriptor identificado por cadena).
- **Cambio:**
  1. El compilador emite el `TypeDesc` como constante del chunk con índice, no
     como texto; el nativo recibe el índice.
  2. Un decodificador dirigido por `TypeDesc` que lee el texto JSON y escribe
     directamente la forma final (`Shape` de la clase o del objeto), validando
     mientras lee. Error de tipo con la misma posición y mensaje que hoy.
  3. `parse_value` genérico queda solo para destino `TypeDesc::Any`.
- (a) Objetivo: `json_pure` de 396 ms a menos de 300 ms; `json_api_payloads`
  de 21.8 a menos de 16 ms. (b) `--baseline` en `json`, `json_pure`,
  `json_parts`, `json_api_payloads`, `dto`. (c) `decode::project`, `walk`,
  `rebuild`, `LAST_ALIGNMENT`, el mapa `DESCRIPTORS` y la variante `decode_native`
  por texto. `conform` se conserva solo si queda otro consumidor; comprobarlo
  con `tgrep`.
- **Riesgo:** el plan unificado registra que "decode conserva miembros
  heredados y recursivos" y que "decode se difiere solo desde retornos
  síncronos directos". El decodificador nuevo debe pasar esos mismos casos del
  corpus antes de borrar el viejo.

### R6. `ryu` y memcpy en `stringify`

- **Evidencia:** `ryu::pretty::format64` 11.7 % y memcpy 14.4 % en `json_pure`;
  `write_json_vm` 3.0 % propio.
- **Primer paso:** ver en `bench_json_pure.vn` qué campos son `float` y si
  algún `int` se está escribiendo por el camino de flotantes. Localizar con
  `--profile-out` qué llamador domina el memcpy.
- **Cambio:** según lo que salga. Si hay enteros formateados como flotantes es
  un defecto de tipo (Ley 6) y se corrige en el emisor; si el memcpy es
  crecimiento del búfer de salida, reservar con el tamaño de la corrida anterior.
- (a), (b) sobre `json_pure`. (c) Depende del hallazgo.

### R7. Una sola guarda por llamada sin frame

- **Evidencia:** el IR de `fib` (`VN_CLIF_PRINT_IR=1`) emite por cada llamada:
  `guard_headroom` (puntero de pila contra `jit_stack_floor`), `enter_frameless`
  (dos cargas, suma, comparación contra 10000, escritura) y `leave_frameless`
  (carga, resta, escritura). Código en
  `crates/vn-jit/src/clif/from_ssa/stack_exit.rs:8-48`. fib pierde 1.41x contra Bun.
- **Estado: decisión cerrada.** El plan unificado (§1, "Recursión 2026-10-26")
  fijó que VM y JIT cuentan contra el mismo límite lógico de 10000. **Reabrir
  exige autorización del dueño.**
- **Propuesta que conserva el límite lógico:**
  1. Mantener el contador en un registro durante la función y escribirlo a
     `frameless_depth_off` solo antes de llamadas que salen del código compilado.
  2. En autorrecursión directa, pasar la profundidad como valor SSA y comparar
     sin tocar memoria.
  3. Omitir `guard_headroom` cuando `enter_frameless` ya acota la profundidad y
     el tamaño de frame por el límite cabe en la reserva de pila; conservarla
     en el resto.
- (a) Objetivo: fib de 56.8 ms a 45 ms o menos. (b) `--baseline` en fib y
  `multiple`; prueba de recursión infinita del corpus con el mismo error y la
  misma profundidad en los cuatro cuadrantes. (c) Las cargas y escrituras de
  `frameless_depth_off` en el camino caliente.
- **Riesgo:** cambia el ABI del JIT; sube la versión de ABI e invalida la caché.

### R8. Argumentos muertos en la convención de llamada

- **Evidencia:** la firma compilada de `fib` es `(i64, i64, i64, i64, i64)` y
  la autollamada pasa tres ceros (`v25`, `v26`, `v27`).
- **Primer paso:** identificar en `crates/vn-jit/src/clif/from_ssa/call/` qué
  representan esos tres parámetros y qué funciones los leen.
- **Cambio:** firma especializada por función cuando no los usa.
- (a) Pequeña; se ejecuta junto a R7 solo si su delta propio supera el ruido.
  (b) `--baseline` en fib. (c) Los parámetros fijos.

### R9. Closures y métodos sin pasar por el runtime

- **Evidencia en `http_routing`:** `calls_static::settle` 2.9 %, `prepare_call`
  2.1 %, `invoke_pushing` 2.1 % (3.6 total), `call_method` 1.5 %; solo 15.6 %
  de las muestras cae en código compilado.
- **Primer paso:** `vn bench bench_http_routing.vn -v` y leer la tabla de
  frames JIT y de inline caches: qué llamadas son `CallMethod` o llamada de
  closure y por qué no se resolvieron a llamada directa.
- **Cambio:** llamada directa a closure con destino conocido; ver también el
  item "tax llamada alloc-free" de §2.4, reservado: **coordinar**.
- (a) `in compiled code` por encima de 40 % en `http_routing`. (b) Perfil y
  `--baseline`. (c) Los helpers que queden sin llamadores.

### R10. `charCodeAt` en línea

- **Evidencia:** `jit_str_char_code_at`
  (`crates/vn-vm/src/exec/jit_helpers/intrinsics.rs:53`) es una llamada a
  helper: 5.5 % en `str_ops`, más `HeapStr::as_str` 3.1 %.
- **Cambio:** en el JIT, camino rápido en línea para SSO y para `HeapStr`
  marcado ASCII: comprobar bandera, cargar byte, comparar índice. El helper
  queda solo como camino lento.
- (a) `benchCharCode` deja de dominar (hoy 18.4 % total). (b) `--baseline` en
  `str_ops`. (c) Nada; el helper sigue siendo el camino lento.
- **Coordinación:** zona de strings, solapa con "str inline" y SSO-8 de §2.4.
  La decisión cerrada "startsWith-SSO-inline" no se reabre.

### R11. `split` sin vistas intermedias

- **Evidencia:** `split_ranges` (`crates/vn-types/src/str_util.rs:76`) 9.6 %,
  `jit_str_split` 5.5 %, `alloc_str_view_of` 7.3 %, `core::str::from_utf8` 1.6 %
  en `str_ops`; `split_ranges` 1.3 % y `jit_str_split` 1.2 % en `http_routing`.
- **Primer paso:** ubicar el llamador de `from_utf8` con `--profile-out`. Si
  revalida texto que ya es UTF-8 válido, eliminar la validación.
- **Cambio:** separador de un byte con `memchr`; partes de 8 bytes o menos
  como SSO sin celda.
- (a), (b) sobre `str_ops` y `http_routing`. (c) La validación redundante.

### R12. Costo fijo por opcode del intérprete

- **Evidencia:** fib tarda 3177 ms sin JIT contra unos 100 ms con JIT. En
  `run_until_inner_raw` (`crates/vn-vm/src/exec/dispatch/mod.rs`) cada opcode paga:
  1. `ip >= code_len`.
  2. `debug::check_break` (`crates/vn-vm/src/debug.rs:35`): comparación de
     `debug_skip` y dos `Option`.
  3. `code[ip]` con comprobación de límites.
  4. `OpCode::from_u8` con rama de opcode desconocido.
  5. `compiler_fence(SeqCst)`, introducido en `54129d29`.
  6. **Hipótesis:** un segundo `match op` en los módulos `ops_*.rs` (existen,
     falta confirmar que el bucle principal delega en ellos por opcode).
  7. `record_hotspot_global` en cada carga de global.
- **Cambios, uno por commit, cada uno medido con `VN_NO_JIT=1` en fib y matrix:**
  1. Verificar el bytecode una vez al cargar el chunk (opcodes válidos, saltos
     dentro de rango, último opcode terminador) y quitar 1, 3 y 4 del bucle.
  2. Depuración: bucle distinto cuando hay sesión de depuración activa, elegido
     al entrar al frame; el bucle normal no llama a `check_break`.
  3. `compiler_fence`: averiguar con `git show 54129d29` qué error tapaba,
     corregir esa causa y retirarlo (Ley 14).
  4. Perfilado de globales: mismo tratamiento que la depuración.
- (a) Objetivo: fib sin JIT por debajo de 1500 ms. (b) `VN_NO_JIT=1 vn bench`.
  (c) Las comprobaciones por opcode y el `compiler_fence`.
- **Límite:** Ley 10 no admite `unsafe` por ganancia de compilación; aquí es
  runtime y el verificador de carga es lo que justifica el acceso sin límites.

### R13. Menos `Move` y `LoadNull` emitidos

- **Evidencia:** 23 % y 10 % de los opcodes del corpus.
- **Primer paso:** histograma por patrón (`Move` tras `Call`, `Move` a registro
  de argumento, `LoadNull` de inicialización de frame) sobre `debug -p bytecode`.
- **Cambio:** fusión de copias en el emisor SSA
  (`crates/vn-compiler/src/ssa/emit/`) para el patrón dominante.
- (a) Solo cuenta si baja el tiempo sin JIT; el conteo no es resultado.
  (b) `VN_NO_JIT=1` en fib, matrix, collection_pipeline. (c) Nada.

### R14. Costo de la caché JIT fría

- **Evidencia:** la suite tardó 112 s en la primera corrida tras recompilar y
  40 s en las siguientes. `TIER_LOOP = 1`, `TIER_STRAIGHT = 128`,
  `OSR_BACKEDGES = 1000` (`crates/vn-vm/src/settings.rs:19-21`).
- **Hipótesis:** los 72 s son compilación con Cranelift de funciones con bucle
  que corren una sola vez.
- **Primer paso:** `VN_JIT_CACHE=off vn run tests\main.vn` cronometrado, y
  `vn bench tests\main.vn --profile` para leer `in jit compile`.
- **Cambio (si se confirma):** barrer `VN_JIT_TIER` en 1, 2, 4, 8 con la
  suite y las 25 cargas, y fijar el menor valor que no empeore ninguna carga
  más que su ci95. OSR ya cubre el bucle largo en la primera llamada.
- (a) Suite en frío por debajo de 60 s sin regresión en caliente. (b) Las dos
  mediciones anteriores. (c) Nada.

---

## 4. Fase L — cumplimiento de las leyes

### L1. Ley 17 y 20: la inferencia no cae a `Dynamic` en silencio

- **Evidencia:** `crates/vn-sem/src/infer/ops.rs` devuelve `Type::Dynamic`
  con `unwrap_or(Type::Dynamic)` (líneas 28 y 35) y con `_ => Type::Dynamic`
  (línea 57) cuando los operandos no son numéricos. `SlotKind::Dynamic` es el
  `#[default]` (`crates/vn-types/src/register_meta.rs`). `Dynamic` aparece
  481 veces fuera de tests: emit 140, compiler 110, sem 55, checker 52.
- **Cambio:**
  1. Operación binaria sin tipo resoluble: diagnóstico en el checker, no
     `Dynamic`. Enumerar los casos en lugar de `_ =>` (Ley 7).
  2. `SlotKind` sin `Default`: cada sitio elige de forma explícita.
  3. Cruzar con `docs/dev/audits/DYNAMIC_INVENTORY.md` y sus CSV solo como
     lista de sitios; el estado se toma del código (Ley 12).
- (a) Los 216 `Eq` y 1471 `GetProperty` genéricos del corpus bajan por causa
  conocida. (b) Skill `varn-static-coverage` antes y después, y
  `varn-dynamic-audit` sobre `tests/`. (c) Los fallbacks.
- **Riesgo:** rompe código del corpus que hoy compila (Ley 15 y 18: se corrige
  el corpus). Coordinar con §2.5 del plan unificado.

### L2. `Eq` genérico en generadores

- **Evidencia:** 11 `Eq` genéricos en `bench_generator.vn`; generator pierde
  contra Bun en `compare.ps1`, dato no verificado por checksum.
- **Primer paso:** skill `varn-type-loss-audit` sobre un `Eq` de `pingpong`.
- (a), (b) Añadir `generator` a `xtask compare` y medir. (c) Según la causa.

### L3. Ley 11: tests de Rust y comentarios

- **Evidencia:** 134 `#[test]`: vn-tir 72 (11 archivos), vn-lsp 32 (8),
  vn-cli 8 (5), vn-checker 7, vn-types 5
  (`src/bytecode/tests.rs`), vn-parser 4, vn-pipeline 3, vn-resolver 3;
  `#[cfg(test)]` en `crates/vn-parser/src/lib.rs:9`. Nueve líneas de
  comentario: `crates/vn-sem/src/types/context.rs:22-26` y
  `crates/vn-pipeline/src/class_link.rs:15-18`.
- **Requiere decisión del dueño**, porque la ley y el repositorio se contradicen:
  - Opción A: cumplir la ley. Por cada test, comprobar que `tests/main.vn` o
    los scripts de humo (`scripts\lsp-smoke.js`, `scripts\cli-smoke.ps1`) cubren
    el caso; añadir el caso `.vn` que falte; borrar el test.
  - Opción B: enmendar la Ley 11 para admitir tests de integración por crate
    donde `main.vn` no llega (LSP, CLI, forma del TIR).
- Comentarios: el de `context.rs` describe un contrato que L5 convierte en
  tipo; se borra con L5. El de `class_link.rs` se sustituye por nombres.
- Los comentarios de `Cargo.toml` no son código Rust; fuera de alcance salvo
  que el dueño diga lo contrario.

### L4. Ley 5: pánicos en fases de compilación

- **Evidencia:** 65 `panic!`/`unreachable!`/`todo!` y 265 `unwrap()`/`expect(`
  fuera de tests; el parser tiene 106 de los segundos.
- **Sitios a tratar primero:**
  - `crates/vn-emit/src/body/expr_node.rs:241,281`,
    `expr_map_literal.rs:22,25`, `expr_assign.rs:114`,
    `expr_value_update.rs:125` (`unreachable!`).
  - `crates/vn-compiler/src/from_tir/build/infra.rs:186`,
    `build/ops.rs:36`, `ssa/emit/values.rs:288`.
  - `crates/vn-sem/src/types/interned/resolution.rs:12,25,33,42` y
    `crates/vn-core/src/atom.rs:90`: id ausente de la tabla.
- **Cambio:** clasificar cada sitio. Invariante interna inalcanzable por
  entrada de usuario: se hace irrepresentable en el tipo o pasa a diagnóstico
  interno con código en el `DiagnosticBag`. Alcanzable por entrada de usuario:
  diagnóstico normal.
- **Primer paso para el parser:** listar los 106 `unwrap`/`expect` y separar
  los que siguen a una comprobación local de los que dependen de la entrada.
- (a) Ningún programa de entrada tumba el compilador. (b) Corpus de entradas
  rotas (`tests/helpers/broken-escape.vn` y similares) sin pánico. (c) Los pánicos.

### L5. Ley 2: el contrato de `rehome` pasa al sistema de tipos

- **Evidencia:** `crates/vn-sem/src/types/context.rs:22-26` dice que los
  miembros devueltos pueden llevar `Type` con ids de otra tabla y que quien los
  reciba debe pasarlos por `rehome`. Es una convención, no una garantía.
  `CheckerTyId` aparece en vn-emit (3), vn-lsp (3) y vn-binder (1).
- **Cambio:** un tipo distinto para el `Type` foráneo, sin conversión implícita
  a `Type`; la única salida es `rehome`. Los puntos canónicos
  `reintern_member_type` y `reintern_foreign_ty` pasan a ser los únicos constructores.
- (a) Imposible usar un id contra la tabla equivocada. (b) Compila, y la suite
  pasa en los cuatro cuadrantes. (c) El comentario de contrato.
- **Coordinación:** ADR-0012 y ADR-0013 gobiernan la tabla de tipos.

### L6. Ley 7: revisión de los 152 `_ =>`

- **Evidencia:** vm 28, sem 22, compiler 16, core 16, jit 11, checker 10,
  types 9. `wildcard_enum_match_arm` está en `deny` y no hay ningún `allow`, así
  que los que quedan caen sobre tuplas, enteros, cadenas u `Option`.
- **Primer paso:** clasificar cada uno. Los que cubren una tupla de enums (por
  ejemplo `match (table.get(l.0), table.get(r.0))` en `sem/infer/ops.rs` y los
  de `crates/vn-sem/src/compat/`) esconden la misma decisión que la ley
  prohíbe y el lint no los ve.
- **Cambio:** enumerar las variantes en esos; dejar los de enteros y cadenas.
- (a) Añadir una variante de tipo obliga a decidir en cada sitio. (b) Añadir
  una variante de prueba en una rama local y contar errores de compilación.
  (c) Los comodines sobre enums.

### L7. Ley 16: capacidades por enum

- **Evidencia:** `has_capability(&self, cap: &str)` en
  `crates/vn-types/src/capabilities.rs:62-74` decide por literales, con
  alias duplicados (`"fs.read" | "fs:read"`, `"net.client" | "net:client" | "net.connect"`).
- **Cambio:** enum de capacidad en `vn-core` con una única forma textual;
  el análisis del texto ocurre una vez en la frontera (CLI, manifiesto).
- (a) Una sola tabla. (b) `scripts\cli-smoke.ps1`. (c) Los alias (Ley 8 y 10:
  no hay compatibilidad que conservar) y la comparación por cadena.
- Fuera de alcance: `"true"`/`"false"`/`"null"` en `ctx_csv/parse.rs` y los
  nombres de algoritmo en `crypto.rs` son datos, no identidad del lenguaje.
  Decidir con el dueño si `crypto` debe pasar a enum.

### L8. Corolario 1: estado global

- **Evidencia:** 91 `OnceLock`/`thread_local!`/`lazy_static` fuera de tests.
  En el camino de compilación y ejecución: `vn-jit/src/config.rs:37`
  (`CONFIG`), `vn-vm/src/jit/code_cache.rs:10,23` (`STORE`, `MODE`),
  `vn-core/src/env/mod.rs:20` (`PROCESS`), `vn-jit/src/clif/reloc.rs:60`,
  `vn-types/src/well_known.rs:4`, y `thread_local!` en
  `vn-types/src/value/{shape,instance,class}.rs` y `memory.rs`.
- **Primer paso:** clasificar en (i) constantes de proceso inmutables tras
  iniciar (ISA de Cranelift, tabla de helpers), admisibles; (ii) configuración
  que debería entrar por parámetro (`CONFIG`, `MODE`, `PROCESS`); (iii) tablas
  mutables por hilo (formas, clases, libro externo).
- **Cambio:** (ii) se pasa de forma explícita desde `vn-pipeline`/`vn-cli`.
  (iii) pasa a pertenecer al `Heap` o al `ExecCtx`, lo que además permite dos
  VMs en el mismo hilo.
- (a) Fases re-ejecutables de forma aislada (Ley 5). (b) Suite y
  `tests/helpers/isolate-workers.vn`. (c) Los estáticos de (ii) y (iii).
- **Riesgo:** alto volumen y toca el ABI del JIT (offsets de `young_*`).
  Ejecutar después de R1–R2 para no moverles el suelo.

### L9. Matriz de fases

- **Evidencia:** `vn-compiler` no importa `vn_core::ast` (cumple).
  `crates/vn-vm/src/globals/mod.rs:64` usa
  `vn_core::ast::MetaProperty::Main.slot()`. `vn-checker` no depende de
  `vn-tir`; el TIR lo produce `vn-emit`, que la matriz de `AGENTS.md` no nombra.
- **Cambio:** mover el slot de `MetaProperty::Main` a una tabla de `vn-core`
  fuera de `ast`. Añadir `vn-emit` a la matriz de `AGENTS.md` y corregir la
  fila de `vn-checker`.
- (a), (b) `tgrep 'vn_core::ast' crates/vn-vm crates/vn-jit
  crates/vn-compiler` sin resultados. (c) La dependencia.

---

## 5. Orden y dependencias

| Paso | Items | Depende de | Motivo |
|---|---|---|---|
| 1 | M1, M2, M3 | — | Sin esto no hay veredicto fiable |
| 2 | M4, M5, M6, M7 | M1 | Base estable para todo lo demás |
| 3 | R1 | M6 | Mayor ganancia con el menor cambio |
| 4 | R5, R6 | M6 | Independiente del GC; dos cargas perdedoras |
| 5 | R12 (cuatro commits), R14 | M6 | Independiente; mejora el arranque |
| 6 | R7, R8 | autorización, M6 | Reabre una decisión cerrada |
| 7 | R3, R4, R9 | R1 | Dependen del nuevo comportamiento del vivero |
| 8 | R2, R10, R11 | coordinación con O-items | Zona reservada en §2.4 |
| 9 | L1, L2, L6 | M6 | Rompen corpus; van juntos en el tiempo, no en el commit |
| 10 | L4, L5, L7, L9 | — | Sin efecto en tiempos |
| 11 | L3 | decisión del dueño | Ley contra práctica |
| 12 | L8, R13 | R1, R2 | Mayor volumen, menor urgencia |

## 6. Criterio de terminado

- `xtask compare` sin `OUTPUT MISMATCH` y con las 25 cargas con equivalente.
- Ninguna carga más de 1.2x más lenta que Bun, o con causa escrita y medida.
- `PASSED: N`, `FAILED: 0` en los cuatro cuadrantes; `scripts\verify.ps1` limpio.
- `scripts\bytecode-hash.ps1 -Compare` idéntico tras cada item que no cambie el emisor.
- Cada item de Fase L con su comprobación mecánica en `verify.ps1`, para que
  no se vuelva a abrir.

## 7. Lo que esta auditoría no cubrió

- Ley 3 (dueño único de tablas) y Ley 4 más allá del hash: no se revisó cada
  iteración sobre `FxHashMap` (998 menciones) ni cada tabla compartida.
- Leyes de proceso 9, 13, 19 y 21.
- Memoria pico por proceso: `vn bench` da un pico acumulado de varias corridas
  (194–359 MiB) que no sirve para comparar; falta medirlo por proceso contra
  Bun y Node.
- `task_soak`, `task_spawn`, `gpu_matmul`, `csv_etl`, `now`, `math`,
  `multiple`, `generator`: sin equivalente verificado en `xtask compare` o sin perfilar.
- Cuadrante `VN_STD=@embedded`: no se corrió en esta auditoría.
- Los perfiles se tomaron con el muestreador añadiendo 14–17 % de sobrecarga;
  el de `str_ops` reportó −49.9 %, señal de ruido (ver M5).
