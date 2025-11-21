use futures_util::future::Shared;
use futures_util::FutureExt;
use http_body::SizeHint;
use std::future::Future;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use crate::clonable_frame::ClonableFrame;

/// A future that polls an HTTP body once to retrieve the next frame.
///
/// This is a low-level primitive used by [`InnerFuture`]. It wraps a body and
/// polls it exactly once, returning both the frame (if any) and the body itself.
/// This allows the body to be reused for subsequent polls.
///
/// # Design Rationale
///
/// We need to poll the body, get a frame, but also retain ownership of the body
/// for the next poll. This future accomplishes that by taking ownership of the body,
/// polling it once, and then returning both the result and the body back to the caller.
///
/// # Panics
///
/// Polling this future more than once will panic with "polling BodyFuture twice".
/// This is by design - each `BodyFuture` should only be polled once, after which
/// a new `BodyFuture` should be created with the returned body.
#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
struct BodyFuture<B> {
    inner: Option<B>,
}

impl<B> Future for BodyFuture<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    type Output = (Option<Result<ClonableFrame<B::Data>, B::Error>>, B);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let item = {
            let b = self.inner.as_mut().expect("polling BodyFuture twice");
            ready!(Pin::new(b).poll_frame(cx))
        };
        let body = self.inner.take().unwrap();
        Poll::Ready((item.map(|r| r.map(ClonableFrame::new)), body))
    }
}

impl<B> BodyFuture<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    fn new(body: B) -> Self {
        Self { inner: body.into() }
    }
}

pub(crate) type IsEndStream = bool;

/// Internal future wrapper that enables sharing of body state across multiple consumers.
///
/// This is the core mechanism that makes `SharedBody` work. It wraps a [`BodyFuture`]
/// and implements the below pattern:
///
/// 1. When polled, it polls the underlying `BodyFuture` to get the next frame
/// 2. It extracts HTTP metadata (`is_end_stream`, `size_hint`) from the body
/// 3. It creates a **new** `InnerFuture` with the body and wraps it in `Shared`
/// 4. It returns the frame, the new shared future, and the metadata
///
/// This allows each `SharedBody` clone to independently advance through the frames
/// while sharing the underlying polling work through `futures_util::future::Shared`.
///
/// # How Sharing Works
///
/// When multiple `SharedBody` instances poll the same `Shared<InnerFuture>`:
/// - The first poll actually polls the underlying body
/// - Subsequent polls receive a clone of the result
/// - Each consumer then gets its own `Shared<InnerFuture>` for the next frame
///
/// This creates a chain of shared futures, where each link in the chain represents
/// one frame from the body.
///
/// # Output Type
///
/// The output is `Option<(frame, next_future, is_end_stream, size_hint)>`:
/// - `Some(...)` when a frame is available
/// - `None` when the body is exhausted
#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub(crate) struct InnerFuture<B> {
    inner: Option<BodyFuture<B>>,
}

impl<B> InnerFuture<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    /// Creates a new `InnerFuture` from the given body.
    pub(crate) fn new(body: B) -> Self {
        InnerFuture {
            inner: Some(BodyFuture::new(body)),
        }
    }
}

impl<B> Future for InnerFuture<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    type Output = Option<(
        Result<ClonableFrame<B::Data>, B::Error>,
        Shared<Self>,
        IsEndStream,
        SizeHint,
    )>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner_future = match self.inner.as_mut() {
            Some(f) => Pin::new(f),
            None => return Poll::Ready(None),
        };

        match inner_future.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready((Some(item), body)) => {
                let is_end_stream = body.is_end_stream();
                let size_hint = body.size_hint();
                let next_shared_future = InnerFuture::new(body).shared();
                self.inner.take();
                Poll::Ready(Some((item, next_shared_future, is_end_stream, size_hint)))
            }
            Poll::Ready((None, _body)) => {
                self.inner.take();
                Poll::Ready(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::stream;
    use http_body::Frame;
    use http_body_util::StreamBody;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[tokio::test]
    async fn test_inner_future_direct_poll() {
        // Test InnerFuture directly to hit the edge case where inner is None
        let chunks = vec!["test"];
        let stream = stream::iter(
            chunks
                .into_iter()
                .map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))),
        );

        let body = StreamBody::new(stream);

        let mut inner_future = InnerFuture::new(body);

        // Create a dummy waker for polling
        let waker = futures_util::task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        // First poll should return the item
        let result = Pin::new(&mut inner_future).poll(&mut cx);
        assert!(matches!(result, Poll::Ready(Some(_))));

        // Poll again - this should hit the None case and return Poll::Ready(None)
        let result = Pin::new(&mut inner_future).poll(&mut cx);
        assert!(matches!(result, Poll::Ready(None)));
    }
}
