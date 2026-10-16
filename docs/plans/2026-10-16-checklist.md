# Checklist plan único (living, 2026-10-16)

> Actualizar con cada item verde (commit hash + medida). Fuente: `2026-10-16-plan-unificado.md`. Código manda.

## 2.1 Representación un bloque por valor
- [x] Fase A — clave Map O(1) (`heap/str.rs`, `heap/strings.rs`, `heap/map_keys.rs`) — VERDE `52feaf6d`
  - [x] Re-medir `repr_rows.vn` base (JIT + NOJIT, pareado)
  - [x] Bit internado en `HeapStr` + `canonical_map_key` única vía
  - [x] Borrar canonicalización por interner en cada acceso
  - [x] Prueba: test 160 ya cubría concat/slice/literal (líneas 72-81), sin ampliar
  - [x] `verify.ps1 -Fast` pendiente antes de push (quick 3522/3522 JIT y NOJIT)
  - Medida quick: same-key 5.5→3.7ms (-30%); fresh-key sin cambio
- [ ] Fase B — arrays/split un bloque (`vm_value/array*`, `heap/cells/typed.rs`, `heap/jit.rs`)
  - [ ] Re-medir base
  - [ ] Elementos en cola de celda + derrame; `split_ranges` única fuente
  - [ ] Prueba suite nueva (push oltre inline, I64→Boxed, pop/shift, split vacío/multi)
  - [ ] `verify.ps1 -Fast` 4/4 + commit
- [ ] Fase C — vistas string sin `Arc` (`heap/str.rs`, `heap/children.rs`)
  - [ ] Re-medir base
  - [ ] Vista referencia celda padre trazada por GC
  - [ ] Prueba: sobrevive a padre, realloc, GC en medio, `spawnIsolate`
  - [ ] `verify.ps1 -Fast` 4/4 + commit
- [ ] Fase D — objetos literales inline (compuerta: `objlit3` > 1,5× `instance3` tras A–C)
  - [ ] Medir compuerta (si no abre, aparcar con cifra)
  - [ ] JIT toma celda sin helper; borrar `jit_build_object_with_shape`, `jit_build_record_with_shape`
  - [ ] `verify.ps1 -Fast` 4/4 + commit
- [ ] Fase E — tipos objeto exactos como instancias (compuerta: D)
  - [ ] Graduar `DynReason::Shape` a `BackendTy` con layout
  - [ ] Decidir tipado→`dynamic`→agregar-campo (prohibir o desoptimizar)
  - [ ] Prueba: test 171 sin cambios + campos en loop
  - [ ] `verify.ps1 -Fast` 4/4 + commit

## 2.2 B1 reanudar async/generadores en JIT
- [ ] B1a tabla `resume_ip`→bloque en SSA portable (params-args vs solo-homes+`live`)
  - [ ] Dump `bench_generator.vn`, sin cambio ejecución
- [ ] B1b `resume_at(ip)` en `drive.rs` (una tabla con loop_headers)
  - [ ] `osr_entries` sube en `pingpong`, B0 intacto
- [ ] B1c scheduler pide entrada (`thaw`+`Delivery` `Settled(Rejected)`), quitar veto `tiering.rs:144` solo reanudación
  - [ ] Tests 154+77+20/21, soak 3000 sin fugas
- [ ] B1d `CatchParam`, try/`await`, yield delegado, GC nativo, fuzz×GC, AOT vetado
  - [ ] 4/4 + A/B final (si sin ganancia en `pingpong`/cadena `await`, aparcar)

## 2.3 Try/catch gates
- [ ] Gate 1 try-en-loop (`drive.rs`): dispatch compartido fuera del loop (rompe 78 `EntryLivein`, 179 sin loop-opts)
- [ ] Gate 2 imported-188: dispatch fantasma catch nativo `Headers` (run pasa / test no; aislar framework/tasks)

## 2.4 Rendimiento JIT
- [ ] O1–O7, B1–B3: NO TOCAR (otra sesión)
- [ ] C4 str_ops / C3 http_routing / C5 json: bloqueados tras O-items (revalidar números release)
- [ ] C6: re-medir csv/collection (solo si gap >1,2× con causa)
- [ ] C2-bounds/C1-hoist: aparcados con medición
- [ ] `matrix_local`: dedupe con dueño benches (duplicado de `matrix`)

## 2.5 Tipos restante
- [ ] P3 enum-Eq / P4 Decimal-BigInt: aparcados con medición
- [x] 2.3 DynReason: no graduar (cerrado)
- [ ] 2.8 despacho (6ns vs 81ns, 13,5×): fusión verify/despacho o memo con GC (proyecto runtime, sin diseño aún)
- [ ] Genéricos en calls: inferencia anidada + defaults importados + error VN3201 (no parche a medias)
- [ ] Struct en namespace: bug real (`Geo::Point` check pasa / runtime rechaza) → registro shapes cualificados
- [ ] Campo-sin-init: proyecto definite-assignment (`null` actual ínterin)
- [ ] Task flotante: diferido (falta supresión + lifecycle B2)
- [ ] Deuda: `Union`→estructural (solo NYS hecho); `case` +9% (ruido hasta réplica)

## Progreso
- 2026-10-16: checklist creado. Sin items verdes nuevos.
- 2026-10-16: Fase A verde (`52feaf6d`). Sigue Fase B.
