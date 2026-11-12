use cranelift_codegen::isa::OwnedTargetIsa;

pub(super) fn has_round_support(isa: &OwnedTargetIsa) -> bool {
    isa.isa_flags()
        .iter()
        .find(|f| f.name == "has_sse41")
        .and_then(|f| f.as_bool())
        .unwrap_or(false)
}
