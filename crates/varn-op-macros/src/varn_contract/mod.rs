mod fast;
mod input;
mod mapping;
mod member;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::path::Path;
use syn::LitStr;

use crate::contract_members::{collect_functions, collect_members, find_class};
use input::ContractInput;

pub(crate) use mapping::{classify, Mapped};

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as ContractInput);

    let (source, abs_path_str) = if input.contract.trim().starts_with("declare")
        || input.contract.trim().starts_with("export")
        || input.contract.contains('\n')
    {
        (input.contract.clone(), "<inline>".to_string())
    } else {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let abs_path = Path::new(&manifest_dir).join(&input.contract);
        let abs_path_str = abs_path.to_string_lossy().replace('\\', "/");
        match std::fs::read_to_string(&abs_path) {
            Ok(s) => (s, abs_path_str),
            Err(e) => {
                return err(format!("cannot read contract `{}`: {e}", abs_path_str));
            }
        }
    };

    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(&source, &input.contract);
    // This proc-macro parses the contract itself, right here, in its own
    // execution (a proc-macro body is ordinary Rust code that happens to run
    // during another crate's build — not const-eval), so the `AtomInterner`
    // `varn_parser::parse` now returns is a normal local value for the rest
    // of `expand`, not a runtime value reaching across a compile-time
    // boundary. No design gap here: propagating it from Part A resolves
    // every `Atom`-to-text site below directly.
    let (program, interner, arena) = match varn_parser::parse(
        tokens,
        lexeme_buf,
        &input.contract,
        varn_core::AtomInterner::new(),
    ) {
        Ok(p) => p,
        Err(_) => return err(format!("failed to parse contract `{}`", abs_path_str)),
    };

    let members = match &input.class {
        Some(class) => match find_class(&program.body, &arena, class, &interner) {
            Some(decl) => collect_members(class, &decl, &arena, &interner),
            None => {
                return err(format!(
                    "class `{class}` not found in contract `{abs_path_str}`"
                ))
            }
        },
        None => {
            let fns = collect_functions(&program.body, &arena, &interner);
            if fns.is_empty() {
                return err(format!(
                    "no `declare function`s found in contract `{abs_path_str}`"
                ));
            }
            fns
        }
    };

    let prefix = input.class.clone().unwrap_or_else(|| input.module.clone());
    let trait_ident = format_ident!("__VarnContract_{}", sanitize(&prefix));

    let self_ty = &input.self_ty;
    let user_fns = &input.fns;
    let module = &input.module;
    let ns = "";

    // Class name for the `namespace_path` of per-method dispatch entries; empty
    // for function-modules (whose members are all `Kind::Function`).
    let class_name_str: String = input.class.clone().unwrap_or_default();

    // Per-method `NativeOpEntry`s so core-type methods/getters are addressable by
    // a stable op-id (`module::class::symbol`) for direct dispatch — in addition
    // to living in the class vtable via `setup_calls`.

    let cx = member::Cx {
        prefix: &prefix,
        trait_ident: &trait_ident,
        self_ty,
        module,
        ns,
        class_name: &class_name_str,
    };
    let mut generated = member::Generated::default();
    for m in &members {
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

    let abs_lit = LitStr::new(&abs_path_str, proc_macro2::Span::call_site());

    let registration = if let Some(class) = &input.class {
        let builder_ident = format_ident!("__varn_build_{}", sanitize(class));
        let entry_ident = format_ident!("__VARN_OP_{}", sanitize(class).to_uppercase());
        generated_entry_idents.push(entry_ident.clone());
        let superclass_setup = if let Some(parent) = &input.extends {
            quote! {
                if let Some(parent) = ctx.get_class(#parent) {
                    *cls.superclass.borrow_mut() = Some(parent.clone());
                    *cls.root_shape.borrow_mut() =
                        parent.root_shape.borrow().with_class(Some(cls.clone()));
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

            #[used]
            #[cfg_attr(target_os = "windows", link_section = ".varn_ops$B")]
            #[cfg_attr(target_os = "macos", link_section = "__DATA,varn_ops")]
            #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), link_section = "varn_ops")]
            static #entry_ident: ::varn_types::NativeOpEntry = ::varn_types::NativeOpEntry {
                module_id: #module.as_ptr(),
                module_id_len: #module.len() as u32,
                namespace_path: #ns.as_ptr(),
                namespace_path_len: #ns.len() as u32,
                symbol_name: #class.as_ptr(),
                symbol_name_len: #class.len() as u32,
                func_ptr: #builder_ident as *const u8,
                raw_func_ptr: ::core::ptr::null(),
                signature: ::varn_types::SignatureDescriptor::empty(),
                capability_mask: 0,
                entry_kind: 0x10,
                flags: 0,
                _reserved: [0; 7],
            };

            #(#method_entries)*
        }
    } else {
        quote! { #(#fn_entries)* }
    };

    let marker_name = if let Some(class) = &input.class {
        format!("__VARN_LINK_MARKER_{}", sanitize(class).to_uppercase())
    } else {
        format!(
            "__VARN_LINK_MARKER_{}",
            sanitize(&input.module).to_uppercase()
        )
    };
    let link_marker_ident = format_ident!("{}", marker_name);

    let out = quote! {

        const _: &[u8] = include_bytes!(#abs_lit);

        #[allow(non_camel_case_types)]
        pub trait #trait_ident {
            #(#trait_sigs)*
        }

        #[allow(non_snake_case)]
        impl #trait_ident for #self_ty {
            #(#user_fns)*
        }

        #(#wrappers)*

        pub static #link_marker_ident: &[&::varn_types::NativeOpEntry] = &[
            #(&#generated_entry_idents),*
        ];

        #registration
    };

    out.into()
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
