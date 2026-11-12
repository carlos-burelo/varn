use crate::bind::BindResult;
use crate::types::TySlice;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct PortableModule {
    pub(crate) exports: crate::exports::ExportMap,
    pub(crate) bind: BindResult,
    pub(crate) names: Vec<Box<str>>,
    pub(crate) types: TySlice,
}
