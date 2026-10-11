pub const CONFORM: &str = "conform";
pub const VERIFY_CALL: &str = "verify-call";
pub const VERIFY_SPREAD: &str = "verify-spread";
pub const VERIFY_METHOD: &str = "verify-method";
pub const VERIFY_METHOD_SPREAD: &str = "verify-method-spread";
pub const VERIFY_SUPER: &str = "verify-super";
pub const VERIFY_SUPER_SPREAD: &str = "verify-super-spread";
pub const STR_INDEX: &str = "str-index";
pub const INHERITED: &str = "inherited";

pub fn op_id(name: &str) -> u64 {
    crate::op_id::compound_op_id3(crate::op_id::CORE_MODULE, "<runtime>", name)
}
