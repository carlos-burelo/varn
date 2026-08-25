//! `$/progress` reporting.
//!
//! The initial index walks the whole workspace and analyses every `.vn` file in
//! it. Until it finishes, requests answer from a partial graph — goto
//! definition into a file nobody opened yet simply fails. That was reported by
//! writing counters into the output channel, where the user is not looking, so
//! the server looked idle while it was busy and looked broken while it was
//! incomplete.
//!
//! Progress is advertised by the client, not the server: sending
//! `window/workDoneProgress/create` to a client that does not support it is a
//! request it will answer with an error. Hence [`Progress::begin`] takes that
//! capability and hands back nothing when it is absent — every later call is
//! then a no-op, and the caller has no branch to write.

use tower_lsp::lsp_types::notification::Progress as ProgressNotification;
use tower_lsp::lsp_types::request::WorkDoneProgressCreate;
use tower_lsp::lsp_types::*;
use tower_lsp::Client;

/// A live progress notification. Ends on [`Progress::end`].
pub struct Progress {
    client: Client,
    token: Option<ProgressToken>,
}

impl Progress {
    /// Start reporting, if the client asked to be told.
    pub async fn begin(client: &Client, supported: bool, id: &str, title: &str) -> Self {
        let mut progress = Self {
            client: client.clone(),
            token: None,
        };
        if !supported {
            return progress;
        }

        let token = ProgressToken::String(id.to_owned());
        if client
            .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                token: token.clone(),
            })
            .await
            .is_err()
        {
            return progress;
        }

        progress.token = Some(token);
        progress
            .send(WorkDoneProgress::Begin(WorkDoneProgressBegin {
                title: title.to_owned(),
                cancellable: Some(false),
                message: None,
                percentage: Some(0),
            }))
            .await;
        progress
    }

    pub async fn report(&self, message: String, percentage: u32) {
        self.send(WorkDoneProgress::Report(WorkDoneProgressReport {
            cancellable: Some(false),
            message: Some(message),
            percentage: Some(percentage),
        }))
        .await;
    }

    pub async fn end(self, message: String) {
        self.send(WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some(message),
        }))
        .await;
    }

    async fn send(&self, value: WorkDoneProgress) {
        let Some(token) = self.token.clone() else {
            return;
        };
        self.client
            .send_notification::<ProgressNotification>(ProgressParams {
                token,
                value: ProgressParamsValue::WorkDone(value),
            })
            .await;
    }
}
