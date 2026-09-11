//! Live trace of the minor (nursery) collector, one line per collection.
//!
//! Existe porque el resumen de fin de proceso (`vn debug -p gc`) dice CUÁNTO
//! se recolectó en total, pero no CUÁNDO ni CADA CUÁNTO — para eso hace falta
//! ver cada colección en el momento en que ocurre. Mismo patrón que
//! [`crate::alloc_profile`]: variable de entorno, atómico global, cero costo
//! apagado (una carga relajada que el predictor acierta siempre).
//!
//! Se enciende con `VARN_GC_TRACE=1`. La colección menor es rara — dispara
//! cada ~49k asignaciones ([`crate::nursery::Nursery::FULL_THRESHOLD`]), no en
//! el camino caliente de cada objeto — así que usa `Instant`, no `rdtsc`: el
//! overhead de `Instant::now()` (~25 ns) es irrelevante frente al costo de la
//! colección misma (miles de objetos escaneados).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Lee `VARN_GC_TRACE`. La llama el CLI al arrancar, junto a
/// `alloc_profile::init`; hasta entonces el trazado está apagado.
pub fn init() {
    ENABLED.store(
        std::env::var_os("VARN_GC_TRACE").is_some(),
        Ordering::Relaxed,
    );
}

#[inline(always)]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// State captured just before a minor collection starts, handed back to
/// [`note_end`] so the "before" numbers don't need a second read.
pub struct Before {
    t0: Instant,
    objects_before: usize,
    promoted_before: u64,
}

/// Free to call unconditionally — cheap even when tracing is off, since all
/// it does is read the clock and two counters the caller already has handy.
#[inline]
pub fn note_start(objects_before: usize, promoted_before: u64) -> Before {
    Before {
        t0: Instant::now(),
        objects_before,
        promoted_before,
    }
}

/// Prints one line: collection number, objects that were live in the
/// nursery going in, how many survived by being promoted, how many were
/// garbage (reclaimed outright), and how long the collection took. The
/// nursery is a copying collector — every collection empties it completely
/// (`Nursery::collect` ends with `self.objects.clear()`), so there is no
/// "survived in place" bucket: an object either gets promoted or it's gone.
#[inline]
pub fn note_end(before: Before, collection_no: u64, promoted_after: u64) {
    if !enabled() {
        return;
    }
    let elapsed = before.t0.elapsed();
    let promoted_this = promoted_after - before.promoted_before;
    let reclaimed = before.objects_before.saturating_sub(promoted_this as usize);
    eprintln!(
        "[gc] minor #{collection_no}: in={} promoted={promoted_this} reclaimed={reclaimed} took={:.3}ms",
        before.objects_before,
        elapsed.as_secs_f64() * 1000.0,
    );
}
