use std::future::Future;

use agent_desktop_core::{AdapterError, Deadline};

/// Races `future` against the command's absolute deadline, so a provider that
/// never answers costs at most the time the command was given rather than the
/// connection's method timeout.
pub(crate) async fn within<T>(
    deadline: Deadline,
    future: impl Future<Output = Result<T, AdapterError>>,
) -> Result<T, AdapterError> {
    let expiry = async {
        async_io::Timer::after(deadline.remaining()).await;
        Err(deadline.timeout_error())
    };
    futures_lite::future::or(future, expiry).await
}

/// Like [`within`], but an expired deadline yields `None` instead of an error,
/// for walks that report a partial observation rather than discarding it.
pub(crate) async fn until<T>(deadline: Deadline, future: impl Future<Output = T>) -> Option<T> {
    let expiry = async {
        async_io::Timer::after(deadline.remaining()).await;
        None
    };
    futures_lite::future::or(async { Some(future.await) }, expiry).await
}

/// Bounds one provider's answer to `limit`, yielding `None` when it does not
/// answer in time, so a single hung application cannot stall a read that
/// spans every application on the bus.
pub(crate) async fn bounded<T>(
    limit: std::time::Duration,
    future: impl Future<Output = T>,
) -> Option<T> {
    let expiry = async {
        async_io::Timer::after(limit).await;
        None
    };
    futures_lite::future::or(async { Some(future.await) }, expiry).await
}
