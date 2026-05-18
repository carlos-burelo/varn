#![allow(dead_code)]
use std::rc::Rc;

use varn_types::{
    value::{new_object, nv_to_value, ObjData},
    Value,
};

pub(crate) type RouteEntry = (String, String, Value);

pub(crate) fn extract_obj_pairs(v: Option<&Value>) -> Vec<(String, String)> {
    match v {
        Some(Value::Object(o)) => o
            .borrow()
            .inner
            .iter()
            .filter(|(_, nv)| !nv.is_null())
            .filter_map(|(k, nv)| match nv_to_value(nv) {
                Value::Str(s) => Some((k.to_string(), s.to_string())),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

pub(crate) fn split_path_query(url: &str) -> (String, String) {
    match url.find('?') {
        Some(i) => (url[..i].to_owned(), url[i + 1..].to_owned()),
        None => (url.to_owned(), String::new()),
    }
}

pub(crate) fn parse_query_string(qs: &str) -> Value {
    let mut obj = ObjData::new();
    for pair in qs.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        let key = urlencoding::decode(k)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| k.to_owned());
        let val = urlencoding::decode(v)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| v.to_owned());
        obj.set_field(Rc::from(key.as_str()), Value::Str(Rc::from(val.as_str())));
    }
    new_object(obj)
}

fn match_pattern(pattern: &str, path: &str) -> Option<Vec<(String, String)>> {
    let pp: Vec<&str> = pattern.split('/').collect();
    let ps: Vec<&str> = path.split('/').collect();
    if pp.len() != ps.len() {
        return None;
    }
    let mut params = Vec::new();
    for (seg, val) in pp.iter().zip(ps.iter()) {
        if let Some(name) = seg.strip_prefix(':') {
            params.push((name.to_owned(), (*val).to_owned()));
        } else if seg != val {
            return None;
        }
    }
    Some(params)
}

pub(crate) fn find_route(
    routes: &[RouteEntry],
    method: &str,
    path: &str,
) -> Option<(String, String, Value)> {
    for (m, pattern, cb) in routes {
        if m == method && match_pattern(pattern, path).is_some() {
            return Some((m.clone(), pattern.clone(), cb.clone()));
        }
    }
    None
}

pub(crate) fn extract_params(pattern: &str, path: &str) -> Value {
    let mut obj = ObjData::new();
    if let Some(pairs) = match_pattern(pattern, path) {
        for (k, v) in pairs {
            obj.set_field(Rc::from(k.as_str()), Value::Str(Rc::from(v.as_str())));
        }
    }
    new_object(obj)
}

pub(crate) fn build_headers_obj(headers: &[tiny_http::Header]) -> Value {
    let mut obj = ObjData::new();
    for h in headers {
        let name = h.field.to_string().to_lowercase();
        let val = h.value.to_string();
        obj.set_field(Rc::from(name.as_str()), Value::Str(Rc::from(val.as_str())));
    }
    new_object(obj)
}

pub(crate) fn make_request_obj(
    method: &str,
    path: &str,
    query: Value,
    params: Value,
    body: &str,
    headers: Value,
) -> Value {
    let mut obj = ObjData::new();
    obj.set_field(Rc::from("method"), Value::Str(Rc::from(method)));
    obj.set_field(Rc::from("path"), Value::Str(Rc::from(path)));
    obj.set_field(Rc::from("query"), query);
    obj.set_field(Rc::from("params"), params);
    obj.set_field(Rc::from("body"), Value::Str(Rc::from(body)));
    obj.set_field(Rc::from("headers"), headers);
    new_object(obj)
}
