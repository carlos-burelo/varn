use super::stats::RowResult;
use super::term::Term;

pub(super) fn get_verdict_badge(r: &RowResult, term: &Term) -> String {
    if !r.output_ok {
        return term.bold_red("❌ OUTPUT MISMATCH");
    }
    let Some(ratio) = r.work_ratio else {
        return term.gray("--");
    };
    if !r.resolved {
        return term.cyan("🤝 ~tied (ranges overlap)");
    }
    if ratio >= 1.05 {
        term.bold_green(&format!("🏆 {:.2}x faster", ratio))
    } else if ratio <= 0.95 {
        term.yellow(&format!("🔻 {:.2}x slower", 1.0 / ratio))
    } else {
        term.cyan("🤝 ~tied (parity)")
    }
}

pub(super) fn get_verdict_badge_len(r: &RowResult) -> usize {
    if !r.output_ok {
        return 17;
    }
    let Some(ratio) = r.work_ratio else {
        return 2;
    };
    if !r.resolved {
        return 24;
    }
    if ratio >= 1.05 || ratio <= 0.95 {
        15
    } else {
        17
    }
}

pub(super) fn get_verdict_text(r: &RowResult) -> String {
    if !r.output_ok {
        return "OUTPUT MISMATCH".to_string();
    }
    let Some(ratio) = r.work_ratio else {
        return "--".to_string();
    };
    if !r.resolved {
        return "not resolved".to_string();
    }
    if ratio >= 1.0 {
        format!("{:.2}x faster", ratio)
    } else {
        format!("{:.2}x slower", 1.0 / ratio)
    }
}
