use tower_lsp::lsp_types::notification::Progress as ProgressNotification;
use tower_lsp::lsp_types::request::WorkDoneProgressCreate;
use tower_lsp::lsp_types::*;
use tower_lsp::Client;

pub struct Progress {
    client: Client,
    token: Option<ProgressToken>,
}

impl Progress {
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
