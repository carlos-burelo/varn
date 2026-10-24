use varn_core::term::terminal;
use varn_debug::colors::{GREEN, R, RED};
use varn_debug::flags::{Cmp, Predicate, Verb};

use super::queries::Outcome;

pub fn eval(outcomes: &[(Verb, Outcome)], expects: &[Predicate]) -> bool {
    if expects.is_empty() {
        return true;
    }
    let complete = outcomes.iter().find_map(|(v, o)| match (v, o) {
        (Verb::Complete, Outcome::Items { branch, .. }) => Some((*branch, o.labels())),
        _ => None,
    });
    let singles: Vec<bool> = outcomes
        .iter()
        .filter_map(|(_, o)| match o {
            Outcome::Single { found } => Some(*found),
            Outcome::Items { .. } => None,
        })
        .collect();
    let mut pass = true;
    let mut why = String::new();
    for pred in expects {
        let ok = match pred {
            Predicate::Branch(want) => match complete {
                Some((branch, _)) => branch == want,
                None => {
                    why = "branch needs complete".to_string();
                    false
                }
            },
            Predicate::Count(cmp, n) => match complete {
                Some((_, labels)) => match cmp {
                    Cmp::Ge => labels.len() >= *n,
                    Cmp::Eq => labels.len() == *n,
                },
                None => {
                    why = "count needs complete".to_string();
                    false
                }
            },
            Predicate::Has(label) => match complete {
                Some((_, labels)) => labels.iter().any(|l| l == label),
                None => {
                    why = "has needs complete".to_string();
                    false
                }
            },
            Predicate::Missing(label) => match complete {
                Some((_, labels)) => !labels.iter().any(|l| l == label),
                None => {
                    why = "missing needs complete".to_string();
                    false
                }
            },
            Predicate::Found => {
                if singles.is_empty() {
                    why = "found needs a single query (hover, definition, declaration, highlight, signature)".to_string();
                    false
                } else {
                    singles.iter().all(|f| *f)
                }
            }
            Predicate::None_ => {
                if singles.is_empty() {
                    match complete {
                        Some((_, labels)) => labels.is_empty(),
                        None => {
                            why = "none needs a query".to_string();
                            false
                        }
                    }
                } else {
                    singles.iter().all(|f| !f)
                }
            }
        };
        if !ok {
            pass = false;
            if why.is_empty() {
                why = format!("failed {}", pred.describe());
            }
            break;
        }
    }
    let summary = expects
        .iter()
        .map(|p| p.describe())
        .collect::<Vec<_>>()
        .join(" ");
    let line = format!(
        "    expect {summary} → {}",
        if pass {
            format!("{GREEN}ok{RESET}", GREEN = GREEN, RESET = R)
        } else {
            format!("{RED}FAIL{RESET} ({why})", RED = RED, RESET = R)
        }
    );
    terminal::log(line);
    pass
}
