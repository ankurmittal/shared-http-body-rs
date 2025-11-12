use futures_util::{future::Shared, FutureExt};
use http_body::SizeHint;
use std::future::Future;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use crate::inner::{InnerFuture, IsEndStream};

#[derive(Debug)]
pub struct SharedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    future: Option<Shared<InnerFuture<B>>>,
    is_end_stream: IsEndStream,
    size_hint: SizeHint,
}

impl<B> Clone for SharedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    fn clone(&self) -> Self {
        Self {
            future: self.future.clone(),
            is_end_stream: self.is_end_stream,
            size_hint: self.size_hint.clone(),
        }
    }
}

impl<B> SharedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    pub fn new(body: B) -> Self {
        let size_hint = body.size_hint();
        let is_end = body.is_end_stream();

        Self {
            future: InnerFuture::new(body).shared().into(),
            is_end_stream: is_end,
            size_hint,
        }
    }
}

impl<B> http_body::Body for SharedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let future = match self.future.as_mut() {
            Some(f) => Pin::new(f),
            None => return Poll::Ready(None),
        };

        let item = ready!(future.poll(cx));

        match item {
            Some((item, next_shared_future, is_end_stream, size_hint)) => {
                self.future = Some(next_shared_future);
                self.is_end_stream = is_end_stream;
                self.size_hint = size_hint;
                Poll::Ready(Some(item.map(Into::into)))
            }
            None => {
                self.future.take();
                self.is_end_stream = true;
                Poll::Ready(None)
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.is_end_stream
    }

    fn size_hint(&self) -> SizeHint {
        self.size_hint.clone()
    }
}
