use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

pub(super) struct ContractInput {
    pub(super) module: String,

    pub(super) class: Option<String>,

    pub(super) extends: Option<String>,
    pub(super) contract: String,
    pub(super) self_ty: syn::Type,
    pub(super) fns: Vec<syn::ImplItemFn>,
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
            .filter_map(|it| {
                if let syn::ImplItem::Fn(f) = it {
                    Some(f)
                } else {
                    None
                }
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
