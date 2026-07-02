use varn_types::Value;

pub(crate) type RouteEntry = (String, String, Value);

pub(crate) fn split_path_query(url: &str) -> (String, String) {
    match url.find('?') {
        Some(i) => (url[..i].to_owned(), url[i + 1..].to_owned()),
        None => (url.to_owned(), String::new()),
    }
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
