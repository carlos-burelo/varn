use std::sync::Arc;

pub(super) fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut row: Vec<usize> = (0..=m).collect();
    for i in 1..=n {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=m {
            let next = row[j];
            row[j] = if a[i - 1] == b[j - 1] {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j - 1])
            };
            prev = next;
        }
    }
    row[m]
}

pub(super) fn closest_in_list<'a>(name: &str, candidates: &'a [Arc<str>]) -> Option<&'a str> {
    let threshold = (name.len().max(1) / 3).max(1);
    candidates
        .iter()
        .filter_map(|c| {
            let d = levenshtein(name, c.as_ref());
            if d <= threshold {
                Some((d, c.as_ref()))
            } else {
                None
            }
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}
