mod jit_helper_table;
mod jit_slow;

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn jit_slow(attr: TokenStream, input: TokenStream) -> TokenStream {
    jit_slow::expand(attr, input)
}

#[proc_macro]
pub fn jit_helper_table(input: TokenStream) -> TokenStream {
    jit_helper_table::expand(input)
}
