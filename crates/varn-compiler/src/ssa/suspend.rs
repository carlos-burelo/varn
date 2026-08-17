//! Localiza los puntos de suspensión de una función y qué los cruza.
//!
//! Sólo lectura: no transforma el IR. El pase de máquinas de estados consume
//! este análisis en vez de recalcularlo, para que "qué es un punto de
//! suspensión" y "qué debe guardar el estado" tengan una sola definición.

use super::ir::{InstKind, SsaFunc, Terminator, Value};
use super::liveness::Liveness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendKind {
    Await,
    Yield,
}

#[derive(Debug, Clone)]
pub struct SuspendPoint {
    pub block: usize,
    pub inst: usize,
    pub kind: SuspendKind,
    pub operand: Value,
    pub dest: Option<Value>,
    /// Valores vivos cruzando el punto: los campos que necesitará el objeto de
    /// estado. Ordenado por índice de valor.
    ///
    /// Puede incluir el propio `dest` de este punto cuando su resultado se
    /// usa más adelante en la función: `live_after` responde "qué está vivo
    /// justo después de ejecutar la instrucción", y `dest` queda definido
    /// por ella, así que si algo posterior lo usa, aparece aquí también. No
    /// contradice el contrato de `live_after` — pero quien dimensione el
    /// objeto de estado a partir de este campo debe tratarlo junto con
    /// `dest`, no sumarlo aparte, o lo contará dos veces.
    pub live: Vec<Value>,
    pub in_try: bool,
    pub in_loop: bool,
}

pub fn analyze(ssa: &SsaFunc) -> Vec<SuspendPoint> {
    let lv = Liveness::analyze(ssa);
    let in_loop = compute_in_loop(ssa);
    let try_depth_in = compute_try_depth_in(ssa);
    let mut out = Vec::new();

    for (b, block) in ssa.blocks.iter().enumerate() {
        // Profundidad de anidamiento `try` a la entrada del bloque, que el
        // dataflow de `compute_try_depth_in` ya resolvió cruzando bloques.
        // Aquí sólo queda aplicar el neto local +1/-1 instrucción a
        // instrucción para saber la profundidad EN cada punto concreto.
        let mut depth = try_depth_in[b];
        for (i, inst) in block.insts.iter().enumerate() {
            let suspend = match &inst.kind {
                InstKind::Await { operand } => Some((SuspendKind::Await, *operand)),
                InstKind::Yield { operand } => Some((SuspendKind::Yield, *operand)),
                _ => None,
            };
            if let Some((kind, operand)) = suspend {
                out.push(SuspendPoint {
                    block: b,
                    inst: i,
                    kind,
                    operand,
                    dest: inst.dest,
                    live: lv.live_after(ssa, b, i),
                    in_try: depth > 0,
                    in_loop: in_loop[b],
                });
            }
            match &inst.kind {
                InstKind::Try { .. } => depth += 1,
                InstKind::PopTry => depth -= 1,
                _ => {}
            }
        }
    }
    out
}

/// Profundidad de anidamiento `try` a la ENTRADA de cada bloque, calculada
/// hacia adelante sobre el CFG desde `ssa.entry`.
///
/// Un contador local por bloque no basta: el cuerpo de un `try` puede
/// abarcar muchos bloques (`if`, bucle o cortocircuito dentro de él, ver
/// `ssa/build/stmt/try_catch.rs`), así que `InstKind::Try` y el
/// `InstKind::PopTry` que lo cierra caen a menudo en bloques distintos del
/// punto de suspensión que cubren. La región guardada va del `Try` al
/// `PopTry` atravesando todo lo que haya en medio — de ahí el dataflow
/// hacia adelante en vez de contar sólo lo visto en el bloque actual.
///
/// Dos tipos de arista, con semántica distinta:
/// - Aristas normales (`Jump`/`Branch`): propagan la profundidad de SALIDA
///   del bloque, ya con el neto +1/-1 de sus propios `Try`/`PopTry`
///   aplicado.
/// - La arista de excepción de cada `Try { handler }`: propaga la
///   profundidad ANTERIOR a ese `Try` concreto, no la posterior. Entrar al
///   `catch` significa que ESE `try` ya terminó (saltó por la excepción);
///   los `try` exteriores siguen aplicando, pero éste no debe contarse — si
///   se propagara la profundidad posterior, el cuerpo del `catch`
///   quedaría marcado como dentro de su propio `try`, que es falso.
///
/// Con anidamiento bien formado, todo bloque alcanzable recibe la misma
/// profundidad por cualquier camino que llegue a él. Si dos caminos
/// discrepan es un bug en la bajada de try/catch a SSA, no un caso
/// legítimo — se entra en pánico con el detalle en vez de adivinar cuál de
/// los dos vale, porque este análisis es la única fuente de verdad que
/// usará el pase de máquinas de estados para decidir el estado guardado.
fn compute_try_depth_in(ssa: &SsaFunc) -> Vec<i32> {
    let n = ssa.blocks.len();
    let mut depth_in: Vec<Option<i32>> = vec![None; n];
    let entry = ssa.entry.0 as usize;
    depth_in[entry] = Some(0);
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(entry);

    while let Some(b) = queue.pop_front() {
        let mut depth = depth_in[b].expect("bloque encolado sin profundidad asignada");
        let block = &ssa.blocks[b];
        let mut out_edges: Vec<(usize, i32)> = Vec::new();
        for inst in &block.insts {
            match &inst.kind {
                InstKind::Try { handler } => {
                    // La rama de excepción ve la profundidad ANTERIOR a
                    // este `Try`: ese `try` concreto ya terminó al entrar
                    // al catch.
                    out_edges.push((handler.0 as usize, depth));
                    depth += 1;
                }
                InstKind::PopTry => depth -= 1,
                _ => {}
            }
        }
        for s in normal_succs(&block.term) {
            out_edges.push((s, depth));
        }
        for (target, d) in out_edges {
            match depth_in[target] {
                None => {
                    depth_in[target] = Some(d);
                    queue.push_back(target);
                }
                Some(existing) => assert_eq!(
                    existing, d,
                    "suspend: profundidad de try inconsistente en b{target}: \
                     {existing} por un camino ya visitado, {d} por este — \
                     revisar la bajada de try/catch a SSA"
                ),
            }
        }
    }

    depth_in.into_iter().map(|d| d.unwrap_or(0)).collect()
}

/// Sucesores normales de control de un terminador — Jump/Branch, sin la
/// arista de excepción de cada `Try`. `compute_try_depth_in` trata esa
/// arista aparte porque lleva una profundidad distinta a la de las demás
/// (ver su doc-comment); mezclarla aquí perdería esa distinción.
fn normal_succs(term: &Terminator) -> Vec<usize> {
    match term {
        Terminator::Jump { target, .. } => vec![target.0 as usize],
        Terminator::Branch {
            then_blk, else_blk, ..
        } => vec![then_blk.0 as usize, else_blk.0 as usize],
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}

/// `true` para los bloques alcanzables desde sí mismos, o sea los que están en
/// un ciclo del CFG. Es la definición que le importa al pase: un estado situado
/// en un bloque así es reentrable.
fn compute_in_loop(ssa: &SsaFunc) -> Vec<bool> {
    let n = ssa.blocks.len();
    let mut reach = vec![vec![false; n]; n];
    for (b, block) in ssa.blocks.iter().enumerate() {
        for s in succs(block) {
            reach[b][s] = true;
        }
    }
    // Clausura transitiva (Floyd-Warshall booleano). Los CFG de una función
    // caben de sobra en O(n^3) a estos tamaños.
    for k in 0..n {
        for i in 0..n {
            if reach[i][k] {
                for j in 0..n {
                    if reach[k][j] {
                        reach[i][j] = true;
                    }
                }
            }
        }
    }
    (0..n).map(|b| reach[b][b]).collect()
}

fn succs(block: &super::ir::Block) -> Vec<usize> {
    let mut v = Vec::new();
    for inst in &block.insts {
        if let InstKind::Try { handler } = &inst.kind {
            v.push(handler.0 as usize);
        }
    }
    match &block.term {
        Terminator::Jump { target, .. } => v.push(target.0 as usize),
        Terminator::Branch {
            then_blk, else_blk, ..
        } => {
            v.push(then_blk.0 as usize);
            v.push(else_blk.0 as usize);
        }
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => {}
    }
    v
}
