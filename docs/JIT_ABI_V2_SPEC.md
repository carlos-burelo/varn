# JIT ABI v2 — Cimientos nuevos, sin herencia

Estado: diseño cerrado para reescritura total. Sin retrocompatibilidad, sin migración
gradual, sin shims. Se borra la ABI actual entera y se escribe la nueva sobre git.
Si algo de abajo necesita convivir con lo viejo, lo viejo es el bug (Ley 8).

Objetivo medible: máximo rendimiento en estado estable + arquitectura con el mínimo
boilerplate posible. Garantías:

- Cero `offset_of!` / probes fuera de un solo crate (`varn-abi`). Hoy hay ~1000 líneas
  de plumbing (`helper_abi.rs`, `JitHelpers`, `JitFrameLayout`, `frame_layout.rs`,
  `helpers.rs`, `clif/abi.rs`) solo para decirle al JIT dónde vive la memoria.
  Objetivo: ~200 líneas totales de contrato, cero probes, cero macros de lista.
- Cero cargas/indirecciones por llamada en estado estable. Hoy cada llamada rápida
  paga: load de `jit_entry`, stores de `resume_ip`/`call_dest`, handshake
  `frame_prepushed`, recargas de bases tras cada posible realloc. Objetivo: llamada
  directa con dirección horneada, cero stores por llamada, cero recargas.
- Cero `unsafe` nuevo fuera de `varn-abi`. El contrato es `#[repr(C)]` + asserts de
  compilación. Si el JIT necesita aritmética de punteros, el tipo ya la hizo.

## 1. Decisión raíz: poseer la memoria

La causa de todo el boilerplate actual es una sola: la ABI describe el interior de
tipos de Rust que no le pertenecen (`Vec`, `Rc`, `Option<Rc>`, `Cell`, `ExecCtx`).
Cada cambio de `rustc` o reorden de campos es un riesgo, y cada riesgo se cubre con
más probes, más offsets, más macros. La solución no es mejorar los probes. Es dejar
de describir memoria ajena.

La v2 posee su memoria:

- Nuevo crate hoja `varn-abi` (debajo de todo: solo depende de `varn-core`). Es la
  única fuente de verdad del layout caliente (Ley 3, Ley 6). `varn-jit` y `varn-vm`
  lo importan; ninguno lo redefine, ninguno lo re-deriva.
- Los stacks de registros no son `Vec`. Son reservas virtuales (`mmap` /
  `VirtualAlloc`, infraestructura ya existente en `varn-jit/src/mem.rs`) con bases
  estables durante todo el proceso: se reserva grande, se confirma por páginas, el
  puntero base no se mueve jamás. Desaparece la categoría entera de "recargar el
  puntero tras una llamada que pudo realojar". El JIT lee la base una vez por
  función y la conserva en registros.
- Los frames no son `Vec<CallFrame>` con checks de len/cap por push. Son registros
  de activación de tamaño fijo en una arena bump con un solo check de capacidad por
  llamada (branch predecible). Regla única: **el caller empuja siempre**. Se borra
  el handshake `frame_prepushed` (una palabra, una lectura y una escritura por
  entrada, solo para resolver quién empuja).
- `Rc`/`Option`/`Cell` no cruzan la frontera. La ABI solo nombra punteros crudos,
  índices `u32` y los structs de abajo. Lo que la VM necesita del lado Rust
  (`Rc<FrameLayout>`, mapas, contadores) vive en estructuras paralelas que el código
  generado no ve ni direcciona.

## 2. Contrato (todo, no hay más)

Todo lo que el código generado toca cabe en estos tipos. Nada más existe para él.

```rust
// crates/varn-abi/src/lib.rs — el contrato entero.
pub const ABI_MAGIC: u32 = 0x5641524E;
pub const ABI_VERSION: u16 = 2;

#[repr(C)]
pub struct AbiHeader { pub magic: u32, pub version: u16, pub size: u16 }

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AbiCtx {
    pub header: AbiHeader,
    pub epoch: u64,          // contexto/heap para el que vale este código
    pub stacks: AbiStacks,   // 4 bases estables + topes bump
    pub frames: AbiFrameArena,// bump de activaciones
    pub heap: AbiHeap,       // descriptor mínimo (umbral nursery, flag gc)
    pub result: VmValue,     // único canal de retorno boxed
    pub poll: u8,            // safepoint: 0 = seguir, !=0 = slow path
    // ... prohibido añadir campos calientes sin justificación medida
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AbiStacks {
    pub gpr: *mut i64,  pub gpr_end: *mut i64,
    pub fpr: *mut f64,  pub fpr_end: *mut f64,
    pub refs: *mut u32, pub refs_end: *mut u32,
    pub dyn_: *mut VmValue, pub dyn_end: *mut VmValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ActBases { pub bases: [u32; 4] } // índice por SlotClass::index()

#[repr(C)]
pub struct AbiFrame {
    pub closure: *const AbiClosure, // vista C, nunca VmClosure
    pub caller: u32,    // id de activación caller (para unwind)
    pub resume: u32,    // ip de reanudación (SOLO la lee el unwinder)
    pub dest: u16,      // registro destino (SOLO lo lee el unwinder)
    pub bases: ActBases,
}
```

Reglas que no se negocian:

- `static_assert` de tamaño, alineación y offset de cada campo en `varn-abi`. Si
  falla, no compila. Ningún test de arranque, ningún probe por contenido, ningún
  `transmute` de `Vec` para descubrir dónde guarda el puntero.
- El JIT direcciona **solo** dentro de `AbiCtx`. Lo demás (`try_handlers`, loader,
  mapas de constantes, contadores) es memoria privada de la VM.
- `VmValue` no cambia (16 B tag+payload, kinds densos). Es lo único viejo que
  sobrevive, porque es correcto.

## 3. Llamadas: una firma, cero peaje por llamada

### 3.1 Firma única

```text
raw(ctx: *mut AbiCtx, frame: *mut AbiFrame)
```

Sin args en registros. El caller ya materializó los registros del callee en su tramo
de stacks (conoce el layout del callee solo en el caso estático, ver 3.2). El callee
lee sus slots por `bases[clase] + idx`, direcciones estables, sin recargas.
Retorno: escalar unboxed en `rax`/`xmm0` (`Int/Bool → i64`, `Float → f64`), resto
`void` + boxed en `ctx.result`. El wrapper `JitFn` sobrevive **solo** como puerta
VM→JIT (dispatch del intérprete, top-level, OSR-entry). JIT→JIT nunca lo usa.

### 3.2 Estático directo, dinámico por un solo helper

Se borra el fast path con frame-push hand-rolled en Cranelift (`emit_vm_call` y su
inlineo de `CallFrame`, ~el código más complejo del backend). Sustituido por dos
caminos, uno solo existe en estado estable:

- **Estático:** callee conocido en compilación (mismo módulo, proto final, epoch
  fijado en la caché de código). Call-site = `call` directo con dirección horneada.
  Sin load de celda, sin guard por llamada. La validez la garantiza la caché de
  código entera: si el epoch cambia, se invalida todo el lote (un solo compare por
  entrada a función, no por llamada). En una ejecución el epoch es estable, así que
  el costo por llamada es exactamente una instrucción `call`.
- **Dinámico:** todo lo demás (métodos, closures, `dynamic`) pasa por **un solo**
  helper con firma real:

```rust
pub fn invoke_dynamic(ctx: *mut AbiCtx, callee: VmValue, argc: u32) -> VmValue;
```

Los argumentos viajan en una ventana contigua ya preparada por el caller. Sin IC
inlineado a mano en cada call-site: el helper resuelve, ejecuta (entrando al código
compilado si existe) y retorna. Cuando un call-site dinámico se vuelve monomórfico
y caliente, el tiering lo recompila a estático. Dos caminos, cada uno con un solo
mecanismo (Ley 8). Hoy hay cuatro (raw/ wrapper × leaf/frame-aware + fallback +
OSR especial).

### 3.3 Cero stores por llamada

Se borran `jit_resume_ip` + `jit_call_dest` como escrituras por llamada (dos stores
en el camino más caliente del VM por pura instrumentación de unwind). Sustituidos
por **side-table por función**: `{offset_pc_llamada → (resume_ip, dest)}` emitida en
compilación, consultada **solo** por el unwinder en `throw` (frío). El camino rápido
no escribe nada que solo el camino de excepción lea.

### 3.4 Constantes en `.rodata`, no en mapas

Se borra `proto_constants: FxHashMap<ptr-proto, …>` (lookup por hash en camino de
ejecución + retención de `Rc` para evitar reutilización de direcciones). Las
constantes del proto se emiten a una sección read-only del objeto de código y se
direccionan RIP-relativo. La caché de código se etiqueta con el epoch; código de un
epoch nunca ejecuta contra heap de otro. El bug que el mapa con `Rc` retenido
evitaba ("a" + objeto + "b" donde iba un literal) se vuelve imposible por
construcción en vez de por disciplina de retención.

### 3.5 Safepoints de un byte

`ctx.poll` en back-edges y push de frame. No comparan de longitud de nursery
inline. El colector arma el flag; el código generado hace `test al,al; jnz slow`.
Branch predecible, una instrucción, sin operandos de memoria caliente.

## 4. Helpers slow-path: de 120 `usize` a ~25 firmas reales

Se borra `helper_abi.rs` (lista de ~120 nombres con mapeo manual `bit_and →
jit_bitand` que no deriva de nada) y el `fill` macro. Sustituido por:

- Cada slow-path se anota una vez en la VM: `#[jit_slow] fn jit_throw(…)`, etc.
  Un proc-macro genera la tabla, las firmas y los `static_assert` de aridad/tipo
  contra los call-sites Cranelift. Olvidar registrar = error de compilación, no
  salto a 0.
- El JIT importa slow-paths como `FuncRef` Cranelift con firma declarada;
  direcciones resueltas una vez por epoch de caché, no una tabla de `usize` leída
  por offset. `JitHelpers` como struct de 120+ campos **desaparece**. No hay
  reemplazo: es la eliminación lo que da rendimiento (una indirección menos) y lo
  que mata el boilerplate (tres lugares donde escribir cada nombre).
- Familias que hoy se deshabilitan a 0 (`call`, `invoke_virtual`, IC…) no existen
  como flags. Lo no soportado va al helper dinámico genérico. Sin `caps`, sin
  ceros, sin bifurcación por "dirección nula".

Objetivo contable: `grep -c '=> jit_'` pasa de ~120 a 0. Los slow-paths restantes
(~25: throw, alloc, upvalues, módulos, llamadas dinámicas, nativas, GC, suspend…)
se listan en la revisión, no aquí: si uno más puede inlinearse, se inlinea y no
entra en la lista.

## 5. GC y raíces sin `Vec`

- GPR/FPR nunca raíces (por construcción, se mantiene).
- REF/DYN vivos = rangos contiguos de las arenas bump acotados por los topes de las
  activaciones vivas. Sin `len`/`cap` de `Vec`, sin filtrado de sentinelas por
  elemento en el colector rápido (el sentinela `REF_UNINIT` se mantiene como valor,
  pero el barrido va por rango, no por `Vec::len`).
- `FrameStore::collect_roots` se reescribe contra rangos. Los tests de raíces
  existentes se portan 1:1; si alguno necesita `Vec`, el test es el bug.

## 6. Archivos: qué vive, qué muere

Muere (borrado, no deprecado):

- `varn-jit/src/helper_abi.rs` — la lista.
- `JitHelpers`, `JitFrameLayout`, `JitArrayLayout`, `JitObjectLayout`, `JitStrLayout`
  como tablas de offsets — sustituidos por `varn-abi`.
- `varn-vm/src/jit/helpers.rs` (relleno), `varn-vm/src/jit/frame_layout.rs`
  (probes), `probe_vec_words`, aritmética `RcBox - 16` en el JIT.
- `current_exec_ctx` + thread-local, ramas leaf sin contexto, `jit_call_base` /
  `jit_call_closure_ptr`, `stack_data_offset` y familia, `frame_prepushed`,
  `jit_resume_ip`/`jit_call_dest` como escrituras por llamada, bloque de ceros fase B.
- `emit_vm_call` inlineado a mano. El lowering de `Call` emite estático-directo o
  `invoke_dynamic`. Nada intermedio.

Vive (reescrito contra `varn-abi`):

- `varn-jit/src/clif/abi.rs` — la única convención (firma única + puerta VM→JIT +
  side-tables). Archivo pequeño, sin offsets.
- `varn-jit/src/mem.rs` — se extiende de código a stacks: una sola reserva virtual
  por clase. Mismo mecanismo, dos usos (Ley 8).
- `varn-vm/src/frame_store.rs` — adelgaza a bump arenas + metadatos. Límite duro:
  <400 líneas o se parte por dominio (AGENTS.md §6).
- Nuevo `crates/varn-abi/` — el contrato. Límite autoimpuesto: <300 líneas con
  docs. Si no cabe, el modelo es el bug, no el tamaño.

## 7. Validación (sin matriz gradual: todo o nada)

- `cargo test` verde por crate. Tests portados, no duplicados: `class_ptr_offsets`,
  `alloc_bases`, raíces, resume-tras-throw (el caso que la side-table debe cubrir),
  recursión mutua bajo llamada estática.
- `tests/main.vn` en las 4 combinaciones (std `dev-checkout`/`@embedded` ×
  JIT/`VARN_NO_JIT=1`): `PASSED`, `FAILED: 0`, `ALL TESTS PASSED`.
- `.\scripts\verify.ps1 -Fast` final.
- Contables de aceptación de esta propuesta (lo que la hace falsable):
  1. Cero `offset_of!` fuera de `varn-abi` (`grep -r offset_of crates/varn-jit
     crates/varn-vm` vacío salvo `varn-abi`).
  2. Cero `current_exec_ctx`, cero `jit_call_base`, cero `frame_prepushed` en el
     árbol.
  3. `tokei` en los archivos ABI/frame/call baja ≥60% líneas vs base.
  4. Ningún `_ =>` nuevo ocultando variante de `SlotKind`/`SlotClass`/opcode.
  5. Bytecode idéntico entre corridas (Ley 4); fases testeables sin IO (Ley 5).
- Caché: `VARN_CACHE_DIR=<temp>` limpio ante cualquier fallo no reproducible. Con
  código etiquetado por epoch, un fallo que desaparece con caché limpio es bug de
  invalidación de caché, y se persigue ahí.

## 8. Riesgos asumidos (declarados, no escondidos)

- Reserva virtual grande por proceso: overcommit, no commit. Plataformas objetivo
  lo soportan (el backend ya reserva ejecutable en `mem.rs`). Límite configurable
  por `ExecSettings`; test de OOM simulado con reserva mínima.
- Llamada estática horneada retrasa la primera ejecución caliente (hay que compilar
  callee antes de enlazar). Se acepta: el tiering ya espera evidencia antes de
  compilar; enlazar tarde es coherente con compilar tarde.
- `invoke_dynamic` único puede engordar. Si el perfil muestra un megamórfico real,
  se añade **un** stub especializado con su firma, no una familia de variantes.
  Cada añadido declara ganancia medida (Ley 10).
