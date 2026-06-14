mod varn_contract;

use proc_macro::TokenStream;

/// Contract-driven native bindings: reads a `.vn` contract and generates a
/// Rust trait + dispatch wrappers + linker registration. See `varn_contract.rs`.
#[proc_macro]
pub fn varn_contract(input: TokenStream) -> TokenStream {
    varn_contract::expand(input)
}
