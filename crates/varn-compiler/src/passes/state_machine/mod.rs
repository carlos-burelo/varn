//! Transformación de funciones suspendibles en máquinas de estados.
//!
//! Corre FUERA del bucle a punto fijo de `optimize_with`, después de él y
//! antes de la asignación de registros: transforma una función en otra de
//! forma distinta, y volver a pasarle `licm`/`cse`/`cfg` por encima sería
//! reoptimizar una máquina de estados como si fuera código normal.
//!
//! ## La convención `Poll`
//!
//! Una función del VM devuelve un solo `VmValue`, así que `Poll<T>` no puede
//! ser el valor de retorno sin alocar por cada poll. Es una convención sobre
//! el discriminante que el estado ya lleva:
//!
//! | `state[0]`      | significado                        |
//! |-----------------|------------------------------------|
//! | `STATE_DONE`    | `Ready`; el retorno es el resultado |
//! | `STATE_YIELDED` | `Yielded`; el retorno es lo emitido |
//! | `>= FIRST_RESUME` | `Pending`; el número es el punto de reanudación |
//!
//! El llamante lee `state[0]` tras la llamada — algo que ya iba a hacer.

use crate::ssa::ir::SsaFunc;
use crate::ssa::suspend;

/// `state[0]` cuando la máquina terminó.
pub const STATE_DONE: u32 = 0;
/// `state[0]` cuando la máquina emitió un valor y sigue viva.
pub const STATE_YIELDED: u32 = 1;
/// Primer discriminante que denota un punto de reanudación.
pub const FIRST_RESUME: u32 = 2;

/// Transforma `func` si es suspendible. Devuelve el tamaño del objeto de
/// estado en palabras, o `0` si no la transformó.
pub fn run(func: &mut SsaFunc) -> u16 {
    if !func.is_async && !func.is_generator {
        return 0;
    }
    let points = suspend::analyze(func);
    if !points.is_empty() {
        // Los cortes de CFG llegan en el plan siguiente. Hasta entonces, una
        // función que sí suspende se deja intacta y sigue por el camino
        // actual (`run_lazy_task_sync`), que aún está vivo.
        return 0;
    }
    // Caso trivial: la Task 4 lo trata.
    0
}
