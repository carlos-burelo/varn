# Plan único (living, 2026-10-16)

Sustituye a `2026-10-09-plan-unificado.md`,
`2026-10-09-representacion-un-bloque-por-valor.md`,
`2026-10-10-propagacion-de-tipos.md`, `2026-10-12-plan-b1-resume-jit.md`
y `2026-10-12-try-catch-jit-puro.md` (eliminados; lo pendiente vive
aquí, lo hecho queda en git). Memoria viva: actualizar con cada item
verde. Si contradice al código, manda el código (Ley 12: las cifras
las produce la auditoría, no se copian).

> Validación: `.\scripts\verify.ps1 -Fast` (matriz 4 cuadrantes,
> release); iteración `.\scripts\verify.ps1 -Quick`; sin JIT
> `VARN_NO_JIT=1 vn run tests/main.vn`.

## 0. Reglas

- Cada item declara (a) ganancia, (b) medida, (c) qué se borra (Ley 10).
- Bench pareado mismo perfil + checksum antes que tiempo; `quick`
  para iterar, `release` para números contra rivales (Ley 13).
- Un commit atómico por item, en `main` (Ley 9). Sin re-litigar
  lo cerrado (§4).
- Un item sin ganancia medida se aparca, no se insiste.

## 1. Cerrado (log compacto; detalle en git)

Tipos checker→JIT (sesión 2026-10-15/16): EqStr/NeqStr nativo
(`3ee1d01d`); narrowing conserva Tuple/Array (`004ce48f`);
destructuring de tupla por posición (`e8e1717d`); local `dynamic`
anotado se conserva (`8a1a06d0`); matches exhaustivos sin comodines
(`3cb9ea86`); SSO 5→8 (-20% str_ops, -55% allocs, `56f493e0`);
`BytesGetIndex` nativo (-88%, `9c311da9`) + vistas (`72029f3e`);
`BytesSetIndex` nativo (`5bc54ab4`); NYS comprueba al cruzar a
concreto (`b36bcf34`); `char<char` (`f72e14f1`); return-generador
contra lo producido (`5aea3bde`); literal foráneo sustituye
(`764deef1`); affix-inline implementado y revertido (`612f295a`,
`e6edae1a`, ver §4).
Representación: `678580d4` (upvalues en BuildObject), `1263145b`
(Map/Set en celda), `0b963319` (length SSO en chars), `4e08159d`
(tupla literal es tupla).
Unificado: C1 class_fields ~2.9× (`f91f7ee2`); C2-overflow
matrix 1.22× (`1f3a8e05`); cimientos O ajenos verdes; B0
checksums JIT/NOJIT.
Try/catch: sync+async en JIT puro, `jit_error_flag` único,
`JIT_HELPERS_VERSION = 4`, gates try-en-loop e imported-188
pendientes (§3).
Estado: suite 3522/3522 en 4 cuadrantes release; cero abandonos
reales del JIT.
Arranque y stdlib (2026-10-16): DLLs GPU/UI con delay-load (`f06ec0c6`,
`--version` 68→11 ms); huella única xxh3 (`50f70b2c`); imagen std
indexada sin copia (`f479aaca`); checker lee interfaces std+core de la
imagen (`37aecac8`, frío 92→19 ms); core ve `core:errors` (`c2f460e3`);
dev-checkout corre de imagen en caché, el árbol solo la construye
(`aeb1cff7`); interfaz canónica solo-importador (`1f264fde`, 2.25 MB→
512 KB, imagen idéntica fría/tibia). `run` vacío < 1 ms sobre
`--version`.
Caché JIT persistente (2026-10-16): lowering con relocaciones para todo
valor del proceso (`5577e184`, lowering puro sin linker) y caché de
imágenes en `<cache>/jit/` con modo `VARN_JIT_CACHE=verify` como
cuadrante 5 de `verify.ps1` (`5489ec54`). http_routing 262→227 ms
(bun 219).
Globales top-level a registros del módulo (regresión de la migración
HIR→TIR): walker TIR exhaustivo (`7a11028a`) y pase de promoción en
varn-emit (`5e652ef0`); class_fields 6.4→3.6 ms.
Módulos/LBI/ABI (2026-10-16): el macro parsea el contrato (`78a47de6`,
borra contracts.json y gen-contract-tables); build.rs con el parser real
(`9f23745a`, AST de namespace guarda `declare` `f9af54e6`); registro
estático único en lugar de secciones del linker y markers (`0cd1f048`);
permisos `@permission` en el contrato (`9394b438`); ids de op densos con
`KnownOp` reservados (`b88a69e3`); un solo resolvedor de especificadores
(`e6528ef3`, arregla doble evaluación por mayúsculas), un solo lector de
fuente (`8a5a0217`), provider con `module()` (`c546a8b5`); ABI v2 muerta
borrada (`877294fc`). `NATIVE_GLOBALS` queda como única tabla ABI.

## 2. Pendiente, por orden

### 2.1 Representación: un bloque por valor

Cada fase: re-medir filas del probe `repr_rows.vn` antes (paso 1),
pareado ambos tiers; sin fila en bitácora no cierra. Orden lo
decide la medición, no esta lista.

- **Fase A — clave Map O(1)**: bit «internado» en `HeapStr` +
  `canonical_map_key`; borra canonicalización por interner en cada
  acceso (`heap/str.rs`, `heap/strings.rs`, `heap/map_keys.rs`).
  Prueba: ampliar test 160 (misma clave por concat/slice/literal).
- **Fase B — arrays/split un bloque**: elementos en cola de celda
  + derrame; `split_ranges` única fuente
  (`vm_value/{array,vm_array,vm_array_access}.rs`,
  `heap/cells/typed.rs`, `heap/jit.rs`). Prueba: suite nueva
  (push oltre inline, I64→Boxed, pop/shift, split vacío/multi).
- **Fase C — vistas string sin `Arc`**: vista referencia celda
  padre trazada por GC (`heap/str.rs`, `heap/children.rs`).
  Prueba: vista sobrevive a padre, realloc, GC en medio,
  `spawnIsolate`.
- **Fase D — objetos literales inline** (compuerta: `objlit3` >
  1,5× `instance3` tras A–C): JIT toma celda sin helper; borra
  `jit_build_object_with_shape`, `jit_build_record_with_shape`
  (`value/shape.rs`, `value/object/*`, `from_ssa/{classops,heap}.rs`).
- **Fase E — tipos objeto exactos como instancias** (compuerta: D):
  graduar `DynReason::Shape` a variante `BackendTy` con layout;
  decidir flujo tipado→`dynamic`→agregar-campo (prohibir o
  desoptimizar). Prueba: test 171 sin cambios + campos en loop.
- Modos de falla por fase: callback que muta Map/Set en forEach,
  crecimiento oltre inline, vista+GC, cruce isolates, shapes host.

### 2.2 B1: reanudar async/generadores en JIT

Verde si B0 + 4 cuadrantes + soak + A/B con ganancia en
`pingpong` y cadena `await`; sin ganancia se aparca. No tocar
`dispatch/mod.rs:73` (`ip == 0`), ni `direct_entry`, ni
Freeze/thaw (B2, otra sesión); tabla nueva cae en
`BUILD_FINGERPRINT`.

- **B1a**: tabla `resume_ip`→bloque en SSA portable + decidir
  params-args vs solo-homes+`live`. Verifica: dump de
  `bench_generator.vn`, sin cambio de ejecución.
- **B1b**: `resume_at(ip)` en `drive.rs` (una sola tabla con
  loop_headers, Ley 8). Verifica: `osr_entries` sube en
  `pingpong`, B0 intacto.
- **B1c**: scheduler pide entrada (`thaw`+`Delivery` con
  `Settled(Rejected)`), quitar veto `tiering.rs:144` solo
  reanudación. Verifica: tests 154+77+20/21, soak 3000 sin fugas.
- **B1d**: `CatchParam`, try/`await`, yield delegado, GC nativo,
  fuzz×GC, AOT vetado. Verifica: 4/4 + A/B final.

### 2.3 Try/catch: gates que quedan (bail a intérprete)

1. **try-en-loop** (`drive.rs`): quitarlo rompe 78
   (`EntryLivein`) y sin loop-opts rompe 179. Causa: dispatch
   compartido fuera del loop.
2. **imported-188** (parche reconocido): sin él, 188 falla por
   dispatch fantasma en catch nativo `Headers`; vía `run` pasa,
   bajo `test` no. Aislar interacción framework/tasks.

### 2.4 Rendimiento JIT (gaps vs rivales, release)

- **O1–O7, B1–B3 en otra sesión (no tocar sin coordinar)**:
  bump-alloc nursery, tax llamada alloc-free, str inline,
  tip-append, Map literal/get, Dyn-compare fast-path, bails
  Dynamic→Int, generadores/tasks/endurecimiento. Zona strings
  solapa con SSO-8/`BytesGetIndex` ya verdes arriba: coordinar
  antes de pisar.
- **C4 str_ops / C3 http_routing / C5 json**: bloqueados tras
  O-items (diseños en doc viejo; revalidar números en release).
- **C6**: re-medir csv/collection; item propio solo si gap
  >1,2× con causa. **C2-bounds/C1-hoist**: aparcados con
  medición. **`matrix_local`**: duplicado de `matrix`, dedupe
  con dueño de benches.
- No tocar: fib/dto/json_pure/gc_alloc (ganan o empatan);
  task_soak/spawn 100% intérprete hasta B1–B3.

### 2.5 Tipos: restante con causa

- **P3 enum-Eq / P4 Decimal-BigInt**: aparcados con medición
  (mismo helper, ~5ns; P4 pide SlotKind+VM+GC).
- **2.3 DynReason**: no graduar (decoración; el backend solo
  necesita estructural-o-no).
- **2.4/2.5/2.6**: layout fijo (Fase E), sin inline foráneo
  (registro global), costo try (proyecto async).
- **2.8 despacho** (6ns vs 81ns medido, 13,5×): fusión
  verify/despacho o memo con GC; sin diseño, proyecto runtime.
- **Genéricos en calls**: proyecto secuenciado (inferencia
  anidada + defaults importados + error VN3201); el error solo
  rompía 500 sitios honestos. No parche a medias.
- **Struct en namespace**: bug real con repro (check pasa,
  runtime rechaza `Geo::Point`): toca registro de shapes
  cualificados.
- **Campo-sin-init**: proyecto definite-assignment (lectura
  `null` actual como ínterin documentado).
- **Task flotante**: diferido (falta supresión + lifecycle B2).
- **Decidido sin código**: `int`→`float` variable se rechaza
  (pérdida de precisión, mensaje preciso); `this`-antes-`super`
  sound por allocate-first; `??` ya es VN3027/VN3028;
  labels+finally verificado; condicionales/mapeados y
  task-values de sus dueños.
- **Deuda declarada**: `Union`→estructural comprueba (solo NYS
  se hizo); `case` +9% sin mecanismo (ruido hasta réplica).

## 3. Validación (puerta)

`.\scripts\verify.ps1 -Fast` release 4 cuadrantes antes de
commitear; `tests/errors/*.vn` sin nuevos fallos; cada bench con
checksum antes que tiempo; A/B pareado mismo perfil, bloques
alternos, descartar primera.

## 4. Decisiones cerradas (no re-litigar)

NaN-boxing `VmValue` 8B; Map SmallVec; `EqChar/EqEnum`;
startsWith-SSO-inline (medido dos veces, cero a nivel fila);
sin-inline-foráneo; `DynReason` sin graduar; P4 bloqueado;
`matrix_local` duplicado; `new` sin deducir = VN3201 (extender
a calls exige el proyecto de §2.5, no un parche).
