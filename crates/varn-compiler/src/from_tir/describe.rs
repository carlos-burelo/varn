use crate::OptError;
type Result<T> = std::result::Result<T, OptError>;
use std::sync::Arc;
use varn_core::type_desc::FieldDesc;
use varn_core::TypeDesc;
use varn_tir::{BackendTy, DynReason, ShapeId, TirFunction, TirModule, TyId};
use varn_types::chunk::{ContractWitness, ParamContract};

pub(crate) struct Describer<'m> {
    tir: &'m TirModule,
    pub witnesses: Vec<Arc<str>>,
    open: Vec<ShapeId>,
}

impl<'m> Describer<'m> {
    pub(crate) fn new(tir: &'m TirModule) -> Self {
        Describer {
            tir,
            witnesses: Vec::new(),
            open: Vec::new(),
        }
    }

    pub(crate) fn describe(&mut self, ty: BackendTy) -> Result<TypeDesc> {
        self.at(ty)
    }

    fn of(&mut self, id: TyId) -> Result<TypeDesc> {
        if self.tir.types.contains(id) {
            self.at(self.tir.types.get(id))
        } else {
            Ok(TypeDesc::Any)
        }
    }

    fn witness(&mut self, name: &Arc<str>) -> u32 {
        match self.witnesses.iter().position(|w| w == name) {
            Some(at) => at as u32,
            None => {
                self.witnesses.push(name.clone());
                self.witnesses.len() as u32 - 1
            }
        }
    }

    fn at(&mut self, ty: BackendTy) -> Result<TypeDesc> {
        Ok(match ty {
            BackendTy::Literal(value) => TypeDesc::Literal(match value {
                varn_core::TypeLiteral::Int(n) => varn_core::TypeLiteral::Int(n),
                varn_core::TypeLiteral::Bool(b) => varn_core::TypeLiteral::Bool(b),
                varn_core::TypeLiteral::Char(c) => varn_core::TypeLiteral::Char(c),
                varn_core::TypeLiteral::Str(id) => varn_core::TypeLiteral::Str(
                    self.tir
                        .types
                        .literal_str(id)
                        .ok_or_else(|| {
                            OptError::Internal(format!("from_tir: missing literal string {}", id.0))
                        })?
                        .to_owned(),
                ),
            }),
            BackendTy::Int => TypeDesc::Int,
            BackendTy::Float => TypeDesc::Float,
            BackendTy::Bool => TypeDesc::Bool,
            BackendTy::Char => TypeDesc::Char,
            BackendTy::Str => TypeDesc::Str,
            BackendTy::Bytes => TypeDesc::Bytes,
            BackendTy::Decimal => TypeDesc::Decimal,
            BackendTy::BigInt => TypeDesc::BigInt,
            BackendTy::Array(el) => TypeDesc::Array(Box::new(self.of(el)?)),
            BackendTy::Dict(v) => TypeDesc::Dict(Box::new(self.of(v)?)),
            BackendTy::Map(k, v) => {
                let key = self.of(k)?;
                let val = self.of(v)?;
                TypeDesc::Map(Box::new(key), Box::new(val))
            }
            BackendTy::Set(el) => TypeDesc::Set(Box::new(self.of(el)?)),
            BackendTy::Tuple(list) => {
                if !self.tir.types.contains_list(list) {
                    return Ok(TypeDesc::Any);
                }
                let els = self.tir.types.get_list(list).to_vec();
                TypeDesc::Tuple(
                    els.into_iter()
                        .map(|el| self.at(el))
                        .collect::<Result<_>>()?,
                )
            }
            BackendTy::Class(class) => match self.tir.class(class) {
                Some(info) => {
                    let name = info.name.clone();
                    TypeDesc::Class {
                        witness: self.witness(&name),
                        name: name.to_string(),
                    }
                }
                None => TypeDesc::Any,
            },
            BackendTy::Enum(id) => match self.tir.enum_info(id) {
                Some(info) => {
                    let name = info.name.clone();
                    TypeDesc::Enum {
                        witness: self.witness(&name),
                        name: name.to_string(),
                    }
                }
                None => TypeDesc::Any,
            },
            BackendTy::Fn(_) | BackendTy::Dynamic(DynReason::Callable) => TypeDesc::Callable,
            BackendTy::Nullable(inner) => {
                let inner = self.of(inner)?;
                if inner.is_any() {
                    TypeDesc::Any
                } else {
                    TypeDesc::Nullable(Box::new(inner))
                }
            }
            BackendTy::Dynamic(
                DynReason::Shape(id) | DynReason::Interface(id) | DynReason::Record(id),
            ) => {
                if !self.tir.types.contains_shape(id) {
                    return Ok(TypeDesc::Any);
                }
                if let Some(at) = self.open.iter().rposition(|open| *open == id) {
                    let name = match self.tir.types.shape_name(id) {
                        Some(name) => name.to_string(),
                        None => return Ok(TypeDesc::Any),
                    };
                    return Ok(TypeDesc::Enclosing {
                        up: (self.open.len() - 1 - at) as u32,
                        name,
                    });
                }
                self.open.push(id);
                let shape = self.tir.types.get_shape(id).to_vec();
                let fields = shape
                    .into_iter()
                    .map(|field| {
                        Ok(FieldDesc {
                            name: field.name.to_string(),
                            ty: self.at(field.ty)?,
                            optional: field.optional,
                            readonly: field.readonly,
                        })
                    })
                    .collect::<Result<_>>()?;
                self.open.pop();
                if matches!(ty, BackendTy::Dynamic(DynReason::Record(_))) {
                    TypeDesc::Record(fields)
                } else {
                    TypeDesc::Object(fields)
                }
            }
            BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(
                DynReason::HostBoundary
                | DynReason::Union
                | DynReason::IndexSignature
                | DynReason::Declared
                | DynReason::Awaitable(_)
                | DynReason::Yields(_)
                | DynReason::Range(_)
                | DynReason::TaskHandle(_)
                | DynReason::CoreMember
                | DynReason::Spread
                | DynReason::Unrecorded
                | DynReason::Generic
                | DynReason::Boxed
                | DynReason::Runtime,
            ) => TypeDesc::Any,
        })
    }
}

fn global_name(tir: &TirModule, bare: &str) -> ContractWitness {
    match tir.global_names.iter().position(|n| n.as_ref() == bare) {
        Some(slot) => ContractWitness::Slot(slot as u32),
        None => ContractWitness::Name(format!("{}::{}", tir.source_file.replace('\\', "/"), bare)),
    }
}

pub(crate) fn param_contract(tir: &TirModule, f: &TirFunction) -> Result<Option<ParamContract>> {
    let defaulted = super::build::defaulted_param_mask(&f.body, f.params.len());
    let mut describer = Describer::new(tir);
    let mut fixed: Vec<TypeDesc> = f
        .params
        .iter()
        .zip(&defaulted)
        .map(|(ty, defaulted)| {
            let desc = describer.describe(*ty)?;
            Ok(match desc {
                TypeDesc::Any | TypeDesc::Nullable(_) => desc,
                required @ TypeDesc::Literal(_)
                | required @ TypeDesc::Int
                | required @ TypeDesc::Float
                | required @ TypeDesc::Bool
                | required @ TypeDesc::Char
                | required @ TypeDesc::Str
                | required @ TypeDesc::Bytes
                | required @ TypeDesc::Decimal
                | required @ TypeDesc::BigInt
                | required @ TypeDesc::Callable
                | required @ TypeDesc::Array(_)
                | required @ TypeDesc::Map(_, _)
                | required @ TypeDesc::Set(_)
                | required @ TypeDesc::Dict(_)
                | required @ TypeDesc::Tuple(_)
                | required @ TypeDesc::Class { .. }
                | required @ TypeDesc::Enum { .. }
                | required @ TypeDesc::Args { .. }
                | required @ TypeDesc::Object(_)
                | required @ TypeDesc::Record(_)
                | required @ TypeDesc::Enclosing { .. }
                    if *defaulted =>
                {
                    TypeDesc::Nullable(Box::new(required))
                }
                required @ TypeDesc::Literal(_)
                | required @ TypeDesc::Int
                | required @ TypeDesc::Float
                | required @ TypeDesc::Bool
                | required @ TypeDesc::Char
                | required @ TypeDesc::Str
                | required @ TypeDesc::Bytes
                | required @ TypeDesc::Decimal
                | required @ TypeDesc::BigInt
                | required @ TypeDesc::Callable
                | required @ TypeDesc::Array(_)
                | required @ TypeDesc::Map(_, _)
                | required @ TypeDesc::Set(_)
                | required @ TypeDesc::Dict(_)
                | required @ TypeDesc::Tuple(_)
                | required @ TypeDesc::Class { .. }
                | required @ TypeDesc::Enum { .. }
                | required @ TypeDesc::Args { .. }
                | required @ TypeDesc::Object(_)
                | required @ TypeDesc::Record(_)
                | required @ TypeDesc::Enclosing { .. } => required,
            })
        })
        .collect::<Result<_>>()?;
    let rest = match (f.has_rest, f.params.last()) {
        (true, Some(BackendTy::Array(el))) => {
            fixed.pop();
            Some(describer.of(*el)?)
        }
        (true, Some(_)) => {
            fixed.pop();
            None
        }
        (true, None) | (false, _) => None,
    };
    if fixed.iter().all(TypeDesc::is_any) && rest.as_ref().is_none_or(TypeDesc::is_any) {
        return Ok(None);
    }
    let desc = TypeDesc::Args {
        fixed,
        rest: rest.map(Box::new),
    };
    Ok(Some(ParamContract {
        desc: desc.encode(),
        witnesses: describer
            .witnesses
            .iter()
            .map(|name| global_name(tir, name))
            .collect(),
    }))
}
