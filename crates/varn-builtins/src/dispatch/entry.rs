use crate::runtime_ops::RuntimeOpFn;

#[derive(Clone, Copy)]
pub struct DispatchEntry {
    pub id: u64,
    pub module_id: &'static str,
    pub name: &'static str,
    pub func: RuntimeOpFn,
    pub capability: Option<&'static str>,
}

pub fn compound_op_id(module_id: &str, symbol: &str) -> u64 {
    let bytes_m = module_id.as_bytes();
    let bytes_s = symbol.as_bytes();
    let sep = b"::";
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes_m.len() {
        h ^= bytes_m[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    let mut j = 0;
    while j < sep.len() {
        h ^= sep[j] as u64;
        h = h.wrapping_mul(0x100000001b3);
        j += 1;
    }
    let mut k = 0;
    while k < bytes_s.len() {
        h ^= bytes_s[k] as u64;
        h = h.wrapping_mul(0x100000001b3);
        k += 1;
    }
    h
}
