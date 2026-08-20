use crate::document::{MemberKind, MemberRecord};

pub fn resolve_builtin_member(
    receiver_ty: &str,
    member_name: &str,
) -> Option<MemberRecord> {
    let clean_receiver = receiver_ty.trim().trim_end_matches(['.', ')', '(']);
    let base_type = clean_receiver
        .split('<')
        .next()
        .unwrap_or(clean_receiver)
        .trim();

    // Extract generic argument if present (e.g. Map<int> -> int, Array<str> -> str)
    let inner_ty = if let Some(start) = clean_receiver.find('<') {
        if let Some(end) = clean_receiver.rfind('>') {
            &clean_receiver[start + 1..end]
        } else {
            "dynamic"
        }
    } else if clean_receiver.ends_with("[]") {
        &clean_receiver[..clean_receiver.len() - 2]
    } else {
        "dynamic"
    };

    match base_type {
        "Map" => match member_name {
            "set" => Some(make_method("set", &format!("key: str, value: {inner_ty}"), "void")),
            "get" => Some(make_method("get", "key: str", &format!("{inner_ty}?"))),
            "has" => Some(make_method("has", "key: str", "bool")),
            "delete" => Some(make_method("delete", "key: str", "bool")),
            "clear" => Some(make_method("clear", "", "void")),
            "keys" => Some(make_method("keys", "", "str[]")),
            "values" => Some(make_method("values", "", &format!("{inner_ty}[]"))),
            "entries" => Some(make_method("entries", "", &format!("[str, {inner_ty}][]"))),
            "size" => Some(make_prop("size", "int")),
            _ => None,
        },
        "Set" => match member_name {
            "add" => Some(make_method("add", &format!("value: {inner_ty}"), "void")),
            "has" => Some(make_method("has", &format!("value: {inner_ty}"), "bool")),
            "delete" => Some(make_method("delete", &format!("value: {inner_ty}"), "bool")),
            "clear" => Some(make_method("clear", "", "void")),
            "values" => Some(make_method("values", "", &format!("{inner_ty}[]"))),
            "size" => Some(make_prop("size", "int")),
            _ => None,
        },
        "Range" => match member_name {
            "from" => Some(make_static_method("from", "start: int, end: int", "Range")),
            "toArray" => Some(make_method("toArray", "", "int[]")),
            "step" => Some(make_method("step", "step: int", "Range")),
            "contains" => Some(make_method("contains", "value: int", "bool")),
            "length" => Some(make_prop("length", "int")),
            "start" => Some(make_prop("start", "int")),
            "end" => Some(make_prop("end", "int")),
            _ => None,
        },
        "Array" | _ if clean_receiver.ends_with("[]") || base_type == "Array" => match member_name {
            "length" => Some(make_prop("length", "int")),
            "push" => Some(make_method("push", &format!("item: {inner_ty}"), "int")),
            "pop" => Some(make_method("pop", "", &format!("{inner_ty}?"))),
            "shift" => Some(make_method("shift", "", &format!("{inner_ty}?"))),
            "unshift" => Some(make_method("unshift", &format!("item: {inner_ty}"), "int")),
            "slice" => Some(make_method("slice", "start?: int, end?: int", &format!("{inner_ty}[]"))),
            "join" => Some(make_method("join", "separator?: str", "str")),
            "indexOf" => Some(make_method("indexOf", &format!("item: {inner_ty}"), "int")),
            "includes" => Some(make_method("includes", &format!("item: {inner_ty}"), "bool")),
            "reverse" => Some(make_method("reverse", "", &format!("{inner_ty}[]"))),
            "concat" => Some(make_method("concat", &format!("other: {inner_ty}[]"), &format!("{inner_ty}[]"))),
            _ => None,
        },
        "str" => match member_name {
            "length" => Some(make_prop("length", "int")),
            "split" => Some(make_method("split", "separator: str", "str[]")),
            "trim" => Some(make_method("trim", "", "str")),
            "trimStart" => Some(make_method("trimStart", "", "str")),
            "trimEnd" => Some(make_method("trimEnd", "", "str")),
            "indexOf" => Some(make_method("indexOf", "substring: str", "int")),
            "includes" => Some(make_method("includes", "substring: str", "bool")),
            "startsWith" => Some(make_method("startsWith", "prefix: str", "bool")),
            "endsWith" => Some(make_method("endsWith", "suffix: str", "bool")),
            "toUpperCase" => Some(make_method("toUpperCase", "", "str")),
            "toLowerCase" => Some(make_method("toLowerCase", "", "str")),
            "replace" => Some(make_method("replace", "from: str, to: str", "str")),
            "substring" => Some(make_method("substring", "start: int, end?: int", "str")),
            "charAt" => Some(make_method("charAt", "index: int", "char")),
            "charCodeAt" => Some(make_method("charCodeAt", "index: int", "int")),
            _ => None,
        },
        "Task" => match member_name {
            "await" => Some(make_method("await", "", inner_ty)),
            "isCompleted" => Some(make_prop("isCompleted", "bool")),
            "cancel" => Some(make_method("cancel", "", "void")),
            _ => None,
        },
        _ => None,
    }
}

fn make_method(name: &str, params: &str, ret: &str) -> MemberRecord {
    MemberRecord {
        name: name.to_string(),
        type_str: ret.to_string(),
        params_str: params.to_string(),
        is_static: false,
        is_optional: false,
        kind: MemberKind::Method,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        line: 0,
        col: 0,
        init_value: String::new(),
        ty: varn_checker::Type::Dynamic,
        symbol_id: None,
        members: Vec::new(),
    }
}

fn make_static_method(name: &str, params: &str, ret: &str) -> MemberRecord {
    MemberRecord {
        name: name.to_string(),
        type_str: ret.to_string(),
        params_str: params.to_string(),
        is_static: true,
        is_optional: false,
        kind: MemberKind::Method,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        line: 0,
        col: 0,
        init_value: String::new(),
        ty: varn_checker::Type::Dynamic,
        symbol_id: None,
        members: Vec::new(),
    }
}

fn make_prop(name: &str, ty: &str) -> MemberRecord {
    MemberRecord {
        name: name.to_string(),
        type_str: ty.to_string(),
        params_str: String::new(),
        is_static: false,
        is_optional: false,
        kind: MemberKind::Property,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        line: 0,
        col: 0,
        init_value: String::new(),
        ty: varn_checker::Type::Dynamic,
        symbol_id: None,
        members: Vec::new(),
    }
}
