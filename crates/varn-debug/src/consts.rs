use crate::colors::{C_CONSTS, DIM, RESET};

pub fn debug_consts(filename: &str) {
    use crate::colors::{footer, header};
    header(C_CONSTS, "constant folding", filename);
    eprintln!("  {DIM}(Constant evaluation trace not implemented){RESET}");
    footer(C_CONSTS, "const trace complete");
}
