//! Liveness sobre valores SSA: único dueño del dataflow.
//!
//! Dos consumidores: la asignación de registros (`emit::regs`) y el pase de
//! máquinas de estados. Tenerlo dos veces es como se separan.
//!
//! ## Numeración de puntos
//!
//! Los índices de `def`/`end` numeran puntos en el orden del vector
//! `ssa.blocks` — NO en orden de emisión: `emit/mod.rs` recorre los bloques
//! con `emission_order`, un RPO consciente de bucles que puede reordenar
//! bloques respecto a `ssa.blocks`. Un consumidor que asuma orden de emisión
//! leería mal los índices.
//!
//! Dentro de un bloque cuyos params ocupan el punto `P`: la instrucción `i`
//! ocupa el punto `P + 1 + i`, y el terminador ocupa el punto `P + 1 + n`
//! (`n` = número de instrucciones del bloque).
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
    /// Bloque -> conjunto de valores vivos a la salida del bloque.
    pub live_out: Vec<FxHashSet<u32>>,
}

impl Liveness {
    pub fn analyze(ssa: &SsaFunc) -> Liveness {
        let order: Vec<usize> = (0..ssa.blocks.len()).collect();
        Self::analyze_ordered(ssa, &order)
    }

    pub fn analyze_ordered(ssa: &SsaFunc, order: &[usize]) -> Liveness {
        let nvals = ssa.values.len();
        let nblocks = ssa.blocks.len();
        let mut def = vec![u32::MAX; nvals];
        let mut last = vec![0u32; nvals];
        let mut term_idx = vec![0u32; nblocks];
        let mut defs: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut uses: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); nblocks];
        let mut idx = 0u32;
        for &b in order {
            let block = &ssa.blocks[b];
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
        for &b in order {
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
            live_out,
        }
    }

    /// Valores vivos DESPUÉS de ejecutar la instrucción `i` del bloque `b`.
    ///
    /// Es el conjunto que debe sobrevivir a una suspensión situada en `i`: lo
    /// que el objeto de estado de una máquina de estados tiene que guardar.
    ///
    /// A diferencia de la numeración lineal de puntos, esto **no depende del
    /// orden del vector `ssa.blocks`**: parte de `live_out[b]`, le une lo que
    /// usa el terminador del bloque (se ejecuta después de toda instrucción,
    /// así que sus usos ya están vivos en este punto), y desde ahí camina
    /// hacia atrás por las instrucciones aplicando la transferencia
    /// `live = (live - def(inst)) ∪ uses(inst)`. Sólo usa información local al
    /// bloque más su `live_out`, ambos independientes del orden del vector.
    ///
    /// El resultado viene ordenado por índice de valor, para que dos
    /// invocaciones sobre el mismo IR den la misma respuesta en el mismo orden
    /// — un consumidor que derive de aquí el layout de un objeto necesita esa
    /// estabilidad.
    pub fn live_after(&self, ssa: &SsaFunc, b: usize, i: usize) -> Vec<Value> {
        let block = &ssa.blocks[b];
        debug_assert!(
            i < block.insts.len(),
            "live_after: i {i} fuera de rango para b{b} ({} insts)",
            block.insts.len()
        );
        let mut live: FxHashSet<u32> = self.live_out[b].clone();

        // El terminador se ejecuta después de todas las instrucciones del
        // bloque, así que arranca el recorrido hacia atrás: lo que usa
        // (el `cond` de un Branch, el valor de Return/Throw, los args de
        // Jump/Branch hacia sus sucesores) tiene que estar vivo en el punto
        // que sigue a la última instrucción. El terminador no define nada,
        // así que aquí sólo hay unión, nunca resta.
        for u in crate::ssa::verify::term_value_uses(&block.term) {
            live.insert(u.0);
        }
        for (_, args) in crate::ssa::verify::out_edges(&block.term) {
            for a in args {
                live.insert(a.0);
            }
        }

        // Recorre hacia atrás hasta pasar la instrucción i+1: el estado que
        // queda es "vivo justo después de i".
        for inst in block.insts[i + 1..].iter().rev() {
            if let Some(d) = inst.dest {
                live.remove(&d.0);
            }
            for u in crate::ssa::verify::inst_uses(&inst.kind) {
                live.insert(u.0);
            }
        }

        let mut out: Vec<Value> = live.into_iter().map(Value).collect();
        out.sort_unstable_by_key(|v| v.0);
        out
    }
}
