use crate::ssa::ir::SsaFunc;

pub(crate) fn dominators(func: &SsaFunc) -> Vec<Vec<u64>> {
    let n = func.blocks.len();
    let words = n.div_ceil(64);
    let full = vec![u64::MAX; words];
    let mut dom = vec![full; n];

    let entry = func.entry.0 as usize;
    dom[entry] = vec![0; words];
    set_bit(&mut dom[entry], entry);

    let mut changed = true;
    while changed {
        changed = false;
        for b in 0..n {
            if b == entry {
                continue;
            }
            let preds = &func.blocks[b].preds;
            if preds.is_empty() {
                continue;
            }
            let mut new = vec![u64::MAX; words];
            for p in preds {
                for (w, pw) in new.iter_mut().zip(&dom[p.0 as usize]) {
                    *w &= pw;
                }
            }
            set_bit(&mut new, b);
            if new != dom[b] {
                dom[b] = new;
                changed = true;
            }
        }
    }
    dom
}

#[inline]
fn set_bit(bits: &mut [u64], i: usize) {
    bits[i / 64] |= 1 << (i % 64);
}

#[inline]
pub(crate) fn dominates(dom: &[Vec<u64>], a: usize, b: usize) -> bool {
    (dom[b][a / 64] >> (a % 64)) & 1 == 1
}
