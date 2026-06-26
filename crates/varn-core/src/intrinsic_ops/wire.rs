#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntrinsicDomain {
    Math = 0x0,
}

#[inline(always)]
pub const fn encode(domain: IntrinsicDomain, op: u8) -> u8 {
    ((domain as u8) << 4) | (op & 0x0F)
}

#[inline(always)]
pub fn decode(byte: u8) -> (u8, u8) {
    (byte >> 4, byte & 0x0F)
}
