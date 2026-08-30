//! Atribución del coste de crear un objeto, tramo por tramo.
//!
//! Existe porque el total —~60 ns por objeto, contra los 24 ns de Bun— se sabe
//! medir pero no se sabe repartir, y las piezas candidatas valen 10-15 ns cada
//! una: por debajo de lo que este host resuelve con wall-clock (deriva del 36 %
//! entre corridas, ±23 % entre repeticiones). Sin este desglose, rediseñar la
//! representación del heap sería apostar.
//!
//! Cuenta CICLOS, no nanosegundos, y con `rdtsc` en vez de `Instant`: el
//! overhead de `Instant::now()` (~25 ns) ahoga por completo un tramo de 5 ns.
//! `rdtsc` cuesta unas decenas de ciclos y ese coste se calibra y se resta.
//!
//! Apagado no cuesta nada: un `bool` en TLS que el predictor acierta siempre.
//! Se enciende con `VARN_ALLOC_PROFILE=1` y se vuelca al terminar el programa.

use std::cell::Cell;

/// Tramos del camino de creación de un objeto. El orden es el de ejecución.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Seg {
    /// Todo el helper, desde que el código compilado entra hasta que sale.
    /// Lo que quede tras restar los demás tramos es el coste del propio cruce
    /// más lo no instrumentado.
    HelperTotal = 0,
    /// `FunctionProto::resolved_shape`: `RefCell` + búsqueda + `Rc::clone`.
    ShapeLookup = 1,
    /// El barrido que busca closures entre los campos para cerrar sus upvalues.
    ClosureScan = 2,
    /// `ObjRef::with_shape_slice` — la asignación DST y la copia de campos.
    ObjDataAlloc = 3,
    /// `Heap::alloc`: mover el `HeapObj` de 48 bytes y empujarlo al nursery.
    HeapPush = 4,
    /// Camino de clase: resolver el constructor (`RefCell`, `downcast`, clones).
    CtorResolve = 5,
    /// Camino de clase: montar y desmontar el frame de la llamada.
    CtorFrame = 6,
}

const N: usize = 7;

pub const NAMES: [&str; N] = [
    "helper (total)",
    "  shape lookup",
    "  closure scan",
    "  ObjData alloc",
    "  heap push",
    "ctor resolve",
    "ctor frame",
];

/// 0 apagado · 1 sólo los totales · 2 con desglose.
///
/// El nivel 1 existe porque medir los sub-tramos mete ocho lecturas de `rdtsc`
/// DENTRO del total, y ese coste no se puede descontar desde dentro. La
/// diferencia entre el total del nivel 1 y el del nivel 2 es exactamente lo que
/// cuesta instrumentar.
///
/// Global atómico y no `thread_local`: la comprobación está en el camino de
/// creación de TODO objeto, y un acceso TLS ahí costaba un 6 % medible con el
/// perfilado apagado. Una carga relajada que el predictor acierta siempre no
/// cuesta nada. Los contadores sí son TLS — se tocan sólo cuando está
/// encendido.
static LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

thread_local! {
    static CYCLES: [Cell<u64>; N] = Default::default();
    static HITS: [Cell<u64>; N] = Default::default();
    /// Coste de un par `read()`/`read()` vacío, para restarlo de cada tramo.
    static OVERHEAD: Cell<u64> = Cell::new(0);
}

#[inline(always)]
pub fn enabled() -> bool {
    LEVEL.load(std::sync::atomic::Ordering::Relaxed) > 0
}

/// ¿Medir también los sub-tramos? Ver [`LEVEL`].
#[inline(always)]
pub fn detail() -> bool {
    LEVEL.load(std::sync::atomic::Ordering::Relaxed) >= 2
}

#[inline(always)]
pub fn read() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // Sin serializar a propósito: serializar cuesta más que varios de los
        // tramos que se miden. Para atribución relativa sobre millones de
        // muestras el reordenamiento se promedia.
        unsafe { core::arch::x86_64::_rdtsc() }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

/// Anota `end - start` en `seg`, descontando el coste de leer el contador.
#[inline(always)]
pub fn record(seg: Seg, start: u64, end: u64) {
    let raw = end.saturating_sub(start);
    let net = raw.saturating_sub(OVERHEAD.with(|o| o.get()));
    let i = seg as usize;
    CYCLES.with(|c| c[i].set(c[i].get() + net));
    HITS.with(|h| h[i].set(h[i].get() + 1));
}

/// Calibra el coste de la propia medición. Se llama una vez al encender.
fn calibrate() {
    let mut best = u64::MAX;
    for _ in 0..1000 {
        let a = read();
        let b = read();
        best = best.min(b.saturating_sub(a));
    }
    OVERHEAD.with(|o| o.set(best));
}

/// Vuelca la tabla. La llama el CLI al terminar; no imprime nada si el
/// perfilado está apagado o no se registró ninguna muestra.
pub fn report() {
    if !enabled() {
        return;
    }
    let hits = HITS.with(|h| h[Seg::HelperTotal as usize].get());
    let ctor_hits = HITS.with(|h| h[Seg::CtorResolve as usize].get());
    if hits == 0 && ctor_hits == 0 {
        return;
    }

    eprintln!("\n  Atribución de la creación de objetos (ciclos por muestra)");
    eprintln!("  overhead de medición descontado: {} ciclos/tramo", OVERHEAD.with(|o| o.get()));
    eprintln!("  ─────────────────────────────────────────────────────────");
    for i in 0..N {
        let n = HITS.with(|h| h[i].get());
        if n == 0 {
            continue;
        }
        let total = CYCLES.with(|c| c[i].get());
        eprintln!(
            "  {:<18} {:>8.1} ciclos   ({} muestras)",
            NAMES[i],
            total as f64 / n as f64,
            n
        );
    }

    // Lo que el helper gasta y ninguno de sus tramos explica.
    let ht = HITS.with(|h| h[Seg::HelperTotal as usize].get());
    if ht > 0 {
        let total = CYCLES.with(|c| c[Seg::HelperTotal as usize].get()) as f64 / ht as f64;
        let mut parts = 0.0;
        for s in [Seg::ShapeLookup, Seg::ClosureScan, Seg::ObjDataAlloc, Seg::HeapPush] {
            let n = HITS.with(|h| h[s as usize].get());
            if n > 0 {
                parts += CYCLES.with(|c| c[s as usize].get()) as f64 / n as f64;
            }
        }
        eprintln!(
            "  {:<18} {:>8.1} ciclos   (total menos tramos: cruce y no instrumentado)",
            "  resto", total - parts
        );
    }
}

/// Lee `VARN_ALLOC_PROFILE` y calibra. La llama el CLI al arrancar; hasta
/// entonces el perfilado está apagado y no cuesta nada.
pub fn init() {
    let level = std::env::var("VARN_ALLOC_PROFILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0u8);
    LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
    if level > 0 {
        calibrate();
    }
}
