//! The analysis thread.
//!
//! One thread owns the analysis, and nothing owned by it leaves.
//!
//! Requests arrive as closures. A closure runs *on* the analysis thread, with
//! `&mut Analyzer` in hand, and returns whatever it wants — but the return type
//! is bound by `Send`, so a closure that tried to hand back something that
//! shouldn't cross the boundary (an `Rc`, a thread-local handle) does not
//! compile. The boundary is enforced by the type system rather than asserted
//! in a comment.
//!
//! Two queues, not one, and `spawn` drains the foreground one first: a
//! workspace-wide index walks hundreds of files as individual jobs (one
//! `run_background` call per file — see `backend::lifecycle::index_workspace`),
//! most a few milliseconds, but a handful of `tests/errors/*` fixtures exist
//! specifically to be enormous (one is 65k lines, generated to trip a
//! constant-pool limit) and take seconds to check. A single FIFO queue made
//! every live request submitted while one of those was running — hover,
//! completion, whatever the editor was doing — wait out that whole file
//! before its own, much smaller, job even started. `query()`
//! (`backend/mod.rs`) — every interactive LSP handler — calls `run`, which
//! goes on the foreground queue; only the indexing loop calls
//! `run_background`. The worker can't preempt a job already in flight (still
//! one thread), so the one genuinely huge fixture still costs whoever is
//! unlucky enough to submit a request the instant it starts, but every other
//! file no longer costs everyone their turn.

use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::workspace::Workspace;

/// Everything the server knows about the workspace, owned by one thread.
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

/// A handle to the analysis thread. Cloneable, `Send`, holds no analysis state.
#[derive(Clone)]
pub struct AnalysisHandle {
    fg_tx: mpsc::UnboundedSender<Job>,
    bg_tx: mpsc::UnboundedSender<Job>,
}

impl AnalysisHandle {
    /// Start the analysis thread.
    ///
    /// A plain OS thread, not a tokio task: jobs are CPU-bound and run to
    /// completion, and the state they touch must stay on this one thread
    /// rather than migrate between workers (see the module doc). The tiny
    /// current-thread runtime underneath exists only so the loop can
    /// `select!` between the two queues with the foreground one biased —
    /// nothing here runs concurrently, one job always runs to completion
    /// before the next is picked.
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

    /// Run `f` on the analysis thread and await what it returns, ahead of any
    /// queued background work. Every interactive LSP request goes through
    /// this (via `Backend::query`).
    ///
    /// `R: Send` is the load-bearing bound: it is what stops a caller from
    /// smuggling out state that must stay on the analysis thread. Yields
    /// `None` only if the analysis thread is gone, which happens at shutdown.
    pub async fn run<R, F>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut Analyzer) -> R + Send + 'static,
        R: Send + 'static,
    {
        Self::send_and_await(&self.fg_tx, f).await
    }

    /// Like `run`, but queued behind every foreground request instead of
    /// ahead of them — for work the editor is not actively waiting on, i.e.
    /// the initial workspace index. Never call this from a request handler:
    /// it would starve itself behind the very indexing it is trying to time.
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
            // A dropped receiver means the request was cancelled; the work
            // is already done by here, so there is nothing to undo.
            let _ = reply_tx.send(f(a));
        }))
        .ok()?;
        reply_rx.await.ok()
    }

    /// Queue `f` without waiting for it, ahead of background work — used for
    /// live editor events (`didClose`, `didDelete`) that mutate state a
    /// foreground request might read next, not for anything that can wait.
    pub fn submit<F>(&self, f: F)
    where
        F: FnOnce(&mut Analyzer) + Send + 'static,
    {
        let _ = self.fg_tx.send(Box::new(f));
    }
}
