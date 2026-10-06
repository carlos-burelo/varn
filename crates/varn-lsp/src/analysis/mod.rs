use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::workspace::Workspace;

pub struct Analyzer {
    pub workspace: Workspace,
}

impl Analyzer {
    fn new() -> Self {
        Self {
            workspace: Workspace::new(),
        }
    }
}

type Job = Box<dyn FnOnce(&mut Analyzer) + Send>;

#[derive(Clone)]
pub struct AnalysisHandle {
    fg_tx: mpsc::UnboundedSender<Job>,
    bg_tx: mpsc::UnboundedSender<Job>,
}

impl AnalysisHandle {
    pub fn spawn() -> Self {
        let (fg_tx, mut fg_rx) = mpsc::unbounded_channel::<Job>();
        let (bg_tx, mut bg_rx) = mpsc::unbounded_channel::<Job>();
        std::thread::Builder::new()
            .name("varn-analysis".to_owned())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .expect("failed to start the analysis thread's runtime");
                rt.block_on(async move {
                    let mut analyzer = Analyzer::new();
                    loop {
                        let job = tokio::select! {
                            biased;
                            job = fg_rx.recv() => job,
                            job = bg_rx.recv() => job,
                        };
                        match job {
                            Some(job) => job(&mut analyzer),
                            None => break,
                        }
                    }
                });
            })
            .expect("failed to start the analysis thread");
        Self { fg_tx, bg_tx }
    }

    pub async fn run<R, F>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut Analyzer) -> R + Send + 'static,
        R: Send + 'static,
    {
        Self::send_and_await(&self.fg_tx, f).await
    }

    pub async fn run_background<R, F>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut Analyzer) -> R + Send + 'static,
        R: Send + 'static,
    {
        Self::send_and_await(&self.bg_tx, f).await
    }

    async fn send_and_await<R, F>(tx: &mpsc::UnboundedSender<Job>, f: F) -> Option<R>
    where
        F: FnOnce(&mut Analyzer) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Box::new(move |a| {
            let _ = reply_tx.send(f(a));
        }))
        .ok()?;
        reply_rx.await.ok()
    }

    pub fn submit<F>(&self, f: F)
    where
        F: FnOnce(&mut Analyzer) + Send + 'static,
    {
        let _ = self.fg_tx.send(Box::new(f));
    }
}
