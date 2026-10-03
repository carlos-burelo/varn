//! Forma **portable** de un `Type`: la misma estructura semántica, sin
//! `CheckerTyId`.
//!
//! Un `Type` es un `CheckerTyId` y solo significa algo dentro de la
//! `CheckerTyTable` que lo internó (ver `interned.rs`). Ningún artefacto que
//! cruce una frontera de módulo o de proceso — el caché en disco, la interfaz
//! de un módulo, el bundle stdlib — puede llevar ese id: el consumidor tiene
//! su propia tabla y el número apunta a otra forma.
//!
//! `PortableType` es ese árbol self-describing: nombres como texto, hijos
//! recursivos, sin índices. `encode` lo produce desde una tabla y `decode` lo
//! re-interna en la tabla del consumidor. Es el análogo, para tipos ya
//! resueltos, de lo que `TypeNode` es para tipos sintácticos.
//!
//! Lo único que no puede codificarse es `TypeKind::Typeof(ExprId)`: un
//! `ExprId` es relativo a un `AstArena` que este formato no transporta. Se
//! degrada a `Dynamic` (honesto-desconocido, igual que el resto del checker
//! para un tipo que no puede determinar) en lugar de inventar un índice.

mod decode;
mod encode;
mod shape;
#[cfg(test)]
mod tests;

pub use decode::decode;
pub use encode::encode;
pub use shape::{PortableFunction, PortableObjectMember, PortableParam, PortableType};
