mod contract_members;
mod jit_slow;
mod varn_contract;

use proc_macro::TokenStream;

#[proc_macro]
pub fn varn_contract(input: TokenStream) -> TokenStream {
    varn_contract::expand(input)
}

/// Marca un slow-path JIT (spec §4). Valida forma, pasa función intacta.
#[proc_macro_attribute]
pub fn jit_slow(attr: TokenStream, input: TokenStream) -> TokenStream {
    jit_slow::expand(attr, input)
}
