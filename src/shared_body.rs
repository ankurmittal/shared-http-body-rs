use futures_util::{future::Shared, FutureExt};
use http_body::SizeHint;
use std::future::Future;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use crate::inner::{InnerFuture, IsEndStream};

/// A cloneable HTTP body wrapper that allows multiple consumers to share the same body.
///
/// `SharedBody` wraps any [`http_body::Body`] that is [`Unpin`] and whose data and error types
/// implement [`Clone`], allowing it to be cloned and consumed by multiple tasks simultaneously.
/// All clones share the same underlying body state and position.
///
/// # Examples
///
/// Basic usage with concurrent consumers:
///
/// ```
/// use shared_http_body::SharedBody;
/// use http_body_util::{BodyExt, StreamBody};
/// use http_body::Frame;
/// use bytes::Bytes;
/// use futures_util::stream;
///
/// # tokio_test::block_on(async {
/// let chunks = vec!["hello", "world"];
/// let stream = stream::iter(chunks.into_iter().map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))));
/// let body = StreamBody::new(stream);
/// let shared_body = SharedBody::new(body);
///
/// // Create multiple consumers
/// let consumer1 = shared_body.clone();
/// let consumer2 = shared_body.clone();
///
/// // Both will receive all frames
/// let result1 = consumer1.collect().await.unwrap().to_bytes();
/// let result2 = consumer2.collect().await.unwrap().to_bytes();
///
/// assert_eq!(result1, Bytes::from("helloworld"));
/// assert_eq!(result2, Bytes::from("helloworld"));
/// # });
/// ```
///
/// Cloning after partial consumption:
///
/// ```
/// use shared_http_body::SharedBody;
/// use http_body_util::{BodyExt, StreamBody};
/// use http_body::Frame;
/// use bytes::Bytes;
/// use futures_util::stream;
///
/// # tokio_test::block_on(async {
/// let chunks = vec!["first", "second", "third"];
/// let stream = stream::iter(chunks.into_iter().map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))));
/// let body = StreamBody::new(stream);
/// let mut shared_body = SharedBody::new(body);
///
/// // Consume first frame
/// use http_body_util::BodyExt;
/// let _ = http_body_util::BodyExt::frame(&mut shared_body).await;
///
/// // Clone after partial consumption
/// let cloned = shared_body.clone();
/// let remaining = cloned.collect().await.unwrap().to_bytes();
///
/// // Clone only sees remaining frames
/// assert_eq!(remaining, Bytes::from("secondthird"));
/// # });
/// ```
///
/// # Requirements
///
/// The wrapped body must satisfy these bounds:
/// - [`http_body::Body`]: The type must implement the Body trait
/// - [`Unpin`]: Required for safe polling without pinning
/// - [`Clone`] for `Body::Data`: The data type must be cloneable
/// - [`Clone`] for `Body::Error`: The error type must be cloneable
///
/// # Thread Safety
///
/// `SharedBody` is both [`Send`] and [`Sync`] when the underlying body and its data/error types
/// are `Send` and `Sync`. This means cloned bodies can be safely moved across threads
/// and shared between tasks running on different threads.
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
    #[cfg(feature = "stats")]
    stats: crate::stats::Stats,
}

impl<B> Clone for SharedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    fn clone(&self) -> Self {
        let s = Self {
            future: self.future.clone(),
            is_end_stream: self.is_end_stream,
            size_hint: self.size_hint.clone(),
            #[cfg(feature = "stats")]
            stats: self.stats.clone(),
        };

        #[cfg(feature = "stats")]
        if self.future.is_some() {
            s.stats.increment();
        }

        s
    }
}

impl<B> SharedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    /// Creates a new `SharedBody` from the given HTTP body.
    ///
    /// The body must implement [`Unpin`], and both its data type (`Body::Data`) and
    /// error type (`Body::Error`) must implement [`Clone`]. Once created, the `SharedBody`
    /// can be cloned to create multiple consumers that all share the same underlying body state.
    ///
    /// # Examples
    ///
    /// ```
    /// use shared_http_body::SharedBody;
    /// use http_body_util::StreamBody;
    /// use http_body::Frame;
    /// use bytes::Bytes;
    /// use futures_util::stream;
    ///
    /// let chunks = vec!["hello", "world"];
    /// let stream = stream::iter(chunks.into_iter().map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))));
    /// let body = StreamBody::new(stream);
    /// let shared_body = SharedBody::new(body);
    /// ```
    ///
    /// # Panics
    ///
    /// This method does not panic under normal circumstances.
    pub fn new(body: B) -> Self {
        let size_hint = body.size_hint();
        let is_end = body.is_end_stream();

        let s = Self {
            future: InnerFuture::new(body).shared().into(),
            is_end_stream: is_end,
            size_hint,
            #[cfg(feature = "stats")]
            stats: crate::stats::Stats::new(),
        };

        #[cfg(feature = "stats")]
        s.stats.increment();

        s
    }

    /// Returns the number of active clones of this body, including the current instance.
    ///
    /// This method allows you to track how many shared consumers exist for the body.
    /// The count includes the current instance, so a value of 1 means there are no clones.
    ///
    /// # Examples
    ///
    /// ```
    /// use shared_http_body::SharedBody;
    /// use http_body_util::StreamBody;
    /// use http_body::Frame;
    /// use bytes::Bytes;
    /// use futures_util::stream;
    ///
    /// let chunks = vec!["test"];
    /// let stream = stream::iter(chunks.into_iter().map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))));
    /// let body = StreamBody::new(stream);
    /// let shared = SharedBody::new(body);
    /// let stats = shared.stats();
    /// assert_eq!(stats.active_clones(), 1); // No clones yet
    ///
    /// let clone1 = shared.clone();
    /// assert_eq!(stats.active_clones(), 2); // Original + 1 clone
    ///
    /// let clone2 = shared.clone();
    /// assert_eq!(stats.active_clones(), 3); // Original + 2 clones
    ///
    /// drop(clone1);
    /// assert_eq!(stats.active_clones(), 2); // Original + 1 clone remaining
    /// ```
    ///
    #[cfg(feature = "stats")]
    #[cfg_attr(docsrs, doc(cfg(feature = "stats")))]
    pub fn stats(&self) -> crate::stats::Stats {
        self.stats.clone()
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
                #[cfg(feature = "stats")]
                {
                    self.stats.decrement();
                }
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

#[cfg(feature = "stats")]
impl<B> Drop for SharedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: Clone,
    B::Error: Clone,
{
    fn drop(&mut self) {
        // Decrement active-clone count when this handle is dropped.
        if self.future.is_some() {
            self.stats.decrement();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::stream;
    use http_body::Body;
    use http_body::Frame;
    use http_body_util::{BodyExt, StreamBody};

    type TestBody = SharedBody<
        StreamBody<
            stream::Iter<std::vec::IntoIter<Result<Frame<Bytes>, std::convert::Infallible>>>,
        >,
    >;
    static_assertions::assert_impl_all!(TestBody: Send, Sync);

    // Helper function to create a test body from a vector of byte chunks
    fn create_test_body(
        chunks: Vec<&'static str>,
    ) -> impl http_body::Body<Data = Bytes, Error = std::convert::Infallible> + Unpin {
        let stream = stream::iter(chunks.into_iter().map(|s| Ok(Frame::data(Bytes::from(s)))));
        StreamBody::new(stream)
    }

    #[tokio::test]
    async fn test_basic_shared_body_works() {
        let chunks = vec!["hello", "world"];
        let body = create_test_body(chunks.clone());
        let shared_body = SharedBody::new(body);

        let mut collected = Vec::new();
        let mut body_pin = std::pin::pin!(shared_body);
        while let Some(frame) = body_pin.frame().await {
            let frame = frame.unwrap();
            if let Some(data) = frame.data_ref() {
                collected.push(data.clone());
            }
        }

        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0], Bytes::from("hello"));
        assert_eq!(collected[1], Bytes::from("world"));
    }

    #[tokio::test]
    async fn test_multiple_clones_get_same_data() {
        let chunks = vec!["foo", "bar", "baz"];
        let body = create_test_body(chunks.clone());
        let shared_body = SharedBody::new(body);

        let clone1 = shared_body.clone();
        let clone2 = shared_body.clone();
        let clone3 = shared_body.clone();

        // Collect from all clones concurrently
        let (result1, result2, result3) = tokio::join!(
            clone1.collect().map(|r| r.unwrap().to_bytes()),
            clone2.collect().map(|r| r.unwrap().to_bytes()),
            clone3.collect().map(|r| r.unwrap().to_bytes())
        );

        let expected = Bytes::from("foobarbaz");
        assert_eq!(result1, expected);
        assert_eq!(result2, expected);
        assert_eq!(result3, expected);
    }

    #[tokio::test]
    async fn test_clone_after_partial_consumption() {
        let chunks = vec!["first", "second", "third", "fourth"];
        let body = create_test_body(chunks);
        let mut shared_body = SharedBody::new(body);

        // Consume first frame
        let first_frame = std::pin::Pin::new(&mut shared_body).frame().await;
        assert!(first_frame.is_some());
        if let Some(Ok(frame)) = first_frame {
            assert_eq!(frame.data_ref().unwrap(), &Bytes::from("first"));
        }

        // Now clone and collect remaining
        let cloned_body = shared_body.clone();
        let remaining = cloned_body.collect().await.unwrap().to_bytes();

        // Clone should only see remaining frames
        assert_eq!(remaining, Bytes::from("secondthirdfourth"));
    }

    #[tokio::test]
    async fn test_with_different_data_types() {
        let chunks = vec!["alpha", "beta", "gamma"];
        let body = create_test_body(chunks);
        let shared_body = SharedBody::new(body);

        let clone1 = shared_body.clone();
        let clone2 = shared_body.clone();

        let (result1, result2) = tokio::join!(
            clone1.collect().map(|r| r.unwrap().to_bytes()),
            clone2.collect().map(|r| r.unwrap().to_bytes())
        );

        let expected = Bytes::from("alphabetagamma");
        assert_eq!(result1, expected);
        assert_eq!(result2, expected);
    }

    #[tokio::test]
    async fn test_empty_body_behavior() {
        let chunks: Vec<&str> = vec![];
        let body = create_test_body(chunks);
        let shared_body = SharedBody::new(body);

        let clone1 = shared_body.clone();
        let clone2 = shared_body.clone();

        let (result1, result2) = tokio::join!(
            clone1.collect().map(|r| r.unwrap().to_bytes()),
            clone2.collect().map(|r| r.unwrap().to_bytes())
        );

        assert!(result1.is_empty());
        assert!(result2.is_empty());
    }

    #[tokio::test]
    async fn test_single_frame_body() {
        let chunks = vec!["single"];
        let body = create_test_body(chunks);
        let mut shared_body = SharedBody::new(body);

        let clone1 = shared_body.clone();
        let clone2 = shared_body.clone();

        let (result1, result2) = tokio::join!(
            clone1.collect().map(|r| r.unwrap().to_bytes()),
            clone2.collect().map(|r| r.unwrap().to_bytes())
        );

        assert_eq!(result1, Bytes::from("single"));
        assert_eq!(result2, Bytes::from("single"));

        // Consume from original
        let frame = std::pin::Pin::new(&mut shared_body).frame().await;
        assert!(frame.is_some());

        // Verify exhausted
        let remaining = shared_body.collect().await.unwrap().to_bytes();
        assert!(remaining.is_empty());
    }

    #[tokio::test]
    async fn test_many_clones_stress_test() {
        let chunks = vec!["test", "data", "here"];
        let body = create_test_body(chunks);
        let shared_body = SharedBody::new(body);

        // Create 20 clones
        let mut clone_futures = Vec::new();
        for _ in 0..20 {
            let clone = shared_body.clone();
            clone_futures.push(async move { clone.collect().await.unwrap().to_bytes() });
        }

        let all_results = futures_util::future::join_all(clone_futures).await;

        let expected = Bytes::from("testdatahere");
        for result in all_results {
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_cross_thread_sharing() {
        use std::sync::Arc;
        use tokio::task;

        let chunks = vec!["cross", "thread", "test"];
        let body = create_test_body(chunks);
        let shared_body = Arc::new(SharedBody::new(body));

        // Clone and move to different threads
        let body1 = Arc::clone(&shared_body);
        let body2 = Arc::clone(&shared_body);

        let handle1 = task::spawn(async move {
            let cloned = (*body1).clone();
            cloned.collect().await.unwrap().to_bytes()
        });

        let handle2 = task::spawn(async move {
            let cloned = (*body2).clone();
            cloned.collect().await.unwrap().to_bytes()
        });

        let (result1, result2) = tokio::join!(handle1, handle2);
        let expected = Bytes::from("crossthreadtest");
        assert_eq!(result1.unwrap(), expected);
        assert_eq!(result2.unwrap(), expected);
    }

    #[tokio::test]
    async fn test_is_end_stream_behavior() {
        let chunks = vec!["one", "two"];
        let body = create_test_body(chunks);
        let mut shared_body = SharedBody::new(body);

        // Initially should not be end stream
        assert!(!shared_body.is_end_stream());

        // Consume first frame
        let _ = std::pin::Pin::new(&mut shared_body).frame().await;
        assert!(!shared_body.is_end_stream());

        // Consume second frame
        let _ = std::pin::Pin::new(&mut shared_body).frame().await;
        assert!(!shared_body.is_end_stream());

        // Consume until exhausted
        while std::pin::Pin::new(&mut shared_body).frame().await.is_some() {}

        // Now should be end stream
        assert!(shared_body.is_end_stream());
    }

    #[tokio::test]
    async fn test_size_hint_updates() {
        let chunks = vec!["a", "b", "c"];
        let body = create_test_body(chunks);
        let shared_body = SharedBody::new(body);

        // Size hint should be available (though exact values depend on StreamBody implementation)
        let _hint = shared_body.size_hint();

        // Just verify we can call it without panicking
        let clone = shared_body.clone();
        let _clone_hint = clone.size_hint();
    }

    #[tokio::test]
    async fn test_poll_frame_after_exhaustion() {
        let chunks = vec!["data"];
        let body = create_test_body(chunks);
        let mut shared_body = SharedBody::new(body);

        // Consume all frames
        while std::pin::Pin::new(&mut shared_body).frame().await.is_some() {}

        // Poll again - should return None
        let result = std::pin::Pin::new(&mut shared_body).frame().await;
        assert!(result.is_none());

        // And again to make sure it's consistently None
        let result2 = std::pin::Pin::new(&mut shared_body).frame().await;
        assert!(result2.is_none());
    }

    #[tokio::test]
    async fn test_clone_from_exhausted_body() {
        let chunks = vec!["test"];
        let body = create_test_body(chunks);
        let shared_body = SharedBody::new(body);

        // Clone before exhausting
        let clone1 = shared_body.clone();
        let clone2 = shared_body.clone();

        // Exhaust via clone1
        let result1 = clone1.collect().await.unwrap().to_bytes();
        assert_eq!(result1, Bytes::from("test"));

        // clone2 should NOT be exhausted - clones are independent
        let result2 = clone2.collect().await.unwrap().to_bytes();
        assert_eq!(result2, Bytes::from("test"));

        // Original should also still have data
        let result3 = shared_body.collect().await.unwrap().to_bytes();
        assert_eq!(result3, Bytes::from("test"));
    }

    #[tokio::test]
    async fn test_pending_future_behavior() {
        use std::pin::Pin;
        use std::sync::{Arc, Mutex};
        use std::task::{Context, Poll, Waker};

        // Create a custom body that returns Pending once
        #[derive(Clone)]
        struct PendingOnceBody {
            data: Vec<Bytes>,
            index: usize,
            has_returned_pending: Arc<Mutex<bool>>,
            stored_waker: Arc<Mutex<Option<Waker>>>,
        }

        impl http_body::Body for PendingOnceBody {
            type Data = Bytes;
            type Error = std::convert::Infallible;

            fn poll_frame(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
                let this = self.get_mut();
                let mut has_returned_pending = this.has_returned_pending.lock().unwrap();

                // Return Pending exactly once
                if !*has_returned_pending {
                    *has_returned_pending = true;
                    *this.stored_waker.lock().unwrap() = Some(cx.waker().clone());

                    // Immediately wake to continue
                    let waker = cx.waker().clone();
                    waker.wake();

                    return Poll::Pending;
                }

                // After returning Pending once, behave normally
                if this.index < this.data.len() {
                    let data = this.data[this.index].clone();
                    this.index += 1;
                    Poll::Ready(Some(Ok(Frame::data(data))))
                } else {
                    Poll::Ready(None)
                }
            }
        }

        let pending_body = PendingOnceBody {
            data: vec![Bytes::from("test1"), Bytes::from("test2")],
            index: 0,
            has_returned_pending: Arc::new(Mutex::new(false)),
            stored_waker: Arc::new(Mutex::new(None)),
        };

        let shared_body = SharedBody::new(pending_body);
        let result = shared_body.collect().await.unwrap().to_bytes();

        assert_eq!(result, Bytes::from("test1test2"));
    }

    #[tokio::test]
    #[cfg(feature = "stats")]
    async fn test_stats() {
        use bytes::Bytes;
        use futures_util::stream;
        use http_body::Frame;
        use http_body_util::{BodyExt, StreamBody};

        // 1) Creation with non-empty body
        let chunks = vec!["frame1", "frame2", "frame3"];
        let stream = stream::iter(
            chunks
                .into_iter()
                .map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))),
        );
        let body = StreamBody::new(stream);
        let shared = SharedBody::new(body);
        let stats = shared.stats();

        assert_eq!(stats.active_clones(), 1);

        // clone increments
        let clone1 = shared.clone();
        assert_eq!(stats.active_clones(), 2);

        // clone of clone increments
        let clone2 = clone1.clone();
        assert_eq!(stats.active_clones(), 3);

        // dropping a clone decrements
        drop(clone2);
        assert_eq!(stats.active_clones(), 2);

        // exhausting the original handle decrements its contribution
        let _orig_collected = shared.collect().await.unwrap().to_bytes();

        // only clone1 remains
        assert_eq!(stats.active_clones(), 1);

        // dropping the last active clone brings the count to zero
        drop(clone1);
        assert_eq!(stats.active_clones(), 0);

        // empty-body case: wrapper is counted until polled/exhausted
        let empty_stream =
            stream::iter(Vec::<Result<Frame<Bytes>, std::convert::Infallible>>::new());
        let shared_empty = SharedBody::new(StreamBody::new(empty_stream));
        let stats_empty = shared_empty.stats();

        // The body wrapper exists and hasn't been polled yet, so it's counted.
        assert_eq!(stats_empty.active_clones(), 1);

        let clone_empty = shared_empty.clone();
        assert_eq!(stats_empty.active_clones(), 2);
        drop(clone_empty);
        assert_eq!(stats_empty.active_clones(), 1);

        // exhausting the wrapper clears the count
        let _ = shared_empty.collect().await.unwrap().to_bytes();
        assert_eq!(stats_empty.active_clones(), 0);

        // cloning after partial consumption
        let chunks2 = vec!["data1", "data2", "data3"];
        let stream2 = stream::iter(
            chunks2
                .into_iter()
                .map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))),
        );
        let body2 = StreamBody::new(stream2);
        let mut shared2 = SharedBody::new(body2);
        let stats2 = shared2.stats();
        assert_eq!(stats2.active_clones(), 1);

        // consume one frame
        let first = http_body_util::BodyExt::frame(&mut shared2).await;
        assert!(first.is_some());

        let clone_after = shared2.clone();
        assert_eq!(stats2.active_clones(), 2);

        drop(clone_after);
        assert_eq!(stats2.active_clones(), 1);

        let _ = shared2.collect().await.unwrap().to_bytes();
        assert_eq!(stats2.active_clones(), 0);
    }
}
