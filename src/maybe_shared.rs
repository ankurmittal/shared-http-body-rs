use std::pin::Pin;
use std::task::{Context, Poll};

use http_body::{Body, Frame, SizeHint};

use crate::shared_body::SharedBody;

/// A body that is either the original body or a [`SharedBody`] made from it.
///
/// Middleware that forks only some requests, such as retries or shadow
/// traffic, can pass this one type to the next service.
///
/// ```
/// use shared_http_body::{MaybeSharedBody, SharedBody};
/// use http_body_util::Full;
/// use bytes::Bytes;
///
/// let original = MaybeSharedBody::original(Full::new(Bytes::from("hello")));
/// assert!(matches!(original, MaybeSharedBody::Original(_)));
///
/// let shared = MaybeSharedBody::shared(SharedBody::new(Full::new(Bytes::from("hello"))));
/// assert!(matches!(shared, MaybeSharedBody::Shared(_)));
/// ```
#[derive(Debug, Clone)]
pub enum MaybeSharedBody<B>
where
    B: Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    Original(B),
    Shared(SharedBody<B>),
}

impl<B> MaybeSharedBody<B>
where
    B: Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    /// Wraps a body that was not shared.
    pub fn original(body: B) -> Self {
        Self::Original(body)
    }

    /// Wraps a shared body.
    pub fn shared(body: SharedBody<B>) -> Self {
        Self::Shared(body)
    }
}

impl<B> Body for MaybeSharedBody<B>
where
    B: Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match &mut self.get_mut() {
            Self::Original(body) => Pin::new(body).poll_frame(cx),
            Self::Shared(body) => Pin::new(body).poll_frame(cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self {
            Self::Original(body) => body.is_end_stream(),
            Self::Shared(body) => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self {
            Self::Original(body) => body.size_hint(),
            Self::Shared(body) => body.size_hint(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::{BodyExt, Empty, Full};

    fn full(text: &'static str) -> Full<Bytes> {
        Full::new(Bytes::from(text))
    }

    static_assertions::assert_impl_all!(MaybeSharedBody<Full<Bytes>>: Send, Sync, Unpin);

    #[tokio::test]
    async fn an_original_body_yields_its_data() {
        let body = MaybeSharedBody::original(full("hello"));
        assert_eq!(body.collect().await.unwrap().to_bytes(), "hello");
    }

    #[tokio::test]
    async fn a_shared_body_yields_the_same_data_as_its_clones() {
        let shared = SharedBody::new(full("hello"));
        let clone = shared.clone();

        let body = MaybeSharedBody::shared(shared);
        assert_eq!(body.collect().await.unwrap().to_bytes(), "hello");
        assert_eq!(clone.collect().await.unwrap().to_bytes(), "hello");
    }

    #[test]
    fn the_constructors_pick_the_variant() {
        let original = MaybeSharedBody::original(full("a"));
        assert!(matches!(original, MaybeSharedBody::Original(_)));

        let shared = MaybeSharedBody::shared(SharedBody::new(full("a")));
        assert!(matches!(shared, MaybeSharedBody::Shared(_)));
    }

    #[tokio::test]
    async fn a_clone_yields_the_same_data() {
        let original = MaybeSharedBody::original(full("hello"));
        let clone = original.clone();
        assert_eq!(original.collect().await.unwrap().to_bytes(), "hello");
        assert_eq!(clone.collect().await.unwrap().to_bytes(), "hello");

        let shared = MaybeSharedBody::shared(SharedBody::new(full("hello")));
        let clone = shared.clone();
        assert_eq!(shared.collect().await.unwrap().to_bytes(), "hello");
        assert_eq!(clone.collect().await.unwrap().to_bytes(), "hello");
    }

    #[test]
    fn size_hint_and_end_of_stream_come_from_the_inner_body() {
        let original = MaybeSharedBody::original(full("hello"));
        assert_eq!(original.size_hint().exact(), Some(5));
        assert!(!original.is_end_stream());

        let empty = MaybeSharedBody::original(Empty::<Bytes>::new());
        assert!(empty.is_end_stream());

        let shared = MaybeSharedBody::shared(SharedBody::new(full("hello")));
        assert_eq!(shared.size_hint().exact(), Some(5));
        assert!(!shared.is_end_stream());
    }
}
