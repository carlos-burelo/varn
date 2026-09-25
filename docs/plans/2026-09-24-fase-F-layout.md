# Fase F — Layout

> Cada paso compila, deja `tests/main.vn` verde en JIT y `VARN_NO_JIT=1`
> (caché limpio, también con `VARN_STD=@embedded`), corpus negativo y goldens
> verdes, y es un commit. Spec §12, §47–§51, §96, §101.

**Goal:** el layout de un campo o valor lo describe un único `TypeLayout`
(tamaño, alineación, representación) y el GC recorre instancias por su
`GcLayout` (posiciones de referencias), no inspeccionando cada valor.

## Situación medida (2026-09-24, `b025c6e8`)

- `class_field_repr(kind) -> (size, align, is_gc_ref)` es la tabla única
  (Fase C.5); `FieldLayout` guarda `size/align/is_gc_ref`, y quien lee o
  escribe un campo (runtime `InstanceData::read_field/write_field`, JIT
  `clif/fields.rs`) re-deriva la representación de `kind` + `is_gc_ref` +
  `size == 8`.
- `ClassLayout.gc_mask: u64` se calcula y **nadie lo lee**; tope de 64 palabras.
- El GC (marcador, nursery, chequeo de nacimiento) recorre una instancia
  leyendo **todos** sus campos (`field_at`) y preguntando a cada valor si es
  referencia.
- Arrays: `ArrayRepr::{Boxed, I64, F64}` ya existe; `Array<int>`/`Array<float>`
  no se trazan.
- `T?` sobre clase como campo: `field_kind(Nullable(_)) = None` → `VmValue`
  de 16 bytes, aunque la representación compacta de referencia ya codifica
  `null` (`COMPACT_REF_UNINIT`).
- Registros: `HirType::Nullable` → `SlotKind::Dynamic`; un `VmValue` es
  (payload, tag): la pareja (valor, bit) ya es su representación.

## Pasos

### F.1 `TypeLayout` y `GcLayout`
Módulo `varn_types::layout`: `ScalarRepr { Bool, I64, F64, Ref, Boxed }`,
`TypeLayout { size, align, repr }` (`TypeLayout::of_field(kind)` sustituye a
`class_field_repr`), `GcLayout` = lista de `(offset, GcSlot::{Ref, Boxed})`.
`FieldLayout` lleva su `TypeLayout`; `ClassLayout.gc: GcLayout` sustituye a
`gc_mask`. Runtime y JIT leen/escriben por `repr`, no re-derivan.

### F.2 GC por `GcLayout`
Marcador, actualización del nursery y chequeo de nacimiento recorren sólo las
posiciones de `GcLayout`: un `Ref` compacto se lee como índice (sin
`VmValue`), un `Boxed` como `VmValue`. Los campos `int/float/bool` no se
tocan. Test: instancias con referencias sobreviven colecciones; medición
con una clase de muchos campos escalares.

### F.3 Nicho de referencia para `T?`
`field_kind(Nullable(T))` con `T` de representación `Ref` es `T`: el campo es
un índice de 8 bytes y `null` es el nicho `COMPACT_REF_UNINIT`. Test:
campos `Foo?`, `str?`… (los que compactan) leen/escriben `null`.

### Decisiones
- Uniones: el `VmValue` (payload, tag) es la unión con discriminante; sin
  layout propio hasta que exista una unión que compacte (§50).
- `T?` escalar en registros: ya es (valor, tag) en un slot `Dynamic`; tiparlo
  en registros propios es Fase G (ejecución tipada).
- Packing `Array<int>` en anchos angostos: tras Fase L (prueba de rango).

## Ejecución (2026-09-24)

| Paso | Commit | Nota |
|---|---|---|
| F.1 | `d849077` | `varn_types::layout`: `ScalarRepr`, `TypeLayout`, `GcLayout`; `gc_mask` borrado |
| — | `b063678` | hallazgo: `build_classes` recorría las clases en orden alfabético suponiendo padre-antes-que-hijo; `class Horse extends Zebra` re-declaraba los campos heredados y tumbaba la VM (`ClassObj::declare_field`) |
| — | `02c42ba` | hallazgo: el constructor no está en la vtable y su firma TIR era `Dynamic` en todos los parámetros; `-i` con `i: int` fallaba la verificación SSA y abortaba la compilación |
| — | `a50c2dd` | `InstanceData` a `value/instance.rs` (object.rs pasaba de 900 líneas); accesores angostos muertos borrados |
| F.2 | `529352e` | marcador, fixup del nursery y chequeo de nacimiento recorren `GcLayout`, con una búsqueda de layout por instancia (antes una por campo). Clase de 16 escalares + 1 referencia, 1M instancias: 1.06 s → 0.91 s con `VARN_NO_JIT=1`; con JIT, dentro del ruido |
| F.3 | `3efb63d` | `T?` sobre una referencia compacta usa el slot de 8 bytes de `T`; la alocación (intérprete y `new` inline del JIT) escribe el nicho en cada slot `Ref` |

Desviaciones y deuda:
- Un campo escalar nunca asignado (`n: int` sin constructor que lo pruebe)
  es `int | null` para el checker (`is_optional`) y queda boxed; lee
  `null`, no `0`.
- `holds_nursery_ref` inscribe siempre `EnumVariant`/`Map`/`Set` en
  `remembered` al nacer (sin layout propio).
- `jit_alloc_instance_fast` entrega un payload a ceros: quien lo use sin el
  `ref_slots` de `ClifClassTarget` deja slots `Ref` apuntando al índice 0.
