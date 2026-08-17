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
//!
//! Esta convención cubre sólo uno de los dos productores de `Poll` que
//! define el spec §3.8: el **estado de corrutina** — las funciones que este
//! pase transforma. El otro productor es el **futuro hoja del host**
//! (`varn_types::AsyncTask`), y ése NO tiene `state[0]`: no es una función
//! del VM con objeto de estado propio, así que el scheduler necesita un
//! `poll` nativo aparte para él (el spec ya lo anticipa). `state[0]` no es
//! "la" representación de `Poll`, es la de uno de los dos productores.
//!
//! Las constantes (`STATE_DONE`, `STATE_YIELDED`, `FIRST_RESUME`) viven en
//! `varn_types` (`chunk::proto`), no aquí: son un contrato de RUNTIME que el
//! sitio de llamada y el scheduler leen, y ninguno de los dos depende de
//! `varn-compiler` para poder importarlas desde este módulo.

use crate::ssa::ir::SsaFunc;
use crate::ssa::suspend;

/// Transforma `func` si es suspendible. Devuelve el tamaño del objeto de
/// estado en palabras, o `0` si no la transformó.
pub fn run(func: &mut SsaFunc) -> u16 {
    // `func.is_async` es SIEMPRE false para el top-level de módulo (ver
    // SsaFunc::is_async): este gate nunca alcanza `suspend::analyze` para
    // `<module>` aunque haya `await` de nivel superior. La señal que sí
    // sirve para el top-level ya existe y ya está nombrada en el doc de
    // `SsaFunc::is_async` (ssa/ir.rs): `suspend::analyze(func)` no vacío. El
    // plan siguiente debe usar esa señal ahí, no ampliar esta condición sin
    // más.
    //
    // `is_generator` queda excluido aquí a propósito, junto con `is_async`:
    // `function*`/`async function*` (generador, con o sin `async` — las dos
    // formas ponen `is_generator = true`) caen en el mismo
    // `points.is_empty()` de abajo que una `async` sin `await` cuando no
    // tienen `yield` — la forma declarada por sí sola no basta para
    // distinguirlos — pero `Yielded` es semántica de otro plan: darle
    // `state_size = 1` hoy sería una ambigüedad servida en bandeja a quien
    // construya el camino de coste cero sobre este campo.
    if !func.is_async || func.is_generator {
        return 0;
    }
    let points = suspend::analyze(func);
    if !points.is_empty() {
        // Los cortes de CFG llegan en el plan siguiente. Hasta entonces, una
        // función que sí suspende se deja intacta y sigue por el camino
        // actual (`run_lazy_task_sync`), que aún está vivo.
        return 0;
    }
    // Una `async` que no suspende sigue siendo una máquina de estados: de un
    // solo estado. No hay CFG que partir ni valores que guardar, así que el
    // objeto de estado es sólo el discriminante.
    //
    // El caso importa por sí mismo: es donde el camino de coste cero se ve
    // más claro (el estado nunca sale de los registros del marco), y es la
    // forma que ejercita toda la fontanería sin tocar la parte arriesgada.
    1
}
