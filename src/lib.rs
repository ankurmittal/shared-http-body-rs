//! A library for creating shareable HTTP bodies that can be cloned and consumed by multiple tasks.
//!
//! [`SharedBody`] wraps any [`http_body::Body`] to make it cloneable. All clones share the same underlying
//! body state, so clones created at the same time will see the same frames, while clones created
//! after partial consumption will only see the remaining frames.
//!
//! # Examples
//!
//! ```
//! use shared_http_body::SharedBody;
//! use http_body_util::{BodyExt, StreamBody};
//! use http_body::Frame;
//! use bytes::Bytes;
//! use futures_util::stream;
//!
//! # tokio_test::block_on(async {
//! // Create a body from a stream of frames
//! let chunks = vec!["hello", "world"];
//! let stream = stream::iter(chunks.into_iter().map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))));
//! let body = StreamBody::new(stream);
//! let shared_body = SharedBody::new(body);
//!
//! // Clone the body for multiple consumers
//! let consumer1 = shared_body.clone();
//! let consumer2 = shared_body.clone();
//!
//! // Both consumers will receive all frames
//! let result1 = consumer1.collect().await.unwrap().to_bytes();
//! let result2 = consumer2.collect().await.unwrap().to_bytes();
//!
//! assert_eq!(result1, Bytes::from("helloworld"));
//! assert_eq!(result2, Bytes::from("helloworld"));
//! # });
//! ```
//!
//! # Requirements
//!
//! The underlying [`http_body::Body`] type must be [`Unpin`], and both the body's data (`Body::Data`)
//! and error types (`Body::Error`) must implement [`Clone`].
//!
//! # Behavior
//!
//! When you clone a [`SharedBody`], the clone will start from the current position
//! of the body being cloned, not from the beginning of the original data. Each
//! `SharedBody` maintains its own independent position. This means:
//!
//! - Clones created from the same body at the same time will see the same frames
//! - Clones created after consumption will only see frames remaining from that body's position
//! - Each clone can be consumed independently and can itself be cloned from its current position
//!
//! For example, with a body containing 4 frames:
//! ```
//! use shared_http_body::SharedBody;
//! use http_body_util::{BodyExt, StreamBody};
//! use http_body::Frame;
//! use bytes::Bytes;
//! use futures_util::stream;
//!
//! # tokio_test::block_on(async {
//! let chunks = vec!["frame1", "frame2", "frame3", "frame4"];
//! let stream = stream::iter(chunks.into_iter().map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s)))));
//! let body = StreamBody::new(stream);
//! let mut original = SharedBody::new(body);
//!
//! // Consume first frame
//! use http_body::Body;
//! let _ = std::pin::Pin::new(&mut original).poll_frame(&mut std::task::Context::from_waker(&futures_util::task::noop_waker()));
//! // Or use the frame method from BodyExt
//! let _ = http_body_util::BodyExt::frame(&mut original).await;
//!
//! // Clone after consuming 1 frame - clone1 will have 3 remaining frames
//! let clone1 = original.clone();
//!
//! # });
//! ```
//!
//! # Thread Safety
//!
//! `SharedBody` is both [`Send`] and [`Sync`] when the underlying body and its data/error types
//! are `Send` and `Sync`. This means cloned bodies can be safely moved across threads
//! and shared between tasks running on different threads.
//!
//! ```
//! use shared_http_body::SharedBody;
//! use http_body_util::{BodyExt, StreamBody};
//! use http_body::Frame;
//! use bytes::Bytes;
//! use futures_util::stream;
//! use tokio::task;
//!
//! # tokio_test::block_on(async {
//! let data = vec![Bytes::from("data1"), Bytes::from("data2"), Bytes::from("data3")];
//! let stream = stream::iter(data.clone().into_iter().map(|b| Ok::<_, std::convert::Infallible>(Frame::data(b))));
//! let body = StreamBody::new(stream);
//! let shared_body = SharedBody::new(body);
//!
//! // Clone and move to different threads
//! let body1 = shared_body.clone();
//! let body2 = shared_body.clone();
//!
//! let handle1 = task::spawn(async move {
//!     body1.collect().await.unwrap().to_bytes()
//! });
//!
//! let handle2 = task::spawn(async move {
//!     body2.collect().await.unwrap().to_bytes()
//! });
//!
//! let (result1, result2) = tokio::join!(handle1, handle2);
//! assert_eq!(result1.unwrap(), Bytes::from("data1data2data3"));
//! assert_eq!(result2.unwrap(), Bytes::from("data1data2data3"));
//! # });
//! ```
//!
//! # Use Cases
//!
//! `SharedBody` is particularly useful for:
//!
//! - **Dark Forwarding**: Clone the body to send it to a secondary destination (e.g., testing in production, shadow traffic)
//! - **Request Retries**: Keep a clone of the request body for retry attempts on failure
//! - **Request/Response Logging**: Clone the body to log it while still forwarding to the handler
//!

mod clonable_frame;
mod inner;
mod shared_body;

pub use crate::shared_body::SharedBody;

#[cfg(doc)]
use http_body::Body;
