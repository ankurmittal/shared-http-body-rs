use futures_util::future::Shared;
use futures_util::FutureExt;
use http_body::SizeHint;
use std::future::Future;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use crate::clonable_frame::ClonableFrame;

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

/// Internal future wrapper that enables sharing of body state.
///
/// This type wraps a [`BodyFuture`] and implements the logic for creating
/// shared versions of subsequent futures as the body is consumed.
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
    // The output type is changed to reflect the attempt to return a shared future.
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
