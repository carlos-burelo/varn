#![allow(clippy::disallowed_methods)]
mod contract_members;
mod contract_parse;
mod contract_records;
mod jit_helper_table;
mod jit_slow;
mod vn_contract;
mod vn_enum;
mod vn_record;

use proc_macro::TokenStream;

#[proc_macro]
pub fn vn_contract(input: TokenStream) -> TokenStream {
    vn_contract::expand(input)
}

#[proc_macro_attribute]
pub fn jit_slow(attr: TokenStream, input: TokenStream) -> TokenStream {
    jit_slow::expand(attr, input)
}

#[proc_macro]
pub fn jit_helper_table(input: TokenStream) -> TokenStream {
    jit_helper_table::expand(input)
}

#[proc_macro]
pub fn vn_record(input: TokenStream) -> TokenStream {
    vn_record::expand(input)
}

#[proc_macro]
pub fn vn_enum(input: TokenStream) -> TokenStream {
    vn_enum::expand(input)
}
