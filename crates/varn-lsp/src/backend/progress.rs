use tower_lsp_f::lsp_types::ProgressNotification;
use tower_lsp_f::lsp_types::WorkDoneProgressCreateRequest;
use tower_lsp_f::lsp_types::*;
use tower_lsp_f::Client;

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
            .send_request::<WorkDoneProgressCreateRequest>(WorkDoneProgressCreateParams {
                token: token.clone(),
            })
            .await
            .is_err()
        {
            return progress;
        }

        progress.token = Some(token);
        progress
            .send(serde_json::to_value(WorkDoneProgressBegin {
                title: title.to_owned(),
                cancellable: Some(false),
                message: None,
                percentage: Some(0),
            }))
            .await;
        progress
    }

    pub async fn report(&self, message: String, percentage: u32) {
        self.send(serde_json::to_value(WorkDoneProgressReport {
            cancellable: Some(false),
            message: Some(message),
            percentage: Some(percentage),
        }))
        .await;
    }

    pub async fn end(self, message: String) {
        self.send(serde_json::to_value(WorkDoneProgressEnd {
            message: Some(message),
        }))
        .await;
    }

    async fn send(&self, value: Result<serde_json::Value, serde_json::Error>) {
        let Some(token) = self.token.clone() else {
            return;
        };
        let Ok(value) = value else {
            return;
        };
        self.client
            .send_notification::<ProgressNotification>(ProgressParams { token, value })
            .await;
    }
}
