use crate::constants::{SEVERITY_ERROR, SEVERITY_HINT, SEVERITY_WARNING};
use crate::document::DocumentState;
use crate::util::converters::range_on_line;
use tower_lsp::lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag,
    Location, Position, Range, Url,
};

pub fn convert_diagnostics(state: &DocumentState) -> Vec<LspDiagnostic> {
    state
        .diagnostics
        .iter()
        .map(|d| {
            let severity = match d.severity {
                s if s == SEVERITY_ERROR => DiagnosticSeverity::ERROR,
                s if s == SEVERITY_WARNING => DiagnosticSeverity::WARNING,
                s if s == SEVERITY_HINT => DiagnosticSeverity::HINT,
                _ => DiagnosticSeverity::INFORMATION,
            };

            let related_information = if d.related.is_empty() {
                None
            } else {
                let items: Vec<DiagnosticRelatedInformation> = d
                    .related
                    .iter()
                    .filter_map(|r| {
                        let url = Url::parse(&r.uri).ok()?;
                        let pos = Position {
                            line: r.line,
                            character: r.col,
                        };
                        Some(DiagnosticRelatedInformation {
                            location: Location::new(
                                url,
                                Range {
                                    start: pos,
                                    end: pos,
                                },
                            ),
                            message: r.message.clone(),
                        })
                    })
                    .collect();
                if items.is_empty() {
                    None
                } else {
                    Some(items)
                }
            };

            let data = if d.suggestions.is_empty() {
                None
            } else {
                let json_suggestions: Vec<serde_json::Value> = d
                    .suggestions
                    .iter()
                    .map(|s| {
                        let mut obj = serde_json::Map::new();
                        obj.insert(
                            "message".to_string(),
                            serde_json::Value::String(s.message.clone()),
                        );
                        if let Some(r) = &s.replacement {
                            obj.insert(
                                "replacement".to_string(),
                                serde_json::Value::String(r.clone()),
                            );
                        }
                        if let Some(range) = &s.range {
                            obj.insert("range".to_string(), serde_json::json!({
                            "start": { "line": range.start.line, "character": range.start.column },
                            "end": { "line": range.end.line, "character": range.end.column }
                        }));
                        }
                        serde_json::Value::Object(obj)
                    })
                    .collect();
                Some(serde_json::json!({ "suggestions": json_suggestions }))
            };

            let mut tags = Vec::new();
            let lower_msg = d.message.to_lowercase();
            if lower_msg.contains("unused") || lower_msg.contains("never read") || lower_msg.contains("never used") {
                tags.push(DiagnosticTag::UNNECESSARY);
            }
            if lower_msg.contains("deprecated") {
                tags.push(DiagnosticTag::DEPRECATED);
            }
            let tags = if tags.is_empty() { None } else { Some(tags) };
            let code = d
                .code
                .map(|c| tower_lsp::lsp_types::NumberOrString::String(c.to_string()));

            LspDiagnostic {
                range: range_on_line(d.line, d.col, d.end_col),
                severity: Some(severity),
                code,
                code_description: None,
                message: d.message.clone(),
                source: Some("varn".into()),
                tags,
                related_information,
                data,
            }
        })
        .collect()
}
