use proc_macro::TokenStream;
use quote::quote;
use std::path::Path;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token, Visibility};

use crate::contract_records::{record_fields, RecordSource};
use crate::varn_contract::Mapped;

struct RecordInput {
    contract: LitStr,
    source: RecordSource,
    declared: LitStr,
    vis: Visibility,
    name: Ident,
}

impl Parse for RecordInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut contract = None;
        let mut declared = None;
        while !input.peek(Token![pub]) && !input.peek(Token![struct]) {
            let key: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            let val: LitStr = input.parse()?;
            match key.to_string().as_str() {
                "contract" => contract = Some(val),
                "interface" => declared = Some((RecordSource::Interface, val)),
                "class" => declared = Some((RecordSource::Class, val)),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown varn_record! key `{other}`"),
                    ))
                }
            }
            input.parse::<Token![,]>()?;
        }
        let vis: Visibility = input.parse()?;
        input.parse::<Token![struct]>()?;
        let name: Ident = input.parse()?;
        input.parse::<Token![;]>()?;
        let contract = contract
            .ok_or_else(|| syn::Error::new(name.span(), "varn_record! needs `contract:`"))?;
        let (source, declared) = declared.ok_or_else(|| {
            syn::Error::new(name.span(), "varn_record! needs `interface:` or `class:`")
        })?;
        Ok(RecordInput {
            contract,
            source,
            declared,
            vis,
            name,
        })
    }
}

fn snake(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 4);
    for c in camel.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn rust_ident(name: &str) -> Ident {
    syn::parse_str::<Ident>(name)
        .unwrap_or_else(|_| Ident::new_raw(name, proc_macro2::Span::call_site()))
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as RecordInput);
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let abs_path = Path::new(&manifest_dir).join(input.contract.value());
    let abs_path_str = abs_path.to_string_lossy().replace('\\', "/");
    let fail = |msg: String| -> TokenStream {
        syn::Error::new(input.name.span(), msg)
            .to_compile_error()
            .into()
    };
    let Ok(source) = std::fs::read_to_string(&abs_path) else {
        return fail(format!("cannot read contract `{abs_path_str}`"));
    };
    let declared = input.declared.value();
    let fields = match record_fields(&source, &abs_path_str, &input.source, &declared) {
        Ok(fields) => fields,
        Err(msg) => return fail(msg),
    };

    let mut decls = Vec::new();
    let mut sets = Vec::new();
    let mut keys = Vec::new();
    for (vn_name, mapped) in &fields {
        let rust_name = snake(vn_name);
        let ident = rust_ident(&rust_name);
        let key = rust_ident(&rust_name.to_ascii_uppercase());
        let (ty, value) = match mapped {
            Mapped::Int => (
                quote!(i64),
                quote!(::varn_types::VmValue::from_int(self.#ident)),
            ),
            Mapped::Float => (
                quote!(f64),
                quote!(::varn_types::VmValue::from_f64(self.#ident)),
            ),
            Mapped::Bool => (
                quote!(bool),
                quote!(::varn_types::VmValue::from_bool(self.#ident)),
            ),
            Mapped::Char
            | Mapped::Str
            | Mapped::StrRecv
            | Mapped::Array
            | Mapped::Dynamic
            | Mapped::Void
            | Mapped::Opt(_) => (quote!(::varn_types::VmValue), quote!(self.#ident)),
        };
        decls.push(quote!(pub #ident: #ty));
        sets.push(quote!(ctx.set_field(obj, Self::#key, #value);));
        keys.push(quote!(pub const #key: &'static str = #vn_name;));
    }

    let vis = &input.vis;
    let name = &input.name;
    let abs_lit = LitStr::new(&abs_path_str, proc_macro2::Span::call_site());
    let alloc = match input.source {
        RecordSource::Interface => quote! {
            pub fn alloc(self, ctx: &mut dyn ::varn_types::NativeCtx) -> ::varn_types::VmValue {
                let obj = ctx.alloc_object();
                #(#sets)*
                obj
            }
        },
        RecordSource::Class => quote! {
            pub const CLASS: &'static str = #declared;

            pub fn alloc(
                self,
                ctx: &mut dyn ::varn_types::NativeCtx,
            ) -> ::core::result::Result<::varn_types::VmValue, ::std::string::String> {
                let obj = ctx
                    .alloc_instance(Self::CLASS)
                    .ok_or_else(|| ::std::format!("{} class not registered", Self::CLASS))?;
                #(#sets)*
                Ok(obj)
            }
        },
    };
    quote! {
        const _: &[u8] = include_bytes!(#abs_lit);

        #vis struct #name {
            #(#decls,)*
        }

        impl #name {
            #(#keys)*

            #alloc
        }
    }
    .into()
}
