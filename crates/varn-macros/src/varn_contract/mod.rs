mod fast;
mod input;
mod mapping;
mod member;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::path::Path;
use syn::LitStr;

use crate::contract_parse::LookupFailure;
use input::ContractInput;

pub(crate) use mapping::Mapped;

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as ContractInput);

    if input.contract.trim().starts_with("declare")
        || input.contract.trim().starts_with("export")
        || input.contract.contains('\n')
    {
        return err(
            "inline contracts are not supported: move the contract to a .vn file".to_string(),
        );
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let abs_path = Path::new(&manifest_dir).join(&input.contract);
    let abs_path_str = abs_path.to_string_lossy().replace('\\', "/");
    let Ok(source) = std::fs::read_to_string(&abs_path) else {
        return err(format!("cannot read contract `{abs_path_str}`"));
    };

    let mut sections = Vec::new();
    let mut entries = Vec::new();
    for section in &input.sections {
        match expand_section(&input, section, &source, &abs_path_str) {
            Ok((tokens, idents)) => {
                sections.push(tokens);
                entries.extend(idents);
            }
            Err(msg) => return err(msg),
        }
    }

    let abs_lit = LitStr::new(&abs_path_str, proc_macro2::Span::call_site());
    let module_id = LitStr::new(&input.module, proc_macro2::Span::call_site());
    quote! {
        const _: &[u8] = include_bytes!(#abs_lit);

        pub const MODULE_ID: &str = #module_id;

        #(#sections)*

        pub static NATIVE_ENTRIES: &[&::varn_types::NativeOpEntry] = &[
            #(&#entries),*
        ];
    }
    .into()
}

fn expand_section(
    input: &ContractInput,
    section: &input::Section,
    source: &str,
    abs_path_str: &str,
) -> Result<(proc_macro2::TokenStream, Vec<syn::Ident>), String> {
    let contract =
        match crate::contract_parse::lookup(source, &input.contract, section.class.as_deref()) {
            Ok(contract) => contract,
            Err(LookupFailure::Unparsable(reason)) | Err(LookupFailure::BadCapability(reason)) => {
                return Err(reason)
            }
            Err(LookupFailure::ClassNotFound) => {
                let class = section.class.clone().unwrap_or_default();
                return Err(format!(
                    "class `{class}` not found in contract `{abs_path_str}`"
                ));
            }
            Err(LookupFailure::NoFunctions) => {
                return Err(format!(
                    "no `declare function`s found in contract `{abs_path_str}`"
                ))
            }
        };

    let prefix = section
        .class
        .clone()
        .unwrap_or_else(|| input.module.clone());
    let trait_ident = format_ident!("__VarnContract_{}", sanitize(&prefix));

    let self_ty = &section.self_ty;
    let user_fns = &section.fns;
    let module = &input.module;
    let ns = "";

    let class_name_str: String = section.class.clone().unwrap_or_default();

    let cx = member::Cx {
        prefix: &prefix,
        trait_ident: &trait_ident,
        self_ty,
        module,
        ns,
        class_name: &class_name_str,
    };
    let mut generated = member::Generated::default();
    for m in &contract.members {
        member::generate(&cx, m, &mut generated);
    }
    let member::Generated {
        trait_sigs,
        wrappers,
        setup_calls,
        fn_entries,
        method_entries,
        mut generated_entry_idents,
    } = generated;

    let registration = if let Some(class) = &section.class {
        let builder_ident = format_ident!("__varn_build_{}", sanitize(class));
        let entry_ident = format_ident!("__VARN_OP_{}", sanitize(class).to_uppercase());
        generated_entry_idents.push(entry_ident.clone());
        let superclass_setup = if let Some(parent) = &contract.extends {
            quote! {
                if let Some(parent) = ctx.get_class(#parent) {
                    *cls.superclass.borrow_mut() = Some(parent.clone());
                    cls.set_layout(parent.layout());
                }
            }
        } else {
            quote! {}
        };
        quote! {
            pub fn #builder_ident(
                ctx: &mut dyn ::varn_types::NativeCtx,
                _args: &[::varn_types::VmValue],
            ) -> ::core::result::Result<::varn_types::VmValue, ::varn_types::NativeError> {
                let cls = ctx
                    .get_class(#class)
                    .unwrap_or_else(|| ::varn_types::value::ClassObj::new_native_rc(#class));
                #superclass_setup
                #(#setup_calls)*
                ctx.register_class(#class, cls.clone());
                Ok(ctx.alloc_class(cls.clone()))
            }

            static #entry_ident: ::varn_types::NativeOpEntry = ::varn_types::NativeOpEntry {
                module_id: #module,
                namespace_path: #ns,
                symbol_name: #class,
                func_ptr: #builder_ident as *const u8,
                raw_func_ptr: ::core::ptr::null(),
                signature: ::varn_types::SignatureDescriptor::empty(),
                entry_kind: ::varn_types::EntryKind::ClassDef,
                flags: 0,
                capability: "",
            };

            #(#method_entries)*
        }
    } else {
        quote! { #(#fn_entries)* }
    };

    let tokens = quote! {
        #[allow(non_camel_case_types)]
        pub trait #trait_ident {
            #(#trait_sigs)*
        }

        #[allow(non_snake_case)]
        impl #trait_ident for #self_ty {
            #(#user_fns)*
        }

        #(#wrappers)*

        #registration
    };
    Ok((tokens, generated_entry_idents))
}

pub(super) fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

fn err(msg: String) -> TokenStream {
    let lit = LitStr::new(&msg, proc_macro2::Span::call_site());
    TokenStream::from(quote! { compile_error!(#lit); })
}
