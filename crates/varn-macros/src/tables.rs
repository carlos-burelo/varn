use std::path::Path;

use crate::contract_members::{Kind, Member, ParamInfo};
use crate::varn_contract::Mapped;

pub(crate) enum LookupFailure {
    TablesUnreadable(String),
    ContractUnknown,
    ClassNotFound,
    NoFunctions,
    BadEntry(String),
}

fn kind_from_code(code: &str) -> Option<Kind> {
    match code {
        "method" => Some(Kind::Method),
        "getter" => Some(Kind::Getter),
        "static_method" => Some(Kind::StaticMethod),
        "static_getter" => Some(Kind::StaticGetter),
        "constructor" => Some(Kind::Constructor),
        "property" => Some(Kind::Property),
        "function" => Some(Kind::Function),
        _ => None,
    }
}

fn member_from_json(v: &serde_json::Value) -> Result<Member, String> {
    let symbol = v
        .get("symbol")
        .and_then(|s| s.as_str())
        .ok_or_else(|| "member without symbol".to_string())?;
    let kind = v
        .get("kind")
        .and_then(|s| s.as_str())
        .and_then(kind_from_code)
        .ok_or_else(|| format!("unknown member kind for `{symbol}`"))?;
    let mut params = Vec::new();
    for p in v
        .get("params")
        .and_then(|p| p.as_array())
        .ok_or_else(|| format!("member `{symbol}` without params"))?
    {
        let ty = p
            .get("ty")
            .and_then(|t| t.as_str())
            .and_then(Mapped::from_code)
            .ok_or_else(|| format!("unknown type code in `{symbol}`"))?;
        let rest = p.get("rest").and_then(|r| r.as_bool()).unwrap_or(false);
        params.push(ParamInfo {
            mapped: ty,
            is_rest: rest,
        });
    }
    let ret = v
        .get("ret")
        .and_then(|t| t.as_str())
        .and_then(Mapped::from_code)
        .ok_or_else(|| format!("unknown return type code in `{symbol}`"))?;
    let fallible = v.get("fallible").and_then(|f| f.as_bool()).unwrap_or(false);
    Ok(Member {
        symbol: symbol.to_string(),
        kind,
        params,
        ret,
        fallible,
    })
}

fn members_from_json(v: &serde_json::Value) -> Result<Vec<Member>, String> {
    v.as_array()
        .ok_or_else(|| "expected member list".to_string())?
        .iter()
        .map(member_from_json)
        .collect()
}

pub(crate) fn lookup(
    contract_key: &str,
    manifest_dir: &str,
    class: Option<&str>,
) -> Result<Vec<Member>, LookupFailure> {
    let raw =
        std::fs::read_to_string(Path::new(manifest_dir).join("contracts.json")).map_err(|e| {
            LookupFailure::TablesUnreadable(format!(
                "cannot read contracts.json: {e}; run `vn gen-contract-tables`"
            ))
        })?;
    let tables: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        LookupFailure::TablesUnreadable(format!(
            "contracts.json corrupt: {e}; run `vn gen-contract-tables`"
        ))
    })?;
    let entry = tables
        .get("contracts")
        .and_then(|c| c.get(contract_key))
        .ok_or(LookupFailure::ContractUnknown)?;
    match class {
        Some(name) => {
            let found = entry
                .get("classes")
                .and_then(|c| c.get(name))
                .ok_or(LookupFailure::ClassNotFound)?;
            members_from_json(found).map_err(LookupFailure::BadEntry)
        }
        None => {
            let found = entry
                .get("functions")
                .ok_or(LookupFailure::ContractUnknown)?;
            let members = members_from_json(found).map_err(LookupFailure::BadEntry)?;
            if members.is_empty() {
                return Err(LookupFailure::NoFunctions);
            }
            Ok(members)
        }
    }
}
