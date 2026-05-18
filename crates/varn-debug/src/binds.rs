use crate::colors::{C_BINDS, DIM, RESET};

pub fn debug_binds(filename: &str) {
    use crate::colors::{footer, header};
    header(C_BINDS, "identifier binding", filename);
    eprintln!("  {DIM}(Binding trace not implemented){RESET}");
    footer(C_BINDS, "bind trace complete");
}
