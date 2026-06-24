mod varn_contract;

use proc_macro::TokenStream;

#[proc_macro]
pub fn varn_contract(input: TokenStream) -> TokenStream {
    varn_contract::expand(input)
}
