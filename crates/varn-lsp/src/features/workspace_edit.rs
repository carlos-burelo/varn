use tower_lsp_f::lsp_types::{
    DocumentChange, Edit, OptionalVersionedTextDocumentIdentifier, TextDocumentEdit,
    TextDocumentIdentifier, TextEdit, Uri, WorkspaceEdit,
};

pub fn doc_edits(pairs: Vec<(Uri, Vec<TextEdit>)>) -> WorkspaceEdit {
    let mut pairs = pairs;
    pairs.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    WorkspaceEdit {
        changes: None,
        document_changes: Some(
            pairs
                .into_iter()
                .map(|(uri, edits)| {
                    DocumentChange::TextDocumentEdit(TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            version: None,
                            text_document_identifier: TextDocumentIdentifier { uri },
                        },
                        edits: edits.into_iter().map(Edit::TextEdit).collect(),
                    })
                })
                .collect(),
        ),
        change_annotations: None,
    }
}
