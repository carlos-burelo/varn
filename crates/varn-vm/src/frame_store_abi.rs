//! Vista ABI v2 del almacén por clases (`docs/JIT_ABI_V2_SPEC.md` §2, §5).
//!
//! Dominio propio (AGENTS.md §6): `frame_store.rs` supera el techo y no se le
//! añade nada; la vista vive aquí. Invariante: GPR/FPR nunca raíces por
//! construcción; REF/DYN vivos = rangos contiguos `[base, tope)`.
//!
//! Foto transitoria: los `Vec` aún pueden realojar, así que el JIT no conserva
//! estos punteros entre llamadas (eso llega con `StackArenas` en
//! `varn-jit/src/mem.rs`, bases estables). La VM la usa en safepoints y para
//! verificar el contrato (`AbiStacks`).

use crate::frame_store::FrameStore;
use crate::value::VmValue;

const _: () = {
    assert!(
        core::mem::size_of::<VmValue>() == core::mem::size_of::<varn_abi::AbiValue>(),
        "VmValue/AbiValue: mismo layout 16 B"
    );
    assert!(
        core::mem::align_of::<VmValue>() == core::mem::align_of::<varn_abi::AbiValue>(),
        "VmValue/AbiValue: mismo align"
    );
};

/// Rangos vivos contiguos por clase como [`varn_abi::AbiStacks`].
pub fn abi_stacks(store: &FrameStore) -> varn_abi::AbiStacks {
    let gpr = store.gpr.as_ptr() as *mut i64;
    let fpr = store.fpr.as_ptr() as *mut f64;
    let refs = store.refs.as_ptr() as *mut u32;
    let dyn_ = store.dyn_.as_ptr() as *mut varn_abi::AbiValue;
    // SAFETY: solo lectura de punteros base + longitudes vivas; el llamador
    // no los retiene a través de un push que realoje.
    unsafe {
        varn_abi::AbiStacks {
            gpr,
            gpr_end: gpr.add(store.gpr.len()),
            fpr,
            fpr_end: fpr.add(store.fpr.len()),
            refs,
            refs_end: refs.add(store.refs.len()),
            dyn_,
            dyn_end: dyn_.add(store.dyn_.len()),
        }
    }
}

/// Verificación del contrato para safepoints (debug): cada `end >= base`.
/// Pura lectura, sin efectos; el llamador decide si compilarla.
pub fn debug_check_stacks(store: &FrameStore) -> bool {
    let v = abi_stacks(store);
    // `offset_from` exige non-null/dangling-safe: los `Vec` pueden estar
    // vacíos (puntero dangling), así que se compara como direcciones.
    let ok = |base: *mut u8, end: *mut u8| (end as usize) >= (base as usize);
    ok(v.gpr as *mut u8, v.gpr_end as *mut u8)
        && ok(v.fpr as *mut u8, v.fpr_end as *mut u8)
        && ok(v.refs as *mut u8, v.refs_end as *mut u8)
        && ok(v.dyn_ as *mut u8, v.dyn_end as *mut u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_types::register_meta::SlotKind;

    #[test]
    fn rangos_cubren_tramos_vivos() {
        let proto = std::rc::Rc::new(varn_types::FunctionProto {
            register_count: 2,
            register_meta: vec![
                varn_types::register_meta::RegisterMeta {
                    kind: SlotKind::Int,
                },
                varn_types::register_meta::RegisterMeta {
                    kind: SlotKind::Dynamic,
                },
            ],
            ..Default::default()
        });
        let mut s = FrameStore::new();
        s.push_frame(&proto);
        let v = abi_stacks(&s);
        unsafe {
            assert!(v.gpr_end.offset_from(v.gpr) >= 1);
            assert!(v.dyn_end.offset_from(v.dyn_) >= 1);
        }
    }
}
