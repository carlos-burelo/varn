use proc_macro::TokenStream;
use proc_macro2::TokenStream as TS2;
use quote::quote;
use syn::{
    parse::Parse, parse::ParseStream, parse_macro_input, Expr, Ident, Item, ItemMod, Lit, LitStr,
    Token,
};

pub(crate) struct ModuleArgs {
    pub name: String,
    pub default_cap: Option<String>,
}

impl Parse for ModuleArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        let mut default_cap = None;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let val: LitStr = input.parse()?;
            if key == "cap" {
                default_cap = Some(val.value());
            }
        }
        Ok(Self {
            name: name.value(),
            default_cap,
        })
    }
}

struct OpArgs {
    name: Option<String>,
    is_async: bool,
    cap: Option<String>,
}

impl Parse for OpArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut is_async = false;
        let mut cap = None;

        while !input.is_empty() {
            if input.peek(LitStr) {
                name = Some(input.parse::<LitStr>()?.value());
            } else if input.peek(Ident) {
                let key: Ident = input.parse()?;
                match key.to_string().as_str() {
                    "async" => is_async = true,
                    "cap" => {
                        input.parse::<Token![=]>()?;
                        cap = Some(input.parse::<LitStr>()?.value());
                    }
                    "name" => {
                        input.parse::<Token![=]>()?;
                        name = Some(input.parse::<LitStr>()?.value());
                    }
                    _ => {}
                }
            }
            let _ = input.parse::<Token![,]>();
        }
        Ok(Self {
            name,
            is_async,
            cap,
        })
    }
}

struct GroupArgs {
    name: String,
    cap: Option<String>,
}

impl Parse for GroupArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        let mut cap = None;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let val: LitStr = input.parse()?;
            if key == "cap" {
                cap = Some(val.value());
            }
        }
        Ok(Self {
            name: name.value(),
            cap,
        })
    }
}

struct ClassArgs {
    name: String,
    extends: Option<String>,
}

impl Parse for ClassArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        let mut extends = None;
        if !input.is_empty() {
            let _: Token![,] = input.parse()?;
            if !input.is_empty() {
                let key: Ident = input.parse()?;
                if key == "extends" {
                    let _: Token![=] = input.parse()?;
                    let parent: LitStr = input.parse()?;
                    extends = Some(parent.value());
                }
            }
        }
        Ok(Self {
            name: name.value(),
            extends,
        })
    }
}

enum ClassMember {
    Constructor { fn_ident: Ident },
    Method { symbol: String, fn_ident: Ident },
    Getter { symbol: String, fn_ident: Ident },
    Static { symbol: String, fn_ident: Ident },
    StaticGetter { symbol: String, fn_ident: Ident },

    ConstStr { symbol: String, value: String },
}

fn scan_class_items(
    items: &[Item],
    class_lower: &str,
    mod_path: &[Ident],
) -> (Vec<ClassMember>, Vec<TS2>, Vec<TS2>) {
    let mut members: Vec<ClassMember> = vec![];
    let mut clean: Vec<TS2> = vec![];
    let mut dispatch_fns: Vec<TS2> = vec![];

    for item in items {
        if let Item::Fn(f) = item {
            let fn_ident = &f.sig.ident;
            let mut kind: Option<ClassMember> = None;
            let mut other_attrs: Vec<TS2> = vec![];

            for attr in &f.attrs {
                if attr.path().is_ident("varn_constructor") {
                    kind = Some(ClassMember::Constructor {
                        fn_ident: fn_ident.clone(),
                    });
                } else if attr.path().is_ident("varn_method") {
                    if let Ok(sym) = attr.parse_args::<LitStr>() {
                        kind = Some(ClassMember::Method {
                            symbol: sym.value(),
                            fn_ident: fn_ident.clone(),
                        });
                    } else {
                        kind = Some(ClassMember::Method {
                            symbol: fn_ident.to_string(),
                            fn_ident: fn_ident.clone(),
                        });
                    }
                } else if attr.path().is_ident("varn_getter") {
                    if let Ok(sym) = attr.parse_args::<LitStr>() {
                        kind = Some(ClassMember::Getter {
                            symbol: sym.value(),
                            fn_ident: fn_ident.clone(),
                        });
                    } else {
                        kind = Some(ClassMember::Getter {
                            symbol: fn_ident.to_string(),
                            fn_ident: fn_ident.clone(),
                        });
                    }
                } else if attr.path().is_ident("varn_static") {
                    if let Ok(sym) = attr.parse_args::<LitStr>() {
                        kind = Some(ClassMember::Static {
                            symbol: sym.value(),
                            fn_ident: fn_ident.clone(),
                        });
                    } else {
                        kind = Some(ClassMember::Static {
                            symbol: fn_ident.to_string(),
                            fn_ident: fn_ident.clone(),
                        });
                    }
                } else if attr.path().is_ident("varn_static_getter") {
                    if let Ok(sym) = attr.parse_args::<LitStr>() {
                        kind = Some(ClassMember::StaticGetter {
                            symbol: sym.value(),
                            fn_ident: fn_ident.clone(),
                        });
                    } else {
                        kind = Some(ClassMember::StaticGetter {
                            symbol: fn_ident.to_string(),
                            fn_ident: fn_ident.clone(),
                        });
                    }
                } else {
                    other_attrs.push(quote! { #attr });
                }
            }

            if let Some(member) = kind {
                let sig = &f.sig;
                let block = &f.block;
                let vis = &f.vis;
                clean.push(quote! {
                    #(#other_attrs)*
                    #vis #sig #block
                });

                let dispatch_ident_str = format!("__dispatch_{}_{}", class_lower, fn_ident);
                let dispatch_ident = Ident::new(&dispatch_ident_str, fn_ident.span());
                let full_path = quote! { #(#mod_path::)* #fn_ident };

                match &member {
                    ClassMember::Constructor { .. } => {
                        dispatch_fns.push(quote! {
                            pub fn #dispatch_ident(
                                ctx: &mut dyn ::varn_types::NativeCtx,
                                args: &[::varn_types::VmValue],
                            ) -> Result<::varn_types::VmValue, String> {
                                let this = args.first().copied().unwrap_or(::varn_types::VmValue::null());
                                let rest = if args.len() > 1 { &args[1..] } else { &[] };
                                #full_path(ctx, this, rest)?;
                                Ok(::varn_types::VmValue::null())
                            }
                        });
                    }
                    ClassMember::Method { .. } => {
                        dispatch_fns.push(quote! {
                            pub fn #dispatch_ident(
                                ctx: &mut dyn ::varn_types::NativeCtx,
                                args: &[::varn_types::VmValue],
                            ) -> Result<::varn_types::VmValue, String> {
                                let this = args.first().copied().unwrap_or(::varn_types::VmValue::null());
                                let rest = if args.len() > 1 { &args[1..] } else { &[] };
                                #full_path(ctx, this, rest)
                            }
                        });
                    }
                    ClassMember::Getter { .. } => {
                        dispatch_fns.push(quote! {
                            pub fn #dispatch_ident(
                                ctx: &mut dyn ::varn_types::NativeCtx,
                                args: &[::varn_types::VmValue],
                            ) -> Result<::varn_types::VmValue, String> {
                                let this = args.first().copied().unwrap_or(::varn_types::VmValue::null());
                                #full_path(ctx, this)
                            }
                        });
                    }
                    ClassMember::Static { .. } => {
                        dispatch_fns.push(quote! {
                            pub fn #dispatch_ident(
                                ctx: &mut dyn ::varn_types::NativeCtx,
                                args: &[::varn_types::VmValue],
                            ) -> Result<::varn_types::VmValue, String> {
                                #full_path(ctx, args)
                            }
                        });
                    }
                    ClassMember::StaticGetter { .. } => {
                        dispatch_fns.push(quote! {
                            pub fn #dispatch_ident(
                                ctx: &mut dyn ::varn_types::NativeCtx,
                                args: &[::varn_types::VmValue],
                            ) -> Result<::varn_types::VmValue, String> {
                                #full_path(ctx)
                            }
                        });
                    }
                    ClassMember::ConstStr { .. } => {}
                }

                members.push(member);
            } else {
                clean.push(quote! { #item });
            }
        } else if let Item::Const(c) = item {
            let mut const_sym: Option<String> = None;
            let mut other_attrs: Vec<TS2> = vec![];
            for attr in &c.attrs {
                if attr.path().is_ident("varn_const") {
                    if let Ok(sym) = attr.parse_args::<LitStr>() {
                        const_sym = Some(sym.value());
                    } else {
                        const_sym = Some(c.ident.to_string());
                    }
                } else {
                    other_attrs.push(quote! { #attr });
                }
            }
            if let Some(sym) = const_sym {
                let str_val = match c.expr.as_ref() {
                    Expr::Lit(el) => match &el.lit {
                        Lit::Str(s) => s.value(),
                        _ => String::new(),
                    },
                    _ => String::new(),
                };
                members.push(ClassMember::ConstStr {
                    symbol: sym,
                    value: str_val,
                });

                let vis = &c.vis;
                let ident = &c.ident;
                let ty = &c.ty;
                let expr = &c.expr;
                clean.push(quote! {
                    #(#other_attrs)*
                    #vis const #ident: #ty = #expr;
                });
            } else {
                clean.push(quote! { #item });
            }
        } else {
            clean.push(quote! { #item });
        }
    }

    (members, clean, dispatch_fns)
}

struct CollectedOp {
    canonical: String,
    namespace_path: String,
    fn_ident: Ident,
    path: Vec<Ident>,
    is_async: bool,
    cap: Option<String>,
    kind: u8,
}

fn strip_prefix<'a>(fn_name: &'a str, prefix: &str) -> &'a str {
    fn_name
        .strip_prefix(&format!("{prefix}_"))
        .unwrap_or(fn_name)
}

fn scan_items(
    items: &[Item],
    module_name: &str,
    namespace_path: &str,
    prefix: &str,
    default_cap: Option<&str>,
    current_path: &mut Vec<Ident>,
    ops: &mut Vec<CollectedOp>,
    clean: &mut Vec<TS2>,
) {
    for item in items {
        match item {
            Item::Fn(f) => {
                let mut op_attr: Option<OpArgs> = None;
                let mut export_attr = false;
                let mut other_attrs: Vec<TS2> = vec![];

                for attr in &f.attrs {
                    if attr.path().is_ident("varn_op") || attr.path().is_ident("varn_fn") {
                        let args: OpArgs = attr.parse_args().unwrap_or(OpArgs {
                            name: None,
                            is_async: false,
                            cap: None,
                        });
                        op_attr = Some(args);
                    } else if attr.path().is_ident("varn_export") {
                        let args: OpArgs = attr.parse_args().unwrap_or(OpArgs {
                            name: None,
                            is_async: false,
                            cap: None,
                        });
                        op_attr = Some(args);
                        export_attr = true;
                    } else {
                        other_attrs.push(quote! { #attr });
                    }
                }

                if let Some(args) = op_attr {
                    let fn_name = f.sig.ident.to_string();
                    let suffix = args
                        .name
                        .clone()
                        .unwrap_or_else(|| strip_prefix(&fn_name, prefix).to_string());

                    let cap = args.cap.clone().or_else(|| default_cap.map(str::to_string));

                    ops.push(CollectedOp {
                        canonical: suffix,
                        namespace_path: namespace_path.to_string(),
                        fn_ident: f.sig.ident.clone(),
                        path: current_path.clone(),
                        is_async: args.is_async,
                        cap,
                        kind: if export_attr { 0x09 } else { 0x01 },
                    });

                    let sig = &f.sig;
                    let block = &f.block;
                    let vis = &f.vis;
                    clean.push(quote! {
                        #(#other_attrs)*
                        #vis #sig #block
                    });
                } else {
                    clean.push(quote! { #item });
                }
            }

            Item::Mod(sub) => {
                let mut group_attr: Option<GroupArgs> = None;
                let mut class_attr: Option<ClassArgs> = None;
                let mut other_attrs: Vec<TS2> = vec![];

                for attr in &sub.attrs {
                    if attr.path().is_ident("varn_group") || attr.path().is_ident("varn_namespace")
                    {
                        if let Ok(args) = attr.parse_args::<GroupArgs>() {
                            group_attr = Some(args);
                        }
                    } else if attr.path().is_ident("varn_class") {
                        if let Ok(args) = attr.parse_args::<ClassArgs>() {
                            class_attr = Some(args);
                        }
                    } else {
                        other_attrs.push(quote! { #attr });
                    }
                }

                if let Some(class) = class_attr {
                    if let Some((_, inner)) = &sub.content {
                        let class_name = &class.name;
                        let class_lower = class_name.to_lowercase();
                        let sub_vis = &sub.vis;
                        let sub_ident = &sub.ident;

                        let mut member_path = current_path.clone();
                        member_path.push(sub_ident.clone());

                        let (members, class_clean, dispatch_fns) =
                            scan_class_items(inner, &class_lower, &member_path);

                        let builder_ident =
                            Ident::new(&format!("__build_{}_class", class_lower), sub_ident.span());

                        let mut setup_calls: Vec<TS2> = vec![];

                        for member in &members {
                            if let ClassMember::ConstStr { symbol, value } = member {
                                setup_calls.push(quote! {
                                    cls.add_static(#symbol, ::varn_types::Value::Str(::std::rc::Rc::from(#value)));
                                });
                                continue;
                            }

                            let dispatch_ident_str = format!(
                                "__dispatch_{}_{}",
                                class_lower,
                                match member {
                                    ClassMember::Constructor { fn_ident } => fn_ident.to_string(),
                                    ClassMember::Method { fn_ident, .. } => fn_ident.to_string(),
                                    ClassMember::Getter { fn_ident, .. } => fn_ident.to_string(),
                                    ClassMember::Static { fn_ident, .. } => fn_ident.to_string(),
                                    ClassMember::StaticGetter { fn_ident, .. } =>
                                        fn_ident.to_string(),
                                    ClassMember::ConstStr { .. } => unreachable!(),
                                }
                            );
                            let dispatch_ident = Ident::new(&dispatch_ident_str, sub_ident.span());
                            let dispatch_path =
                                quote! { #(#current_path::)* #sub_ident :: #dispatch_ident };

                            match member {
                                ClassMember::Constructor { .. } => {
                                    setup_calls.push(quote! {
                                        cls.add_method("constructor", ::varn_types::Value::native(#dispatch_path, "constructor"));
                                    });
                                }
                                ClassMember::Method { symbol, .. } => {
                                    setup_calls.push(quote! {
                                        cls.add_method(#symbol, ::varn_types::Value::native(#dispatch_path, #symbol));
                                    });
                                }
                                ClassMember::Getter { symbol, .. } => {
                                    setup_calls.push(quote! {
                                        cls.add_getter(#symbol, ::varn_types::Value::native(#dispatch_path, #symbol));
                                    });
                                }
                                ClassMember::Static { symbol, .. } => {
                                    setup_calls.push(quote! {
                                        cls.add_static(#symbol, ::varn_types::Value::native(#dispatch_path, #symbol));
                                    });
                                }
                                ClassMember::StaticGetter { symbol, .. } => {
                                    setup_calls.push(quote! {
                                        cls.add_static_getter(#symbol, ::varn_types::Value::native(#dispatch_path, #symbol));
                                    });
                                }
                                ClassMember::ConstStr { .. } => unreachable!(),
                            }
                        }

                        clean.push(quote! {
                            #(#other_attrs)*
                            #sub_vis mod #sub_ident {
                                #(#class_clean)*
                                #(#dispatch_fns)*
                            }
                        });

                        let superclass_setup = if let Some(ref parent) = class.extends {
                            quote! {
                                if let Some(parent) = ctx.get_class(#parent) {
                                    *cls.superclass.borrow_mut() = Some(parent.clone());
                                    *cls.root_shape.borrow_mut() = parent.root_shape.borrow().with_class(Some(cls.clone()));
                                }
                            }
                        } else {
                            quote! {}
                        };

                        clean.push(quote! {
                            pub fn #builder_ident(
                                ctx: &mut dyn ::varn_types::NativeCtx,
                                _args: &[::varn_types::VmValue],
                            ) -> Result<::varn_types::VmValue, String> {
                                let cls = ::varn_types::value::ClassObj::new_native_rc(#class_name);
                                #superclass_setup
                                #(#setup_calls)*

                                ctx.register_class(#class_name, cls.clone());
                                Ok(ctx.alloc_class(cls))
                            }
                        });

                        ops.push(CollectedOp {
                            canonical: class_name.clone(),
                            namespace_path: namespace_path.to_string(),
                            fn_ident: builder_ident,
                            path: current_path.clone(),
                            is_async: false,
                            cap: None,
                            kind: 0x10,
                        });
                    } else {
                        clean.push(quote! { #item });
                    }
                } else if let Some(group) = group_attr {
                    if let Some((_, inner)) = &sub.content {
                        let _sub_module = format!("{}.{}", module_name, group.name);
                        let sub_prefix = &group.name;
                        let sub_cap = group.cap.as_deref().or(default_cap);
                        let sub_vis = &sub.vis;
                        let sub_ident = &sub.ident;

                        let sub_namespace = if namespace_path.is_empty() {
                            group.name.clone()
                        } else {
                            format!("{}.{}", namespace_path, group.name)
                        };
                        let mut sub_ops: Vec<CollectedOp> = vec![];
                        let mut sub_clean: Vec<TS2> = vec![];
                        current_path.push(sub_ident.clone());
                        scan_items(
                            inner,
                            module_name,
                            &sub_namespace,
                            sub_prefix,
                            sub_cap,
                            current_path,
                            &mut sub_ops,
                            &mut sub_clean,
                        );
                        current_path.pop();

                        for op in sub_ops {
                            ops.push(op);
                        }

                        clean.push(quote! {
                            #(#other_attrs)*
                            #sub_vis mod #sub_ident {
                                #(#sub_clean)*
                            }
                        });
                    } else {
                        clean.push(quote! { #item });
                    }
                } else {
                    clean.push(quote! { #item });
                }
            }

            Item::Struct(s) => {
                let mut is_resource = false;
                let mut other_attrs: Vec<TS2> = vec![];
                for attr in &s.attrs {
                    if attr.path().is_ident("varn_resource") {
                        is_resource = true;
                    } else {
                        other_attrs.push(quote! { #attr });
                    }
                }
                if is_resource {
                    clean.push(quote! {
                        #(#other_attrs)*
                        #s
                    });
                } else {
                    clean.push(quote! { #item });
                }
            }

            Item::Const(c) => {
                let mut export_name: Option<String> = None;
                let mut other_attrs: Vec<TS2> = vec![];
                for attr in &c.attrs {
                    if attr.path().is_ident("export") || attr.path().is_ident("varn_export") {
                        if let Ok(name) = attr.parse_args::<LitStr>() {
                            export_name = Some(name.value());
                        } else {
                            export_name = Some(c.ident.to_string());
                        }
                    } else {
                        other_attrs.push(quote! { #attr });
                    }
                }

                if let Some(name) = export_name {
                    let suffix = name;
                    let fn_ident = &c.ident;

                    let getter_ident = Ident::new(&format!("__get_{}", fn_ident), fn_ident.span());
                    let vis = &c.vis;
                    let ty = &c.ty;
                    let const_expr = &c.expr;

                    let ty_str = quote!(#ty).to_string();
                    let val_expr = match ty_str.as_str() {
                        "f64" | "float" => {
                            quote!(::varn_types::VmValue::from_f64(#fn_ident as f64))
                        }
                        "i64" | "int" => quote!(::varn_types::VmValue::from_int(#fn_ident as i64)),
                        "bool" => quote!(::varn_types::VmValue::from_bool(#fn_ident)),
                        "&str" | "&'static str" => {
                            quote!(::varn_types::VmValue::try_from_sso(#fn_ident).unwrap_or(::varn_types::VmValue::null()))
                        }
                        _ => quote!(::varn_types::VmValue::null()),
                    };

                    clean.push(quote! {
                        #(#other_attrs)*
                        #vis const #fn_ident: #ty = #const_expr;
                        #[allow(non_snake_case)]
                        pub fn #getter_ident(_ctx: &mut dyn ::varn_types::NativeCtx, _args: &[::varn_types::VmValue]) -> Result<::varn_types::VmValue, String> {
                            Ok(#val_expr)
                        }
                    });

                    ops.push(CollectedOp {
                        canonical: suffix,
                        namespace_path: namespace_path.to_string(),
                        fn_ident: getter_ident,
                        path: current_path.clone(),
                        is_async: false,
                        cap: None,
                        kind: 0x05,
                    });
                } else {
                    clean.push(quote! { #item });
                }
            }

            Item::Enum(e) => {
                let mut is_native_enum = false;
                for attr in &e.attrs {
                    if attr.path().is_ident("varn_native_enum") {
                        is_native_enum = true;
                    }
                }

                if is_native_enum {
                    let enum_name = e.ident.to_string();
                    for (idx, variant) in e.variants.iter().enumerate() {
                        let variant_name = variant.ident.to_string();
                        let getter_ident = Ident::new(
                            &format!("__get_enum_{}_{}", enum_name, variant_name),
                            variant.ident.span(),
                        );
                        let _tag = idx as u8;

                        clean.push(quote! {
                            pub fn #getter_ident(_ctx: &mut dyn ::varn_types::NativeCtx, _args: &[::varn_types::VmValue]) -> Result<::varn_types::VmValue, String> {
                                Ok(::varn_types::VmValue::null())
                            }
                        });

                        ops.push(CollectedOp {
                            canonical: variant_name,
                            namespace_path: if namespace_path.is_empty() {
                                enum_name.clone()
                            } else {
                                format!("{}.{}", namespace_path, enum_name)
                            },
                            fn_ident: getter_ident,
                            path: current_path.clone(),
                            is_async: false,
                            cap: None,
                            kind: 0x08,
                        });
                    }
                } else {
                    clean.push(quote! { #item });
                }
            }

            other => clean.push(quote! { #other }),
        }
    }
}

fn cap_to_mask(cap: &str) -> u64 {
    match cap {
        "fs.read" => 1 << 0,
        "fs.write" => 1 << 1,
        "net.client" => 1 << 2,
        "net.server" => 1 << 3,
        "sys.env" => 1 << 4,
        "sys.proc" => 1 << 5,
        "sys.ffi" => 1 << 6,
        "crypto.random" => 1 << 7,
        "time.now" => 1 << 8,
        _ => 0,
    }
}

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ModuleArgs);
    let mod_item = parse_macro_input!(item as ItemMod);

    let (_, items) = match mod_item.content.clone() {
        Some(c) => c,
        None => return TokenStream::from(quote! { #mod_item }),
    };

    let mod_vis = &mod_item.vis;
    let mod_ident = &mod_item.ident;
    let module_name = &args.name;
    let default_cap = args.default_cap.as_deref();

    let mut ops: Vec<CollectedOp> = vec![];
    let mut clean: Vec<TS2> = vec![];
    let mut current_path: Vec<Ident> = vec![];
    scan_items(
        &items,
        module_name,
        "",
        module_name,
        default_cap,
        &mut current_path,
        &mut ops,
        &mut clean,
    );

    let mut entries = vec![];
    for op in &ops {
        let op_name = &op.canonical;
        let ns_path = &op.namespace_path;
        let fn_ident = &op.fn_ident;
        let is_async = op.is_async;
        let op_kind = op.kind;
        let cap_mask = op.cap.as_deref().map(cap_to_mask).unwrap_or(0);
        let flags: u32 = if is_async { 0x01 } else { 0x00 };

        let entry_ident = Ident::new(
            &format!("__varn_OP_{}", fn_ident).to_uppercase(),
            fn_ident.span(),
        );

        let full_path = &op.path;

        entries.push(quote! {
            #[used]
            #[cfg_attr(target_os = "windows", link_section = ".varn_ops$B")]
            #[cfg_attr(target_os = "macos", link_section = "__DATA,varn_ops")]
            #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), link_section = "varn_ops")]
            static #entry_ident: ::varn_types::NativeOpEntry = ::varn_types::NativeOpEntry {
                module_id: #module_name.as_ptr(),
                module_id_len: #module_name.len() as u32,
                namespace_path: #ns_path.as_ptr(),
                namespace_path_len: #ns_path.len() as u32,
                symbol_name: #op_name.as_ptr(),
                symbol_name_len: #op_name.len() as u32,
                func_ptr: #(#full_path::)* #fn_ident as *const u8,
                capability_mask: #cap_mask,
                entry_kind: #op_kind,
                flags: #flags,
                _reserved: [0; 7],
            };
        });
    }

    TokenStream::from(quote! {
        #mod_vis mod #mod_ident {
            #(#clean)*
            #(#entries)*


            pub static META: ::varn_core::op_meta::ModuleMeta = ::varn_core::op_meta::ModuleMeta {
                module: #module_name,
                ops: &[],
            };
        }
    })
}
