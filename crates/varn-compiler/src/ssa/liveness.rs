//! Liveness sobre valores SSA: único dueño del dataflow.
//!
//! Dos consumidores: la asignación de registros (`emit::regs`) y el pase de
//! máquinas de estados. Tenerlo dos veces es como se separan.
//!
//! ## Numeración de puntos
//!
//! Los índices de `def`/`end`/`term_idx` numeran puntos en el orden del
//! vector `ssa.blocks` — NO en orden de emisión: `emit/mod.rs` recorre los
//! bloques con `emission_order`, un RPO consciente de bucles que puede
//! reordenar bloques respecto a `ssa.blocks`. Un consumidor que asuma orden
//! de emisión leería mal los índices.
//!
//! Dentro de un bloque `b` cuyos params ocupan el punto `P`: la instrucción
//! `i` ocupa el punto `P + 1 + i`, y el terminador ocupa
//! `term_idx[b] = P + 1 + n` (`n` = número de instrucciones del bloque).
//!
//! ## `end` es un intervalo lineal
//!
//! `end` es un intervalo LINEAL, no liveness exacta por punto: un valor
//! definido *fuera* de un bucle y usado *dentro* de él fuerza que la
//! propagación `live_out -> term_idx` extienda su `end` hasta el terminador
//! del bloque de retorno (el latch), así que el intervalo abarca todo el
//! bucle aunque el valor no se use en absoluto entre la cabecera y ese
//! punto. Es una sobre-aproximación conservadora — nunca declara muerto algo
//! vivo — así que es correcta para asignar registros y correcta para elegir
//! campos de estado, a costa de guardar de más en bucles.
//!
//! Disparador del clamp final de `analyze` (`end[v] = def[v]` cuando
//! `end[v] < def[v]`): dispara exactamente para definiciones sin ningún
//! uso. Si `v` tiene algún uso, entonces `v ∈ live_out[def_block]`, luego
//! `end[v] >= term_idx[def_block] > def[v]` y el clamp no llega a
//! dispararse. Para una def muerta el clamp colapsa el intervalo a un
//! punto, que es el comportamiento correcto.
//!
//! ## Orden de `ssa.blocks` es compatible con dominancia
//!
//! `live_across` filtra con `def[v] < idx && end[v] > idx`. El término
//! `def[v] < idx` sólo es sano si el orden del vector `ssa.blocks` es
//! compatible con dominancia (si `D` domina a `U`, `índice(D) < índice(U)`).
//! Ese invariante se cumple:
//!
//! - `passes/cfg.rs::get_reachable` recorre el grafo desde `func.entry` con
//!   una `VecDeque` (`push_back`/`pop_front`) — BFS — y `compact_cfg`
//!   renumera los bloques en ese orden.
//! - BFS desde `entry` es compatible con dominancia: si `D` domina a `U`,
//!   todo camino de `entry` a `U` pasa por `D`, luego `dist(D) < dist(U)`,
//!   y BFS asigna índices en orden no decreciente de distancia, así que
//!   `índice(D) < índice(U)`.
//! - `passes/mod.rs:54` llama a `cfg::simplify_and_compact`
//!   incondicionalmente dentro del bucle de `optimize_with`, que siempre da
//!   al menos una vuelta.
//!
//! Precondición: `Liveness::analyze` asume una función que pasó por
//! `passes::optimize_with`. Si se llamara sobre SSA que se saltó los pases,
//! este invariante no está garantizado.

use super::ir::{InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

#[derive(Debug)]
pub struct Liveness {
    /// Valor -> punto de definición. `u32::MAX` si el valor nunca se
    /// define; todo consumidor debe comprobar este centinela antes de
    /// usar el índice.
    pub def: Vec<u32>,
    /// Valor -> último punto donde el valor sigue vivo.
    pub end: Vec<u32>,
    /// Bloque -> conjunto de valores vivos a la entrada del bloque.
    pub live_in: Vec<FxHashSet<u32>>,
    /// Bloque -> conjunto de valores vivos a la salida del bloque.
    pub live_out: Vec<FxHashSet<u32>>,
    /// Bloque -> punto de su terminador.
    pub term_idx: Vec<u32>,
    /// Bloque -> punto de sus params (el `P` de la fórmula en el doc del
    /// módulo). Ver `inst_point`.
    pub block_start: Vec<u32>,
}

impl Liveness {
    pub fn analyze(ssa: &SsaFunc) -> Liveness {
        let nvals = ssa.values.len();
        let nblocks = ssa.blocks.len();
        let mut def = vec![u32::MAX; nvals];
        let mut last = vec![0u32; nvals];
        let mut term_idx = vec![0u32; nblocks];
        let mut block_start = vec![0u32; nblocks];
        let mut defs: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut uses: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); nblocks];
        let mut idx = 0u32;
        for (b, block) in ssa.blocks.iter().enumerate() {
            block_start[b] = idx;
            let mut local_defined: FxHashSet<u32> = FxHashSet::default();
            for p in &block.params {
                if def[p.0 as usize] == u32::MAX {
                    def[p.0 as usize] = idx;
                }
                defs[b].insert(p.0);
                local_defined.insert(p.0);
            }
            idx += 1;
            for inst in &block.insts {
                for u in crate::ssa::verify::inst_uses(&inst.kind) {
                    if last[u.0 as usize] < idx {
                        last[u.0 as usize] = idx;
                    }
                    if !local_defined.contains(&u.0) {
                        uses[b].insert(u.0);
                    }
                }
                if let Some(d) = inst.dest {
                    if def[d.0 as usize] == u32::MAX {
                        def[d.0 as usize] = idx;
                    }
                    defs[b].insert(d.0);
                    local_defined.insert(d.0);
                }
                if let InstKind::Try { handler } = &inst.kind {
                    succ[b].push(handler.0 as usize);
                }
                idx += 1;
            }
            let mut touch = |v: Value, uses: &mut FxHashSet<u32>| {
                if last[v.0 as usize] < idx {
                    last[v.0 as usize] = idx;
                }
                if !local_defined.contains(&v.0) {
                    uses.insert(v.0);
                }
            };
            match &block.term {
                Terminator::Return(Some(v)) | Terminator::Throw(v) => touch(*v, &mut uses[b]),
                Terminator::Branch {
                    cond,
                    then_blk,
                    then_args,
                    else_blk,
                    else_args,
                } => {
                    touch(*cond, &mut uses[b]);
                    then_args
                        .iter()
                        .chain(else_args)
                        .for_each(|a| touch(*a, &mut uses[b]));
                    succ[b].push(then_blk.0 as usize);
                    succ[b].push(else_blk.0 as usize);
                }
                Terminator::Jump { target, args } => {
                    args.iter().for_each(|a| touch(*a, &mut uses[b]));
                    succ[b].push(target.0 as usize);
                }
                Terminator::Return(None) | Terminator::Unreachable => {}
            }
            term_idx[b] = idx;
            idx += 1;
        }

        let mut live_in: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut live_out: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut changed = true;
        while changed {
            changed = false;
            for b in (0..nblocks).rev() {
                let mut out = FxHashSet::default();
                for &s in &succ[b] {
                    out.extend(live_in[s].iter().copied());
                }
                let mut nin = uses[b].clone();
                nin.extend(out.iter().copied().filter(|v| !defs[b].contains(v)));
                if out != live_out[b] || nin != live_in[b] {
                    live_out[b] = out;
                    live_in[b] = nin;
                    changed = true;
                }
            }
        }

        let mut end = last;
        for b in 0..nblocks {
            for &v in &live_out[b] {
                if end[v as usize] < term_idx[b] {
                    end[v as usize] = term_idx[b];
                }
            }
        }
        for v in 0..nvals {
            if def[v] != u32::MAX && end[v] < def[v] {
                end[v] = def[v];
            }
        }

        Liveness {
            def,
            end,
            live_in,
            live_out,
            term_idx,
            block_start,
        }
    }

    /// Punto de la instrucción `i` del bloque `b`.
    pub fn inst_point(&self, b: usize, i: usize) -> u32 {
        self.block_start[b] + 1 + i as u32
    }

    /// Valores vivos cruzando el punto `idx`: definidos antes y todavía vivos
    /// después. Es la consulta que necesita el pase de máquinas de estados
    /// para decidir qué guarda el objeto de estado.
    ///
    /// Precondición: `idx` debe ser el punto de una INSTRUCCIÓN, nunca el de
    /// un terminador (ningún `term_idx[b]`). En un terminador el predicado
    /// `end[v] > idx` subaproxima: excluye los valores para los que
    /// `end[v] == term_idx[b]` exactamente, que es justo el caso de un valor
    /// vivo únicamente porque cruza por la arista de retorno de un bucle
    /// (`v` en `live_out[b]` con `end[v] == term_idx[b]`) — vivo de verdad
    /// cruzando ese punto, pero el predicado lo declararía muerto. En
    /// puntos de instrucción el predicado es correcto en los cuatro bordes;
    /// los consumidores reales (`InstKind::Await`/`InstKind::Yield`) sólo
    /// consultan ahí. Omitir un valor vivo del objeto de estado del pase de
    /// máquinas de estados sería un miscompile silencioso, así que este
    /// método rechaza la precondición (con `assert!`, también en release)
    /// en vez de devolver un resultado sub-aproximado en silencio.
    pub fn live_across(&self, idx: u32) -> Vec<u32> {
        assert!(
            !self.term_idx.contains(&idx),
            "live_across: idx {idx} es punto de un terminador; el predicado \
             end[v] > idx subaproxima ahi (ver doc del metodo)"
        );
        (0..self.def.len() as u32)
            .filter(|&v| {
                let d = self.def[v as usize];
                d != u32::MAX && d < idx && self.end[v as usize] > idx
            })
            .collect()
    }
}
