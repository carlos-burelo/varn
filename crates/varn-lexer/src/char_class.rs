#[inline]
pub(crate) fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

#[inline]
pub(crate) fn is_hex_digit(c: u8) -> bool {
    is_digit(c) || (b'a'..=b'f').contains(&c) || (b'A'..=b'F').contains(&c)
}

#[inline]
pub(crate) fn is_binary_digit(c: u8) -> bool {
    c == b'0' || c == b'1'
}

#[inline]
pub(crate) fn is_octal_digit(c: u8) -> bool {
    (b'0'..=b'7').contains(&c)
}

#[inline]
pub(crate) fn is_identifier_start(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_uppercase() || c == b'_' || c == b'$' || c > 127
}

#[inline]
pub(crate) fn is_identifier_part(c: u8) -> bool {
    is_identifier_start(c) || is_digit(c)
}
