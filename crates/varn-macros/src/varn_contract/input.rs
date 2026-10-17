use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

pub(super) struct ContractInput {
    pub(super) module: String,
    pub(super) contract: String,
    pub(super) sections: Vec<Section>,
}

pub(super) struct Section {
    pub(super) class: Option<String>,
    pub(super) self_ty: syn::Type,
    pub(super) fns: Vec<syn::ImplItemFn>,
}

impl Parse for ContractInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut module = None;
        let mut contract = None;
        let mut sections = Vec::new();
        while !input.is_empty() {
            let mut class = None;
            while !input.peek(Token![impl]) {
                let key: Ident = input.parse()?;
                input.parse::<Token![:]>()?;
                let val: LitStr = input.parse()?;
                let slot = match key.to_string().as_str() {
                    "module" => &mut module,
                    "contract" => &mut contract,
                    "class" => &mut class,
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!("unknown varn_contract! key `{other}`"),
                        ))
                    }
                };
                if slot.replace(val.value()).is_some() {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("duplicate `{key}:` key"),
                    ));
                }
                let _ = input.parse::<Token![,]>();
            }
            let imp: syn::ItemImpl = input.parse()?;
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
            sections.push(Section {
                class,
                self_ty: (*imp.self_ty).clone(),
                fns,
            });
        }
        let span = input.span();
        Ok(Self {
            module: module.ok_or_else(|| syn::Error::new(span, "missing `module:` key"))?,
            contract: contract.ok_or_else(|| syn::Error::new(span, "missing `contract:` key"))?,
            sections,
        })
    }
}
