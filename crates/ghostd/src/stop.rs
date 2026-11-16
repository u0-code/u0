//! Knowing when to stop: a signal, or, with `--with-parent`, the program
//! that started her going away.

use tracing::warn;

/// Resolves when she should stop: Ctrl-C at a terminal, or SIGTERM, which
/// is how systemd, docker and a shutting-down machine ask, or, `with_parent`,
/// standard input closing because whoever started her is gone. Every way
/// she is stopped in order, so that nothing is cut off halfway through
/// being written. Says which it was.
pub(crate) fn told_to_stop(with_parent: bool) -> impl std::future::Future<Output = &'static str> {
    let orphaned = with_parent.then(parent_gone);
    // The handlers are set up here, when this is called, not when the
    // future is first polled: a signal that comes in between is kept.
    #[cfg(unix)]
    let term = {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::terminate())
            .inspect_err(|e| {
                warn!("cannot listen for SIGTERM ({e}); only Ctrl-C stops her in order");
            })
            .ok()
    };
    let ctrl_c = tokio::signal::ctrl_c();
    async move {
        let orphaned = async {
            match orphaned {
                Some(gone) => {
                    let _ = gone.await;
                }
                None => std::future::pending().await,
            }
        };
        #[cfg(unix)]
        if let Some(mut term) = term {
            return tokio::select! {
                _ = ctrl_c => "interrupted",
                _ = term.recv() => "terminated",
                () = orphaned => "the program that started her is gone",
            };
        }
        tokio::select! {
            _ = ctrl_c => "interrupted",
            () = orphaned => "the program that started her is gone",
        }
    }
}

/// Resolves once standard input has closed: read on a thread of its own,
/// since a read that waits for ever must not hold up anything else.
fn parent_gone() -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 256];
        while matches!(stdin.read(&mut buf), Ok(n) if n > 0) {}
        let _ = tx.send(());
    });
    rx
}
