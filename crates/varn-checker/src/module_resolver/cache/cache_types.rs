use crate::binder::BindResult;
use crate::types::TySlice;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct PortableModule {
    pub(super) exports: super::super::ExportMap,
    pub(super) bind: BindResult,
    pub(super) names: Vec<Box<str>>,
    pub(super) types: TySlice,
}
