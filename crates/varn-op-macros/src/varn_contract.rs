//! `varn_contract!` — the contract-driven native binding generator.
//!
//! The `.vn` declaration of a builtin type is the single source of truth.
//! This macro reads that contract at expansion time (via the real
//! `varn-lexer`/`varn-parser`), turns every declared member into a typed Rust
//! trait method, re-emits the caller-supplied bodies as `impl Trait for T`,
//! and generates the `&[VmValue]` dispatch wrappers + the class builder +
//! the linker-section registration entry.
//!
//! Because the trait is generated from the contract and the bodies are placed
//! in an `impl` of that trait, *drift becomes a Rust compile error*:
//!   - a declared method with no body  → "not all trait items implemented"
//!   - a body whose name/type is wrong → trait mismatch / unknown-method error
//!
//! Usage:
//! ```ignore
//! varn_contract! {
//!     module: "globals",
//!     class: "bool",
//!     contract: "src/modules/primitives/bool/bool.vn",
//!     impl Bool {
//!         fn toString(_ctx: &mut dyn NativeCtx, this: bool) -> String { ... }
//!         fn valueOf(_ctx: &mut dyn NativeCtx, this: bool) -> bool { this }
//!     }
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TS2;
use quote::{format_ident, quote};
use std::path::Path;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

use varn_core::ast::{ClassDecl, ClassMember, Decl, ExportDecl, Param, Pattern, Stmt, StmtKind};
use varn_core::ast::{FunctionDecl, TypeNode};
use varn_core::kinds::TypeKind;
use varn_core::TypeTag;

// ---------------------------------------------------------------------------
// Macro input
// ---------------------------------------------------------------------------

pub(crate) struct ContractInput {
    module: String,
    /// `Some` for a `declare class` contract; `None` for a function-module
    /// contract (top-level `declare function`s).
    class: Option<String>,
    contract: String,
    self_ty: syn::Type,
    fns: Vec<syn::ImplItemFn>,
}

impl Parse for ContractInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut module = None;
        let mut class = None;
        let mut contract = None;

        while !input.peek(Token![impl]) {
            let key: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            let val: LitStr = input.parse()?;
            match key.to_string().as_str() {
                "module" => module = Some(val.value()),
                "class" => class = Some(val.value()),
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
            contract: contract.ok_or_else(|| syn::Error::new(span, "missing `contract:` key"))?,
            self_ty,
            fns,
        })
    }
}

// ---------------------------------------------------------------------------
// Type mapping (contract TypeNode -> Rust types)
// ---------------------------------------------------------------------------

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

fn classify(t: &TypeNode) -> Mapped {
    match &t.kind {
        TypeKind::Named(n, _) => match n.as_str() {
            "int" => Mapped::Int,
            "float" | "number" => Mapped::Float,
            "bool" => Mapped::Bool,
            "char" => Mapped::Char,
            "str" | "string" => Mapped::Str,
            "void" => Mapped::Void,
            _ => Mapped::Dynamic,
        },
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

/// Receiver (`this`) type for an instance member of `class`.
fn receiver_mapped(class: &str) -> Mapped {
    match class {
        "str" => Mapped::Str,
        "int" => Mapped::Int,
        "float" => Mapped::Float,
        "bool" => Mapped::Bool,
        "char" => Mapped::Char,
        "Array" => Mapped::Array,
        _ => Mapped::Dynamic,
    }
}

/// Type as it appears in the trait signature (what the body author sees).
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

/// Owned type produced by `FromVm` for decoding (the wrapper local binding).
fn owned_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(String),
        Mapped::Opt(inner) => {
            let i = owned_ty(inner);
            quote!(::core::option::Option<#i>)
        }
        _ => param_ty(m),
    }
}

/// Return type in the trait signature.
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

/// How a decoded local binding is passed to the trait method.
fn call_expr(binding: &Ident, m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(&#binding),
        Mapped::Opt(inner) if matches!(**inner, Mapped::Str) => quote!(#binding.as_deref()),
        _ => quote!(#binding),
    }
}

// ---------------------------------------------------------------------------
// Member model
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Method,
    Getter,
    StaticMethod,
    StaticGetter,
    Constructor,
    /// A free `declare function` in a function-module contract. Like a static
    /// method (no receiver) but registered as a standalone module op.
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
                    ret: return_type.as_ref().map(classify).unwrap_or(Mapped::Dynamic),
                });
            }
            // `readonly P: T;` in a declare class is a getter-shaped property.
            ClassMember::Property {
                key,
                type_ann,
                init: None,
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
                    ret: type_ann.as_ref().map(classify).unwrap_or(Mapped::Dynamic),
                });
            }
            ClassMember::Constructor { params, .. } => {
                // A native constructor returns a `VmValue`: returning `this`
                // keeps the VM-allocated instance, returning a fresh value
                // replaces it (how Set/Map/Range become their backing Value).
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

/// Collect top-level `export declare function`s for a function-module contract.
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

/// The parser stores a param's type either on `Param.type_ann` (optional
/// params) or on the identifier pattern (`name: T`), so check both.
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

// ---------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as ContractInput);

    // Read + parse the contract `.vn` at macro-expansion time.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let abs_path = Path::new(&manifest_dir).join(&input.contract);
    let abs_path_str = abs_path.to_string_lossy().replace('\\', "/");
    let source = match std::fs::read_to_string(&abs_path) {
        Ok(s) => s,
        Err(e) => {
            return err(format!("cannot read contract `{}`: {e}", abs_path_str));
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

    // Naming prefix: the class name, or the module id for a function-module.
    let prefix = input.class.clone().unwrap_or_else(|| input.module.clone());
    let trait_ident = format_ident!("__VarnContract_{}", sanitize(&prefix));

    let self_ty = &input.self_ty;
    let user_fns = &input.fns;
    let module = &input.module;
    let ns = "";

    let mut trait_sigs: Vec<TS2> = Vec::new();
    let mut wrappers: Vec<TS2> = Vec::new();
    let mut setup_calls: Vec<TS2> = Vec::new();
    let mut fn_entries: Vec<TS2> = Vec::new();

    for m in &members {
        let sym = &m.symbol;
        let method_ident = format_ident!("{}", sym);
        let wrap_ident = format_ident!("__varn_wrap_{}_{}", sanitize(&prefix), sanitize(sym));

        // Build the receiver param (if any), the trait params and the wrapper
        // decode statements + call args.
        let mut sig_params: Vec<TS2> = Vec::new();
        let mut decode: Vec<TS2> = Vec::new();
        let mut call_args: Vec<TS2> = Vec::new();

        // index into `args` where positional contract params start
        let mut arg_base = 0usize;

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
                // The object being constructed is passed raw.
                sig_params.push(quote!(this: ::varn_types::VmValue));
                let b = format_ident!("__this");
                decode.push(quote! {
                    let #b = args.first().copied().unwrap_or(::varn_types::VmValue::null());
                });
                call_args.push(quote!(#b));
                arg_base = 1;
            }
            Kind::StaticMethod | Kind::StaticGetter | Kind::Function => {
                // positional params start at args[0]
            }
        }

        for (i, p) in m.params.iter().enumerate() {
            let pname = format_ident!("__p{}", i);
            let arg_idx = arg_base + i;
            if p.is_rest {
                sig_params.push(quote!(#pname: &[::varn_types::VmValue]));
                decode.push(quote! {
                    let #pname: &[::varn_types::VmValue] =
                        if args.len() > #arg_idx { &args[#arg_idx..] } else { &[] };
                });
                call_args.push(quote!(#pname));
            } else {
                let pty = param_ty(&p.mapped);
                let oty = owned_ty(&p.mapped);
                sig_params.push(quote!(#pname: #pty));
                decode.push(quote! {
                    let #pname = <#oty as ::varn_types::marshal::FromVm>::from_vm(
                        ctx,
                        args.get(#arg_idx).copied().unwrap_or(::varn_types::VmValue::null()),
                    )?;
                });
                call_args.push(call_expr(&pname, &p.mapped));
            }
        }

        let rty = ret_ty(&m.ret);
        let is_void = matches!(m.ret, Mapped::Void);
        let is_fn = m.kind == Kind::Function;

        // Function-module ops sit on the fallible native boundary, so their
        // bodies return `Result<T, String>` and the wrapper propagates the
        // error. Class members are pure and return `T` directly.
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

        // Registration: a function-module op is a standalone linker entry;
        // class members are wired onto the ClassObj inside the builder.
        if m.kind == Kind::Function {
            let fn_entry_ident = format_ident!(
                "__VARN_OP_{}_{}",
                sanitize(&prefix).to_uppercase(),
                sanitize(sym).to_uppercase()
            );
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
                Kind::Function => unreachable!(),
            };
            setup_calls.push(setup);
        }
    }

    let abs_lit = LitStr::new(&abs_path_str, proc_macro2::Span::call_site());

    let registration = if let Some(class) = &input.class {
        let builder_ident = format_ident!("__varn_build_{}", sanitize(class));
        let entry_ident = format_ident!("__VARN_OP_{}", sanitize(class).to_uppercase());
        quote! {
            pub fn #builder_ident(
                ctx: &mut dyn ::varn_types::NativeCtx,
                _args: &[::varn_types::VmValue],
            ) -> ::core::result::Result<::varn_types::VmValue, String> {
                let cls = ::varn_types::value::ClassObj::new_native_rc(#class);
                #(#setup_calls)*
                ctx.register_class(#class, cls.clone());
                Ok(ctx.alloc_class(cls))
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
                capability_mask: 0,
                entry_kind: 0x10,
                flags: 0,
                _reserved: [0; 7],
            };
        }
    } else {
        quote! { #(#fn_entries)* }
    };

    let out = quote! {
        // Force a rebuild whenever the contract file changes.
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
