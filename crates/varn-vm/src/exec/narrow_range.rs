//! Rango válido de cada tipo numérico angosto (`i8/i16/i32/u8/u16/u32/f32`),
//! y el chequeo que tanto un cast explícito (`x as i32`) como el resultado
//! de una operación aritmética (`i8 + i8`) deben pasar.
//!
//! `i64`/`f64` — el registro donde vive el valor, sin importar el ancho
//! declarado (`BackendTy::Int8` etc. lowerean a `HirType::Int`, el mismo GPR
//! que `int`) — nunca puede desbordar al operar sobre dos valores YA
//! válidos en un rango angosto: el rango de `i64` es astronómicamente mayor
//! que el de cualquiera de estos siete tipos, así que el chequeo se hace
//! DESPUÉS de la operación normal (`AddInt`/`SubInt`/...), nunca antes ni en
//! vez de ella.
//!
//! `u64` no tiene función aquí a propósito: su aritmética necesita
//! `u64::checked_*` sobre los mismos bits reinterpretados sin signo — sumar
//! dos valores `u64` por encima de `i64::MAX` con la aritmética con signo de
//! `Int` da un resultado incorrecto (falso desbordamiento, o ninguno donde sí
//! lo hay). Por eso `BackendTy::UInt64` no existe todavía (`TypeTag::U64`
//! sigue lowereando a `BackendTy::Int` en `emit/ty.rs::lower_tag`) — ver el
//! Anexo K4 en `docs/AUDIT_RESPONSE.md`.
use crate::error::RuntimeError;
use varn_core::TypeTag;

/// `None` si `tag` no es uno de los siete anchos que este módulo cubre — el
/// llamador (`OpCode::CheckNarrowRange`'s handler) nunca debería alcanzar
/// ese caso (el compilador solo emite el chequeo para estos tags), pero
/// devolver `None` en vez de entrar en pánico aquí mismo dentro del VM deja
/// que el llamador decida cómo reportarlo.
fn int_range(tag: TypeTag) -> Option<(i64, i64)> {
    Some(match tag {
        TypeTag::I8 => (i8::MIN as i64, i8::MAX as i64),
        TypeTag::I16 => (i16::MIN as i64, i16::MAX as i64),
        TypeTag::I32 => (i32::MIN as i64, i32::MAX as i64),
        TypeTag::U8 => (0, u8::MAX as i64),
        TypeTag::U16 => (0, u16::MAX as i64),
        TypeTag::U32 => (0, u32::MAX as i64),
        _ => return None,
    })
}

pub(crate) fn checked_narrow_int(tag: TypeTag, v: i64) -> Result<i64, RuntimeError> {
    let Some((min, max)) = int_range(tag) else {
        return Ok(v);
    };
    if v < min || v > max {
        return Err(RuntimeError::new(format!(
            "integer overflow: {v} is outside {} ({min}..={max})",
            tag.name()
        )));
    }
    Ok(v)
}

/// `f32` no tiene un rango "min..=max" simétrico útil para el mensaje de
/// error de la misma forma que los enteros — lo que de verdad importa es si
/// un valor FINITO en `f64` deja de ser finito al angostarse a `f32`
/// (desborda su exponente). `NaN`/`±inf` en `f64` siguen siendo `NaN`/`±inf`
/// en `f32`: no son un desbordamiento, son el mismo valor no numérico.
pub(crate) fn checked_narrow_f32(v: f64) -> Result<f64, RuntimeError> {
    if v.is_finite() && (v as f32).is_infinite() {
        return Err(RuntimeError::new(format!(
            "float overflow: {v} is outside f32 ({}..={})",
            f32::MIN,
            f32::MAX
        )));
    }
    Ok(v)
}
