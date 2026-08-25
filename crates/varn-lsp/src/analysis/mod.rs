//! The analysis thread.
//!
//! Every `DocumentState` — and everything reachable from one: `BindResult`,
//! `Type`, the interned names inside them — is built on `Rc`. Sharing that
//! across tokio's blocking pool, which is what the server used to do, races on
//! non-atomic refcounts: corruption, then a use-after-free. The old code made
//! that compile with `unsafe impl Send for DocumentState`, which did not make
//! it correct; it only switched off the check that would have caught it.
//!
//! So: one thread owns the analysis, and nothing owned by it leaves.
//!
//! Requests arrive as closures. A closure runs *on* the analysis thread, with
//! `&mut Analyzer` in hand, and returns whatever it wants — but the return type
//! is bound by `Send`, and `DocumentState` is no longer `Send`, so a closure
//! that tried to hand one back does not compile. The boundary is enforced by
//! the type system rather than asserted in a comment.
//!
//! This also settles module-cache invalidation. `DiskResolver` memoizes per
//! thread; with analysis spread over a worker pool each worker kept its own
//! copy and an invalidation on one left the others stale, so cross-module
//! resolution came out right or wrong depending on who answered. One thread,
//! one graph, one invalidation.

use tokio::sync::{mpsc, oneshot};

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
    tx: mpsc::UnboundedSender<Job>,
}

impl AnalysisHandle {
    /// Start the analysis thread.
    ///
    /// A plain OS thread, not a tokio task: jobs are CPU-bound and run to
    /// completion, and the state they touch is not `Send`, so it must stay put
    /// rather than migrate between workers.
    pub fn spawn() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        std::thread::Builder::new()
            .name("varn-analysis".to_owned())
            .spawn(move || {
                let mut analyzer = Analyzer::new();
                while let Some(job) = rx.blocking_recv() {
                    job(&mut analyzer);
                }
            })
            .expect("failed to start the analysis thread");
        Self { tx }
    }

    /// Run `f` on the analysis thread and await what it returns.
    ///
    /// `R: Send` is the load-bearing bound: it is what stops a caller from
    /// smuggling out an `Rc`-backed value. Yields `None` only if the analysis
    /// thread is gone, which happens at shutdown.
    pub async fn run<R, F>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut Analyzer) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Box::new(move |a| {
                // A dropped receiver means the request was cancelled; the work
                // is already done by here, so there is nothing to undo.
                let _ = reply_tx.send(f(a));
            }))
            .ok()?;
        reply_rx.await.ok()
    }

    /// Queue `f` without waiting for it.
    pub fn submit<F>(&self, f: F)
    where
        F: FnOnce(&mut Analyzer) + Send + 'static,
    {
        let _ = self.tx.send(Box::new(f));
    }
}
