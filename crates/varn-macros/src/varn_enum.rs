use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::path::Path;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Token, Visibility};

use crate::contract_records::declared_enum_variants;

struct EnumInput {
    module: Expr,
    contract: LitStr,
    vis: Visibility,
    name: Ident,
}

impl Parse for EnumInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut module = None;
        let mut contract = None;
        while !input.peek(Token![pub]) && !input.peek(Token![enum]) {
            let key: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "module" => module = Some(input.parse::<Expr>()?),
                "contract" => contract = Some(input.parse::<LitStr>()?),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown varn_enum! key `{other}`"),
                    ))
                }
            }
            input.parse::<Token![,]>()?;
        }
        let vis: Visibility = input.parse()?;
        input.parse::<Token![enum]>()?;
        let name: Ident = input.parse()?;
        input.parse::<Token![;]>()?;
        let missing = |k: &str| syn::Error::new(name.span(), format!("varn_enum! needs `{k}:`"));
        Ok(EnumInput {
            module: module.ok_or_else(|| missing("module"))?,
            contract: contract.ok_or_else(|| missing("contract"))?,
            vis,
            name,
        })
    }
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as EnumInput);
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let abs_path = Path::new(&manifest_dir).join(input.contract.value());
    let abs_path_str = abs_path.to_string_lossy().replace('\\', "/");
    let fail = |msg: String| -> TokenStream {
        syn::Error::new(input.name.span(), msg).to_compile_error().into()
    };
    let Ok(source) = std::fs::read_to_string(&abs_path) else {
        return fail(format!("cannot read contract `{abs_path_str}`"));
    };
    let enum_name = input.name.to_string();
    let variants = match declared_enum_variants(&source, &abs_path_str, &enum_name) {
        Ok(v) => v,
        Err(msg) => return fail(msg),
    };

    let idents: Vec<Ident> = variants.iter().map(|(v, _)| format_ident!("{}", v)).collect();
    let names: Vec<&str> = variants.iter().map(|(v, _)| v.as_str()).collect();
    let tags: Vec<i64> = variants.iter().map(|(_, t)| *t).collect();
    let count = variants.len();
    let vis = &input.vis;
    let name = &input.name;
    let module = &input.module;
    let builder = format_ident!("__varn_enum_build_{}", enum_name.to_lowercase());
    let entry = format_ident!("__VARN_ENUM_{}", enum_name.to_uppercase());
    let entries = format_ident!("{}_ENTRIES", enum_name.to_uppercase());
    let abs_lit = LitStr::new(&abs_path_str, proc_macro2::Span::call_site());

    quote! {
        const _: &[u8] = include_bytes!(#abs_lit);

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #vis enum #name {
            #(#idents,)*
        }

        impl #name {
            pub const MODULE: &'static str = #module;
            pub const NAME: &'static str = #enum_name;
            pub const ALL: [Self; #count] = [#(Self::#idents,)*];

            pub const fn name(self) -> &'static str {
                match self {
                    #(Self::#idents => #names,)*
                }
            }

            pub const fn tag(self) -> i64 {
                match self {
                    #(Self::#idents => #tags,)*
                }
            }

            pub fn of(name: &str) -> ::core::option::Option<Self> {
                Self::ALL.into_iter().find(|v| v.name() == name)
            }

            pub fn of_tag(tag: i64) -> ::core::option::Option<Self> {
                Self::ALL.into_iter().find(|v| v.tag() == tag)
            }

            pub fn to_value(
                self,
                ctx: &mut dyn ::varn_types::NativeCtx,
            ) -> ::core::result::Result<::varn_types::VmValue, ::std::string::String> {
                ctx.enum_variant(Self::MODULE, Self::NAME, self.name(), &[])
            }

            pub fn from_value(
                ctx: &dyn ::varn_types::NativeCtx,
                value: ::varn_types::VmValue,
            ) -> ::core::option::Option<Self> {
                ctx.enum_variant_name(value).and_then(|n| Self::of(&n))
            }
        }

        fn #builder(
            ctx: &mut dyn ::varn_types::NativeCtx,
            _args: &[::varn_types::VmValue],
        ) -> ::core::result::Result<::varn_types::VmValue, ::varn_types::NativeError> {
            let cls = ::varn_types::value::ClassObj::new_native_rc(#name::NAME);
            for variant in #name::ALL {
                let def = ::std::sync::Arc::new(::varn_types::value::VariantDef {
                    enum_name: ::std::sync::Arc::from(#name::NAME),
                    variant_name: ::std::sync::Arc::from(variant.name()),
                    tag: variant.tag(),
                    fields: ::std::vec::Vec::new(),
                });
                let mut data = ::varn_types::value::EnumVariantData::unit(def);
                data.enum_class_id = Some(cls.id);
                let value = ctx.alloc_enum_variant(data);
                cls.add_static(variant.name(), value);
            }
            ctx.register_class(#name::NAME, cls.clone());
            Ok(ctx.alloc_class(cls))
        }

        static #entry: ::varn_types::NativeOpEntry = ::varn_types::NativeOpEntry {
            module_id: #name::MODULE,
            namespace_path: "",
            symbol_name: #name::NAME,
            func_ptr: #builder as *const u8,
            raw_func_ptr: ::core::ptr::null(),
            signature: ::varn_types::SignatureDescriptor::empty(),
            entry_kind: ::varn_types::EntryKind::ClassDef,
            flags: 0,
            capability: "",
        };

        pub static #entries: &[&::varn_types::NativeOpEntry] = &[&#entry];
    }
    .into()
}
