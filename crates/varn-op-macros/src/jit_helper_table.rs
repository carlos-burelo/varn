use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use syn::{parse_macro_input, Item, LitStr, Meta, Token};

struct Invocation {
    mode: syn::Ident,
    _comma: Token![,],
    dir: LitStr,
}

impl syn::parse::Parse for Invocation {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Invocation {
            mode: input.parse()?,
            _comma: input.parse()?,
            dir: input.parse()?,
        })
    }
}

fn attr_field(attr: &syn::Attribute) -> Result<Option<String>, String> {
    match &attr.meta {
        Meta::Path(_) => Ok(None),
        Meta::List(_) => {
            let nv: syn::MetaNameValue = attr
                .parse_args()
                .map_err(|_| "jit_slow: se admite solo `field = \"nombre\"`".to_owned())?;
            if !nv.path.is_ident("field") {
                return Err("jit_slow: se admite solo `field = \"nombre\"`".to_owned());
            }
            if let syn::Expr::Lit(e) = nv.value {
                if let syn::Lit::Str(s) = e.lit {
                    Ok(Some(s.value()))
                } else {
                    Err("jit_slow: field debe ser string".to_owned())
                }
            } else {
                Err("jit_slow: field debe ser string".to_owned())
            }
        }
        Meta::NameValue(nv) => {
            if !nv.path.is_ident("field") {
                return Err("jit_slow: se admite solo `field = \"nombre\"`".to_owned());
            }
            if let syn::Expr::Lit(e) = &nv.value {
                if let syn::Lit::Str(s) = &e.lit {
                    Ok(Some(s.value()))
                } else {
                    Err("jit_slow: field debe ser string".to_owned())
                }
            } else {
                Err("jit_slow: field debe ser string".to_owned())
            }
        }
    }
}

fn is_jit_slow(attr: &syn::Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|s| s.ident == "jit_slow")
}

pub fn expand(input: TokenStream) -> TokenStream {
    let inv = parse_macro_input!(input as Invocation);
    let mode = inv.mode.to_string();
    if mode != "define" && mode != "fill" {
        return quote! { compile_error!("jit_helper_table: modo `define` o `fill`"); }.into();
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned());
    let dir = std::path::PathBuf::from(manifest).join(inv.dir.value());

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let read_dir = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("jit_helper_table: no lee {}: {e}", dir.display());
            return quote! { compile_error!(#msg); }.into();
        }
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files.sort();

    let mut by_field: BTreeMap<String, (String, String)> = BTreeMap::new();
    for path in &files {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("jit_helper_table: no lee {}: {e}", path.display());
                return quote! { compile_error!(#msg); }.into();
            }
        };
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(e) => {
                let msg = format!("jit_helper_table: no parsea {}: {e}", path.display());
                return quote! { compile_error!(#msg); }.into();
            }
        };
        let module = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        for item in &file.items {
            let Item::Fn(func) = item else { continue };
            let mut field: Option<String> = None;
            for attr in &func.attrs {
                if !is_jit_slow(attr) {
                    continue;
                }
                match attr_field(attr) {
                    Ok(f) => {
                        if f.is_some() {
                            field = f;
                        }
                    }
                    Err(msg) => {
                        let full = format!("{} ({}::{})", msg, module, func.sig.ident);
                        return quote! { compile_error!(#full); }.into();
                    }
                }
            }
            let Some(field) = field else { continue };
            let func_name = func.sig.ident.to_string();
            if by_field.contains_key(&field) {
                let msg = format!("jit_helper_table: campo duplicado `{field}`");
                return quote! { compile_error!(#msg); }.into();
            }
            by_field.insert(field, (module.clone(), func_name));
        }
    }

    if mode == "define" {
        let fields = by_field.keys().map(|f| {
            let ident = format_ident!("{}", f);
            quote! { #ident, }
        });
        return quote! { define_tail! { #(#fields)* } }.into();
    }

    let fills = by_field.iter().map(|(field, (module, func))| {
        let field_ident = format_ident!("{}", field);
        let module_ident = format_ident!("{}", module);
        let func_ident = format_ident!("{}", func);
        quote! {
            #field_ident: crate::exec::jit_helpers::#module_ident::#func_ident,
        }
    });
    quote! { fill_tail!(#(#fills)*) }.into()
}
