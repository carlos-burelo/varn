use std::cell::Cell;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Seg {
    HelperTotal = 0,

    ShapeLookup = 1,

    ClosureScan = 2,

    ObjDataAlloc = 3,

    HeapPush = 4,

    CtorResolve = 5,

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

static LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

thread_local! {
    static CYCLES: [Cell<u64>; N] = Default::default();
    static HITS: [Cell<u64>; N] = Default::default();

    static OVERHEAD: Cell<u64> = const { Cell::new(0) };
}

#[inline(always)]
pub fn enabled() -> bool {
    LEVEL.load(std::sync::atomic::Ordering::Relaxed) > 0
}

#[inline(always)]
pub fn detail() -> bool {
    LEVEL.load(std::sync::atomic::Ordering::Relaxed) >= 2
}

#[inline(always)]
pub fn read() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        unsafe { core::arch::x86_64::_rdtsc() }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

#[inline(always)]
pub fn record(seg: Seg, start: u64, end: u64) {
    let raw = end.saturating_sub(start);
    let net = raw.saturating_sub(OVERHEAD.with(|o| o.get()));
    let i = seg as usize;
    CYCLES.with(|c| c[i].set(c[i].get() + net));
    HITS.with(|h| h[i].set(h[i].get() + 1));
}

fn calibrate() {
    let mut best = u64::MAX;
    for _ in 0..1000 {
        let a = read();
        let b = read();
        best = best.min(b.saturating_sub(a));
    }
    OVERHEAD.with(|o| o.set(best));
}

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
    eprintln!(
        "  overhead de medición descontado: {} ciclos/tramo",
        OVERHEAD.with(|o| o.get())
    );
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

    let ht = HITS.with(|h| h[Seg::HelperTotal as usize].get());
    if ht > 0 {
        let total = CYCLES.with(|c| c[Seg::HelperTotal as usize].get()) as f64 / ht as f64;
        let mut parts = 0.0;
        for s in [
            Seg::ShapeLookup,
            Seg::ClosureScan,
            Seg::ObjDataAlloc,
            Seg::HeapPush,
        ] {
            let n = HITS.with(|h| h[s as usize].get());
            if n > 0 {
                parts += CYCLES.with(|c| c[s as usize].get()) as f64 / n as f64;
            }
        }
        eprintln!(
            "  {:<18} {:>8.1} ciclos   (total menos tramos: cruce y no instrumentado)",
            "  resto",
            total - parts
        );
    }
}

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
