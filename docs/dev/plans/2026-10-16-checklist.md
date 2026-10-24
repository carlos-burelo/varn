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
- [ ] Fase B — arrays/split un bloque (`vm_value/array*`, `heap/cells/typed.rs`, `heap/jit.rs`) — APARCADA CON MEDICIÓN 2026-10-16
  - Medida quick: push 200k 1.41ms, get 0.33ms, set 0.28ms, trans 1.07ms; slice 20×100k 15.3ms pero slice es COPIA semántica (sonda: mutar slice no afecta original) → vista exigiría CoW, subsistema nuevo
  - `gc_alloc` ya en verde (§2.4): sin gap medido que lo pague. Reactivar solo si bench muestra gap en alloc-arrays
- [ ] Fase C — vistas string sin `Arc` (`heap/str.rs`, `heap/children.rs`) — APARCADA CON MEDICIÓN 2026-10-16
  - `Arc::clone` es inc atómico (~ns); quitarlo pide trazado GC del padre (riesgo UAF) por ganancia ~nula en tiempo (solo retención de memoria vista-pequeña-de-string-grande). Reactivar solo con caso de memoria medido
- [x] Fase D — objetos literales inline (compuerta: `objlit3` > 1,5× `instance3`) — VERDE (v1 solo Object, records van slow)
  - [x] Compuerta medida: repr_rows objlit3/instance3 = 51/25 = 2,04× → abre
  - [x] JIT emite alloc bump + header + `HeapObj` + `ObjData` inline; borra solo el uso del helper en vía fast (helper queda como fallback slow; `jit_build_*_with_shape` NO se borran aún: records + fallback los usan)
  - [x] Claves del fix: `ObjData` va tras `HeapObj` completo (`data_off`), forma es CONTROL (`as_ptr-16`), `ObjRef` gordo necesita `len` (`payload_len_off`); todo con tripwires en `heap/jit.rs`
  - [x] Medida quick: push3 ~12%, repr_rows objlit3 51→42 (~18%); checksums iguales
  - [x] Suite 3522/3522 JIT y NOJIT + 14 sondas crash + `VARN_ALLOC_SLOW` intacto
  - Deuda v1: records por helper; `payload_len_off`/`data_off`/`JIT_HELPERS_VERSION=5` quedan como ABI
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
- 2026-10-16: Fase D verde (v1 Object-only). Fases B/C aparcadas con medición (sin gap que pague rewrite). Sigue Fase E (proyecto grande) / B1 / gates.
