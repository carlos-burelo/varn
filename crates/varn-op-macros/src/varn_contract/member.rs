use proc_macro2::TokenStream as TS2;
use quote::{format_ident, quote};
use syn::Ident;

use super::mapping::{
    call_expr, mapped_tag_path, owned_ty, param_ty, receiver_mapped, ret_ty, Mapped,
};
use super::sanitize;
use crate::contract_members::{Kind, Member};

pub(super) struct Cx<'a> {
    pub(super) prefix: &'a str,
    pub(super) trait_ident: &'a Ident,
    pub(super) self_ty: &'a syn::Type,
    pub(super) module: &'a str,
    pub(super) ns: &'a str,
    pub(super) class_name: &'a str,
}

#[derive(Default)]
pub(super) struct Generated {
    pub(super) trait_sigs: Vec<TS2>,
    pub(super) wrappers: Vec<TS2>,
    pub(super) setup_calls: Vec<TS2>,
    pub(super) fn_entries: Vec<TS2>,
    pub(super) method_entries: Vec<TS2>,
    pub(super) generated_entry_idents: Vec<Ident>,
}

pub(super) fn generate(cx: &Cx, m: &Member, out: &mut Generated) {
    let prefix = cx.prefix;
    let trait_ident = cx.trait_ident;
    let self_ty = cx.self_ty;
    let module = cx.module;
    let ns = cx.ns;
    let class_name_str = cx.class_name;
    let sym = &m.symbol;
    if m.kind == Kind::Property {
        let tag = mapped_tag_path(&m.ret);
        out.setup_calls.push(quote! {
            cls.extend_layout(&[(::std::sync::Arc::from(#sym), #tag)]);
        });
        return;
    }
    let rust_sym = sym.trim_end_matches('$');
    
    let method_ident = if syn::parse_str::<Ident>(rust_sym).is_ok() {
        format_ident!("{}", rust_sym)
    } else {
        format_ident!("r#{}", rust_sym)
    };
    let wrap_ident = format_ident!("__varn_wrap_{}_{}", sanitize(&prefix), sanitize(rust_sym));
    let fast_wrap_ident = format_ident!(
        "__varn_fast_wrap_{}_{}",
        sanitize(&prefix),
        sanitize(rust_sym)
    );

    let mut sig_params: Vec<TS2> = Vec::new();
    let mut decode: Vec<TS2> = Vec::new();
    let mut call_args: Vec<TS2> = Vec::new();

    let mut arg_base = 0usize;
    let mut arg_base_is_dynamic = false;

    match m.kind {
        Kind::Method | Kind::Getter => {
            let recv = receiver_mapped(&prefix);
            let pty = param_ty(&recv);
            let oty = owned_ty(&recv);
            sig_params.push(quote!(this: #pty));
            let b = format_ident!("__this");
            decode.push(quote! {
                let #b = <#oty as ::varn_types::marshal::FromVm>::from_vm(
                    ctx,
                    args.first().copied().unwrap_or(::varn_types::VmValue::null()),
                )?;
            });
            call_args.push(call_expr(&b, &recv));
            arg_base = 1;
        }
        Kind::Constructor => {
            sig_params.push(quote!(this: ::varn_types::VmValue));
            let b = format_ident!("__this");
            decode.push(quote! {
                let #b = args.first().copied().unwrap_or(::varn_types::VmValue::null());
            });
            call_args.push(quote!(#b));
            arg_base = 1;
        }
        Kind::StaticMethod | Kind::StaticGetter | Kind::Function | Kind::Property => {
            arg_base_is_dynamic = true;
        }
    }

    if arg_base_is_dynamic {
        let expected_len = m.params.len();
        decode.push(quote! {
            let arg_base = if args.len() > #expected_len {
                if let Some(&first) = args.first() {
                    if ctx.is_static_receiver(first) {
                        1usize
                    } else {
                        0usize
                    }
                } else {
                    0usize
                }
            } else {
                0usize
            };
        });
    }

    for (i, p) in m.params.iter().enumerate() {
        let pname = format_ident!("__p{}", i);
        let arg_idx_expr = if arg_base_is_dynamic {
            quote!(arg_base + #i)
        } else {
            let val = arg_base + i;
            quote!(#val)
        };
        if p.is_rest {
            sig_params.push(quote!(#pname: &[::varn_types::VmValue]));
            decode.push(quote! {
                let #pname: &[::varn_types::VmValue] =
                    if args.len() > #arg_idx_expr { &args[#arg_idx_expr..] } else { &[] };
            });
            call_args.push(quote!(#pname));
        } else {
            let pty = param_ty(&p.mapped);
            let oty = owned_ty(&p.mapped);
            sig_params.push(quote!(#pname: #pty));
            decode.push(quote! {
                let #pname = <#oty as ::varn_types::marshal::FromVm>::from_vm(
                    ctx,
                    args.get(#arg_idx_expr).copied().unwrap_or(::varn_types::VmValue::null()),
                )?;
            });
            call_args.push(call_expr(&pname, &p.mapped));
        }
    }

    let rty = ret_ty(&m.ret);
    let is_void = matches!(m.ret, Mapped::Void);
    let is_fn = m.kind == Kind::Function;

    let trait_ret = if is_fn {
        let inner = if is_void { quote!(()) } else { rty.clone() };
        quote!(::core::result::Result<#inner, String>)
    } else if m.fallible {
        let inner = if is_void { quote!(()) } else { rty.clone() };
        quote!(::core::result::Result<#inner, ::varn_types::NativeError>)
    } else if is_void {
        quote!(())
    } else {
        rty.clone()
    };

    out.trait_sigs.push(quote! {
        #[allow(non_snake_case)]
        fn #method_ident(ctx: &mut dyn ::varn_types::NativeCtx, #(#sig_params),*) -> #trait_ret;
    });

    let call = quote!(<__T>::#method_ident(ctx, #(#call_args),*));
    let ret_encode = match (is_fn || m.fallible, is_void) {
        (true, true) => quote! { #call?; Ok(::varn_types::VmValue::null()) },
        (true, false) => quote! {
            let __ret = #call?;
            Ok(::varn_types::marshal::IntoVm::into_vm(__ret, ctx))
        },
        (false, true) => quote! {
            #call;
            Ok(::varn_types::VmValue::null())
        },
        (false, false) => quote! {
            let __ret = #call;
            Ok(::varn_types::marshal::IntoVm::into_vm(__ret, ctx))
        },
    };

    out.wrappers.push(quote! {
        #[allow(non_snake_case)]
        pub fn #wrap_ident<__T: #trait_ident>(
            ctx: &mut dyn ::varn_types::NativeCtx,
            args: &[::varn_types::VmValue],
        ) -> ::core::result::Result<::varn_types::VmValue, ::varn_types::NativeError> {
            #(#decode)*
            #ret_encode
        }
    });

    let (fast_wrapper, raw_func_val, sig_val) =
        super::fast::fast_path(cx, m, &method_ident, &fast_wrap_ident);

    out.wrappers.push(fast_wrapper);

    if m.kind == Kind::Function {
        let fn_entry_ident = format_ident!(
            "__VARN_OP_{}_{}",
            sanitize(&prefix).to_uppercase(),
            sanitize(sym).to_uppercase()
        );
        out.generated_entry_idents.push(fn_entry_ident.clone());
        out.fn_entries.push(quote! {
                #[used]
                #[cfg_attr(target_os = "windows", link_section = ".varn_ops$B")]
                #[cfg_attr(target_os = "macos", link_section = "__DATA,varn_ops")]
                #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), link_section = "varn_ops")]
                static #fn_entry_ident: ::varn_types::NativeOpEntry = ::varn_types::NativeOpEntry {
                    module_id: #module.as_ptr(),
                    module_id_len: #module.len() as u32,
                    namespace_path: #ns.as_ptr(),
                    namespace_path_len: #ns.len() as u32,
                    symbol_name: #sym.as_ptr(),
                    symbol_name_len: #sym.len() as u32,
                    func_ptr: #wrap_ident::<#self_ty> as *const u8,
                    raw_func_ptr: #raw_func_val,
                    signature: #sig_val,
                    capability_mask: 0,
                    entry_kind: 0x01,
                    flags: 0,
                    _reserved: [0; 7],
                };
            });
    } else {
        let native = quote!(ctx.alloc_fn(#wrap_ident::<#self_ty>, #sym));
        let setup = match m.kind {
            Kind::Method => quote!(cls.add_method(#sym, #native);),
            Kind::Getter => quote!(cls.add_getter(#sym, #native);),
            Kind::StaticMethod => quote!(cls.add_static(#sym, #native);),
            Kind::StaticGetter => quote!(cls.add_static_getter(#sym, #native);),
            Kind::Constructor => {
                quote!(cls.add_method("constructor", ctx.alloc_fn(#wrap_ident::<#self_ty>, "constructor"));)
            }
            Kind::Function | Kind::Property => unreachable!(),
        };
        out.setup_calls.push(setup);

        
        
        let mkind: u8 = match m.kind {
            Kind::Method => 0x03,
            Kind::StaticMethod => 0x04,
            Kind::Getter => 0x05,
            Kind::StaticGetter => 0x14,
            _ => 0x00,
        };
        if mkind != 0x00 {
            let mentry_ident = format_ident!(
                "__VARN_OPM_{}_{}",
                sanitize(&prefix).to_uppercase(),
                sanitize(sym).to_uppercase()
            );
            out.generated_entry_idents.push(mentry_ident.clone());
            out.method_entries.push(quote! {
                    #[used]
                    #[cfg_attr(target_os = "windows", link_section = ".varn_ops$B")]
                    #[cfg_attr(target_os = "macos", link_section = "__DATA,varn_ops")]
                    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), link_section = "varn_ops")]
                    static #mentry_ident: ::varn_types::NativeOpEntry = ::varn_types::NativeOpEntry {
                        module_id: #module.as_ptr(),
                        module_id_len: #module.len() as u32,
                        namespace_path: #class_name_str.as_ptr(),
                        namespace_path_len: #class_name_str.len() as u32,
                        symbol_name: #sym.as_ptr(),
                        symbol_name_len: #sym.len() as u32,
                        func_ptr: #wrap_ident::<#self_ty> as *const u8,
                        raw_func_ptr: #raw_func_val,
                        signature: #sig_val,
                        capability_mask: 0,
                        entry_kind: #mkind,
                        flags: 0,
                        _reserved: [0; 7],
                    };
                });
        }
    }
}
