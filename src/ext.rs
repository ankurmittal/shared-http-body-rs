//! Extension trait for converting HTTP bodies into `SharedBody`.
//!
//! This module provides the `SharedBodyExt` trait which adds an `into_shared` method
//! to any type that implements `http_body::Body + Unpin` with clonable data and error types.

use crate::shared_body::SharedBody;

/// Extension trait for [`http_body::Body`] that provides the `into_shared` method.
///
/// This trait allows any HTTP body that meets the requirements to be easily converted
/// into a [`SharedBody`] for sharing across multiple consumers.
pub trait SharedBodyExt: http_body::Body {
    /// Converts this HTTP body into a [`SharedBody`].
    ///
    /// This method consumes the original body and returns a [`SharedBody`] that
    /// can be cloned to create multiple consumers sharing the same underlying body state.
    ///
    /// # Requirements
    ///
    /// The body must satisfy:
    /// - [`http_body::Body`]: The type must implement the Body trait
    /// - [`Unpin`]: Required for safe polling without additional pinning
    /// - [`Clone`] for `Self::Data`: Body data must be cloneable
    /// - [`Clone`] for `Self::Error`: Body error type must be cloneable
    ///
    /// # Examples
    ///
    /// ```
    /// use shared_http_body::{SharedBody, SharedBodyExt};
    /// use http_body_util::{BodyExt, StreamBody};
    /// use http_body::Frame;
    /// use bytes::Bytes;
    /// use futures_util::stream;
    ///
    /// # tokio_test::block_on(async {
    /// let chunks = vec!["hello", "world"];
    /// let stream = stream::iter(chunks.into_iter().map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))));
    /// let body = StreamBody::new(stream);
    ///
    /// // Use the extension trait to convert to SharedBody
    /// let shared_body = body.into_shared();
    ///
    /// let result = shared_body.collect().await.unwrap().to_bytes();
    /// assert_eq!(result, Bytes::from("helloworld"));
    /// # });
    /// ```
    fn into_shared(self) -> SharedBody<Self>
    where
        Self: Sized + Unpin,
        Self::Data: Clone,
        Self::Error: Clone,
    {
        SharedBody::new(self)
    }
}

impl<B> SharedBodyExt for B where B: http_body::Body {}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::stream;
    use http_body::Frame;
    use http_body_util::{BodyExt, StreamBody};

    #[tokio::test]
    async fn test_into_shared_trait_works() {
        let chunks = vec!["test", "data"];
        let stream = stream::iter(
            chunks
                .into_iter()
                .map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))),
        );
        let body = StreamBody::new(stream);

        let shared_body: SharedBody<_> = body.into_shared();
        let result = shared_body.collect().await.unwrap().to_bytes();

        assert_eq!(result, Bytes::from("testdata"));
    }

    #[tokio::test]
    async fn test_into_shared_with_multiple_consumers() {
        let chunks = vec!["hello", "world"];
        let stream = stream::iter(
            chunks
                .into_iter()
                .map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))),
        );
        let body = StreamBody::new(stream);

        let shared_body = body.into_shared();
        let consumer1 = shared_body.clone();
        let consumer2 = shared_body.clone();

        let result1 = consumer1.collect().await.unwrap().to_bytes();
        let result2 = consumer2.collect().await.unwrap().to_bytes();

        assert_eq!(result1, Bytes::from("helloworld"));
        assert_eq!(result2, Bytes::from("helloworld"));
    }
}
