use proc_macro::TokenStream;
use proc_macro2::TokenStream as TS2;
use quote::{format_ident, quote};
use std::path::Path;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

use varn_core::ast::{ClassDecl, ClassMember, Decl, ExportDecl, Param, Pattern, Stmt, StmtKind};
use varn_core::ast::{FunctionDecl, TypeNode};
use varn_core::kinds::TypeKind;
use varn_core::{IntrinsicType, TypeTag};

pub(crate) struct ContractInput {
    module: String,

    class: Option<String>,

    extends: Option<String>,
    contract: String,
    self_ty: syn::Type,
    fns: Vec<syn::ImplItemFn>,
}

impl Parse for ContractInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut module = None;
        let mut class = None;
        let mut extends = None;
        let mut contract = None;

        while !input.peek(Token![impl]) {
            let key: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            let val: LitStr = input.parse()?;
            match key.to_string().as_str() {
                "module" => module = Some(val.value()),
                "class" => class = Some(val.value()),
                "extends" => extends = Some(val.value()),
                "contract" => contract = Some(val.value()),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown varn_contract! key `{other}`"),
                    ))
                }
            }
            let _ = input.parse::<Token![,]>();
        }

        let imp: syn::ItemImpl = input.parse()?;
        let self_ty = (*imp.self_ty).clone();
        let fns = imp
            .items
            .into_iter()
            .filter_map(|it| match it {
                syn::ImplItem::Fn(f) => Some(f),
                _ => None,
            })
            .collect();

        let span = input.span();
        Ok(Self {
            module: module.ok_or_else(|| syn::Error::new(span, "missing `module:` key"))?,
            class,
            extends,
            contract: contract.ok_or_else(|| syn::Error::new(span, "missing `contract:` key"))?,
            self_ty,
            fns,
        })
    }
}

#[derive(Clone)]
enum Mapped {
    Int,
    Float,
    Bool,
    Char,
    Str,
    Array,
    Dynamic,
    Void,
    Opt(Box<Mapped>),
}

/// Scalar marshalling shape for a [`TypeTag`]. Deliberately excludes `Array`:
/// `Array` reaches `Mapped::Array` only structurally (a `T[]` node in
/// [`classify`]) or as an explicit receiver in [`receiver_mapped`], never from
/// a `Named` value-type position.
fn scalar_mapped(tag: TypeTag) -> Mapped {
    match tag {
        TypeTag::Int => Mapped::Int,
        TypeTag::Float => Mapped::Float,
        TypeTag::Bool => Mapped::Bool,
        TypeTag::Char => Mapped::Char,
        TypeTag::Str => Mapped::Str,
        TypeTag::Void => Mapped::Void,
        _ => Mapped::Dynamic,
    }
}

fn classify(t: &TypeNode) -> Mapped {
    match &t.kind {
        TypeKind::Named(n, _) => TypeTag::from_str(n.as_str())
            .map(scalar_mapped)
            .unwrap_or(Mapped::Dynamic),
        TypeKind::Intrinsic(TypeTag::Void) => Mapped::Void,
        TypeKind::Array(_) => Mapped::Array,
        TypeKind::Union(members) if members.len() == 2 => {
            if matches!(members[1].kind, TypeKind::Intrinsic(TypeTag::Null)) {
                Mapped::Opt(Box::new(classify(&members[0])))
            } else if matches!(members[0].kind, TypeKind::Intrinsic(TypeTag::Null)) {
                Mapped::Opt(Box::new(classify(&members[1])))
            } else {
                Mapped::Dynamic
            }
        }
        _ => Mapped::Dynamic,
    }
}

fn receiver_mapped(class: &str) -> Mapped {
    if class == IntrinsicType::Array.as_str() {
        return Mapped::Array;
    }
    TypeTag::from_str(class)
        .map(scalar_mapped)
        .unwrap_or(Mapped::Dynamic)
}

fn param_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Int => quote!(i64),
        Mapped::Float => quote!(f64),
        Mapped::Bool => quote!(bool),
        Mapped::Char => quote!(char),
        Mapped::Str => quote!(&str),
        Mapped::Array => quote!(::varn_types::VnArray),
        Mapped::Dynamic => quote!(::varn_types::VmValue),
        Mapped::Void => quote!(()),
        Mapped::Opt(inner) => {
            let i = param_ty(inner);
            quote!(::core::option::Option<#i>)
        }
    }
}

fn owned_ty(m: &Mapped) -> TS2 {
    match m {
        // Zero-copy: Rc clone / inline SSO buffer instead of an owned
        // String allocation per call.
        Mapped::Str => quote!(::varn_types::VnStr),
        Mapped::Opt(inner) => {
            let i = owned_ty(inner);
            quote!(::core::option::Option<#i>)
        }
        _ => param_ty(m),
    }
}

fn ret_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(String),
        Mapped::Array => quote!(::std::vec::Vec<::varn_types::VmValue>),
        Mapped::Opt(inner) => {
            let i = ret_ty(inner);
            quote!(::core::option::Option<#i>)
        }
        _ => param_ty(m),
    }
}

fn call_expr(binding: &Ident, m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(#binding.as_str()),
        Mapped::Opt(inner) if matches!(**inner, Mapped::Str) => {
            quote!(#binding.as_ref().map(|s| s.as_str()))
        }
        _ => quote!(#binding),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Method,
    Getter,
    StaticMethod,
    StaticGetter,
    Constructor,
    Property,

    Function,
}

struct ParamInfo {
    mapped: Mapped,
    is_rest: bool,
}

struct Member {
    symbol: String,
    kind: Kind,
    params: Vec<ParamInfo>,
    ret: Mapped,
}

fn param_name_is_rest(p: &Param) -> bool {
    p.is_rest || matches!(p.pattern, Pattern::Rest { .. })
}

fn collect_members(class_name: &str, decl: &ClassDecl) -> Vec<Member> {
    let mut out = Vec::new();
    for m in &decl.body {
        match m {
            ClassMember::Method {
                key,
                params,
                return_type,
                modifiers,
                ..
            } => {
                let kind = if modifiers.is_static {
                    Kind::StaticMethod
                } else {
                    Kind::Method
                };
                out.push(Member {
                    symbol: key.to_string(),
                    kind,
                    params: map_params(params),
                    ret: return_type.as_ref().map(classify).unwrap_or(Mapped::Void),
                });
            }
            ClassMember::Getter {
                key,
                return_type,
                modifiers,
                ..
            } => {
                let kind = if modifiers.is_static {
                    Kind::StaticGetter
                } else {
                    Kind::Getter
                };
                out.push(Member {
                    symbol: key.to_string(),
                    kind,
                    params: vec![],
                    ret: return_type
                        .as_ref()
                        .map(classify)
                        .unwrap_or(Mapped::Dynamic),
                });
            }

            ClassMember::Property {
                key,
                type_ann,
                init: None,
                modifiers,
                ..
            } => {
                let kind = if modifiers.is_static {
                    Kind::StaticGetter
                } else if modifiers.is_readonly {
                    Kind::Getter
                } else {
                    Kind::Property
                };
                out.push(Member {
                    symbol: key.to_string(),
                    kind,
                    params: vec![],
                    ret: type_ann.as_ref().map(classify).unwrap_or(Mapped::Dynamic),
                });
            }
            ClassMember::Constructor { params, .. } => {
                out.push(Member {
                    symbol: "constructor".to_string(),
                    kind: Kind::Constructor,
                    params: map_params(params),
                    ret: Mapped::Dynamic,
                });
            }
            _ => {}
        }
    }
    let _ = class_name;
    out
}

fn collect_functions(body: &[Stmt]) -> Vec<Member> {
    fn from_decl(decl: &Decl, out: &mut Vec<Member>) {
        match decl {
            Decl::Function(f) => out.push(function_member(f)),
            Decl::Export(ExportDecl::Decl { declaration, .. }) => from_decl(declaration, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for stmt in body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            from_decl(decl, &mut out);
        }
    }
    out
}

fn function_member(f: &FunctionDecl) -> Member {
    Member {
        symbol: f.id.to_string(),
        kind: Kind::Function,
        params: map_params(&f.params),
        ret: f.return_type.as_ref().map(classify).unwrap_or(Mapped::Void),
    }
}

fn param_type(p: &Param) -> Option<&TypeNode> {
    if let Some(t) = &p.type_ann {
        return Some(t);
    }
    if let Pattern::Identifier {
        type_ann: Some(t), ..
    } = &p.pattern
    {
        return Some(t);
    }
    None
}

fn map_params(params: &[Param]) -> Vec<ParamInfo> {
    params
        .iter()
        .map(|p| {
            let is_rest = param_name_is_rest(p);
            let base = param_type(p).map(classify).unwrap_or(Mapped::Dynamic);
            let mapped = if p.is_optional && !is_rest {
                Mapped::Opt(Box::new(base))
            } else {
                base
            };
            ParamInfo { mapped, is_rest }
        })
        .collect()
}

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
    let program = match varn_parser::parse(tokens, lexeme_buf, &input.contract) {
        Ok(p) => p,
        Err(_) => return err(format!("failed to parse contract `{}`", abs_path_str)),
    };

    let members = match &input.class {
        Some(class) => match find_class(&program.body, class) {
            Some(decl) => collect_members(class, &decl),
            None => {
                return err(format!(
                    "class `{class}` not found in contract `{abs_path_str}`"
                ))
            }
        },
        None => {
            let fns = collect_functions(&program.body);
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

    let mut trait_sigs: Vec<TS2> = Vec::new();
    let mut wrappers: Vec<TS2> = Vec::new();
    let mut setup_calls: Vec<TS2> = Vec::new();
    let mut fn_entries: Vec<TS2> = Vec::new();
    // Per-method `NativeOpEntry`s so core-type methods/getters are addressable by
    // a stable op-id (`module::class::symbol`) for direct dispatch — in addition
    // to living in the class vtable via `setup_calls`.
    let mut method_entries: Vec<TS2> = Vec::new();
    let mut generated_entry_idents: Vec<syn::Ident> = Vec::new();

    for m in &members {
        let sym = &m.symbol;
        if m.kind == Kind::Property {
            setup_calls.push(quote! {
                cls.declare_field(::std::rc::Rc::from(#sym));
            });
            continue;
        }
        let rust_sym = sym.trim_end_matches('$');
        let method_ident = format_ident!("{}", rust_sym);
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
                        match ctx.extract(first) {
                            ::varn_types::Value::Null | ::varn_types::Value::Class(_) | ::varn_types::Value::Module(_) => 1usize,
                            _ => 0usize,
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
        } else if is_void {
            quote!(())
        } else {
            rty.clone()
        };

        trait_sigs.push(quote! {
            #[allow(non_snake_case)]
            fn #method_ident(ctx: &mut dyn ::varn_types::NativeCtx, #(#sig_params),*) -> #trait_ret;
        });

        let call = quote!(<__T>::#method_ident(ctx, #(#call_args),*));
        let ret_encode = match (is_fn, is_void) {
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

        wrappers.push(quote! {
            #[allow(non_snake_case)]
            pub fn #wrap_ident<__T: #trait_ident>(
                ctx: &mut dyn ::varn_types::NativeCtx,
                args: &[::varn_types::VmValue],
            ) -> ::core::result::Result<::varn_types::VmValue, String> {
                #(#decode)*
                #ret_encode
            }
        });

        // Fast path wrapper generation
        let is_fast = is_fast_eligible(m);
        let (fast_wrapper, raw_func_val, sig_val) = if is_fast {
            let mut fast_sig_params = Vec::new();
            let mut fast_call_args = Vec::new();
            let mut fast_decode = Vec::new();
            let fallback = default_value_token(&m.ret);

            if m.kind == Kind::Method {
                fast_sig_params.push(quote!(this: ::varn_types::VmValue));
                let recv = receiver_mapped(&prefix);
                let oty = owned_ty(&recv);
                fast_decode.push(quote! {
                    let __this = match <#oty as ::varn_types::marshal::FromVm>::from_vm(&mut dummy_ctx, this) {
                        Ok(v) => v,
                        Err(_) => return #fallback,
                    };
                });
                fast_call_args.push(call_expr(&format_ident!("__this"), &recv));
            }

            for (i, p) in m.params.iter().enumerate() {
                let pname = format_ident!("__p{}", i);
                let pty = param_ty(&p.mapped);
                fast_sig_params.push(quote!(#pname: #pty));
                fast_call_args.push(call_expr(&pname, &p.mapped));
            }

            let fast_ret = ret_ty(&m.ret);
            let is_fn = m.kind == Kind::Function;

            let call = quote!(<__T>::#method_ident(&mut dummy_ctx, #(#fast_call_args),*));
            let fast_body = if is_fn {
                quote! {
                    let mut dummy_ctx = ::varn_types::native::DummyCtx;
                    #(#fast_decode)*
                    match #call {
                        Ok(v) => v,
                        Err(_) => #fallback,
                    }
                }
            } else {
                quote! {
                    let mut dummy_ctx = ::varn_types::native::DummyCtx;
                    #(#fast_decode)*
                    #call
                }
            };

            let wrapper = quote! {
                #[allow(non_snake_case)]
                pub extern "C" fn #fast_wrap_ident<__T: #trait_ident>(
                    #(#fast_sig_params),*
                ) -> #fast_ret {
                    #fast_body
                }
            };

            let raw_ptr = quote!(#fast_wrap_ident::<#self_ty> as *const u8);
            let sig = signature_token(m);
            (wrapper, raw_ptr, sig)
        } else {
            (
                quote!(),
                quote!(::core::ptr::null()),
                quote!(::varn_types::SignatureDescriptor::empty()),
            )
        };

        wrappers.push(fast_wrapper);

        if m.kind == Kind::Function {
            let fn_entry_ident = format_ident!(
                "__VARN_OP_{}_{}",
                sanitize(&prefix).to_uppercase(),
                sanitize(sym).to_uppercase()
            );
            generated_entry_idents.push(fn_entry_ident.clone());
            fn_entries.push(quote! {
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
            let native = quote!(::varn_types::Value::native(#wrap_ident::<#self_ty>, #sym));
            let setup = match m.kind {
                Kind::Method => quote!(cls.add_method(#sym, #native);),
                Kind::Getter => quote!(cls.add_getter(#sym, #native);),
                Kind::StaticMethod => quote!(cls.add_static(#sym, #native);),
                Kind::StaticGetter => quote!(cls.add_static_getter(#sym, #native);),
                Kind::Constructor => {
                    quote!(cls.add_method("constructor", ::varn_types::Value::native(#wrap_ident::<#self_ty>, "constructor"));)
                }
                Kind::Function | Kind::Property => unreachable!(),
            };
            setup_calls.push(setup);

            // Emit a stable, op-id-addressable dispatch entry for callable
            // members (skip the constructor — invoked via `new`, not by op-id).
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
                generated_entry_idents.push(mentry_ident.clone());
                method_entries.push(quote! {
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
            ) -> ::core::result::Result<::varn_types::VmValue, String> {
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

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

fn err(msg: String) -> TokenStream {
    let lit = LitStr::new(&msg, proc_macro2::Span::call_site());
    TokenStream::from(quote! { compile_error!(#lit); })
}

fn find_class(body: &[Stmt], name: &str) -> Option<ClassDecl> {
    for stmt in body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Some(c) = class_from_decl(decl, name) {
                return Some(c);
            }
        }
    }
    None
}

fn class_from_decl(decl: &Decl, name: &str) -> Option<ClassDecl> {
    match decl {
        Decl::Class(c) => {
            if c.id.as_deref() == Some(name) {
                Some(c.clone())
            } else {
                None
            }
        }
        Decl::Export(ExportDecl::Decl { declaration, .. }) => class_from_decl(declaration, name),
        _ => None,
    }
}

fn map_to_arg_type_token(m: &Mapped) -> TS2 {
    match m {
        Mapped::Int => quote!(::varn_types::ArgType::Int),
        Mapped::Float => quote!(::varn_types::ArgType::Float),
        Mapped::Bool => quote!(::varn_types::ArgType::Bool),
        Mapped::Char => quote!(::varn_types::ArgType::Char),
        Mapped::Str => quote!(::varn_types::ArgType::Str),
        Mapped::Array => quote!(::varn_types::ArgType::Generic),
        Mapped::Dynamic => quote!(::varn_types::ArgType::Generic),
        Mapped::Void => quote!(::varn_types::ArgType::Void),
        Mapped::Opt(_) => quote!(::varn_types::ArgType::Generic),
    }
}

fn is_scalar(m: &Mapped) -> bool {
    matches!(
        m,
        Mapped::Int | Mapped::Float | Mapped::Bool | Mapped::Char | Mapped::Void
    )
}

fn is_fast_eligible(m: &Member) -> bool {
    if !matches!(m.kind, Kind::Function | Kind::StaticMethod) {
        return false;
    }
    if !is_scalar(&m.ret) {
        return false;
    }
    for p in &m.params {
        if p.is_rest || !is_scalar(&p.mapped) {
            return false;
        }
    }
    true
}

fn signature_token(m: &Member) -> TS2 {
    let ret_token = map_to_arg_type_token(&m.ret);
    let mut param_tokens = Vec::new();

    if m.kind == Kind::Method {
        param_tokens.push(quote!(::varn_types::ArgType::Generic));
    }

    for p in m.params.iter().take(7) {
        param_tokens.push(map_to_arg_type_token(&p.mapped));
    }
    while param_tokens.len() < 7 {
        param_tokens.push(quote!(::varn_types::ArgType::Void));
    }
    let count = (m.params.len() + if m.kind == Kind::Method { 1 } else { 0 }) as u8;
    quote! {
        ::varn_types::SignatureDescriptor {
            return_type: #ret_token,
            param_count: #count,
            param_types: [ #(#param_tokens),* ],
        }
    }
}

fn default_value_token(m: &Mapped) -> TS2 {
    match m {
        Mapped::Dynamic => quote!(::varn_types::VmValue::null()),
        _ => quote!(Default::default()),
    }
}
