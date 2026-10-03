/// Discriminante en `state[0]` del objeto de estado de una máquina de
/// estados (ver [`FunctionProto::state_size`]) cuando la máquina terminó: el
/// retorno de `poll` es el resultado final.
///
/// La convención `Poll` completa:
///
/// | `state[0]`        | significado                                     |
/// |--------------------|--------------------------------------------------|
/// | `STATE_DONE`        | `Ready`; el retorno es el resultado             |
/// | `STATE_YIELDED`      | `Yielded`; el retorno es lo emitido             |
/// | `>= FIRST_RESUME`   | `Pending`; el número es el punto de reanudación |
///
/// Vive aquí, en `varn-types`, y no en `varn-compiler` (que la definía
/// originalmente): es un contrato de RUNTIME — el sitio de llamada y el
/// scheduler leen `state[0]` tras invocar la función para decidir si
/// terminó, emitió o quedó suspendida — y ni `varn-vm` ni `varn-runtime`
/// dependen de `varn-compiler`, así que ninguno de los dos podría importarla
/// desde ahí sin duplicarla.
///
/// Cubre sólo uno de los dos productores de `Poll` que define el spec §3.8:
/// el **estado de corrutina**, que es lo que produce el pase
/// `state_machine`. El otro productor es el **futuro hoja del host**
/// (`crate::task::AsyncTask`), que no tiene `state[0]` — no es una función
/// del VM con objeto de estado propio — así que el scheduler necesita un
/// `poll` nativo aparte para ése.
pub const STATE_DONE: u32 = 0;
/// Ver [`STATE_DONE`].
pub const STATE_YIELDED: u32 = 1;
/// Primer discriminante de `state[0]` que denota un punto de reanudación
/// (`Pending`); el número es el punto de reanudación. Ver [`STATE_DONE`].
pub const FIRST_RESUME: u32 = 2;
