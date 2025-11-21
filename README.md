# shared_http_body

[![Crates.io](https://img.shields.io/crates/v/shared_http_body.svg)](https://crates.io/crates/shared_http_body)
[![Documentation](https://docs.rs/shared_http_body/badge.svg)](https://docs.rs/shared_http_body)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![codecov](https://codecov.io/gh/ankurmittal/shared-http-body-rs/branch/main/graph/badge.svg)](https://codecov.io/gh/ankurmittal/shared-http-body-rs)

A Rust library for creating shareable HTTP bodies that can be cloned and consumed by multiple tasks.

## Overview

`shared_http_body` provides `SharedBody`, which allows you to create an HTTP body that can be cloned and shared across multiple consumers. All clones share the same underlying body state, so clones created at the same time will see the same frames, while clones created after partial consumption will only see the remaining frames.

## Quick Example

```rust
use shared_http_body::SharedBody;
use http_body_util::{BodyExt, StreamBody};
use http_body::Frame;
use bytes::Bytes;
use futures_util::stream;

#[tokio::main]
async fn main() {
    // Create a body from a stream of frames
    let chunks = vec!["hello", "world"];
    let stream = stream::iter(
        chunks.into_iter()
            .map(|s| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(s))))
    );
    let body = StreamBody::new(stream);
    let shared_body = SharedBody::new(body);

    // Clone the body for multiple consumers
    let consumer1 = shared_body.clone();
    let consumer2 = shared_body.clone();

    // Both consumers will receive all frames
    let (result1, result2) = tokio::join!(
        consumer1.collect().map(|r| r.unwrap().to_bytes()),
        consumer2.collect().map(|r| r.unwrap().to_bytes())
    );

    assert_eq!(result1, Bytes::from("helloworld"));
    assert_eq!(result2, Bytes::from("helloworld"));
    println!("Both consumers got: {:?}", result1);
}
```

## Key Features

- **Cloneable HTTP bodies**: Create multiple consumers from a single body
- **Thread-safe**: `Send` + `Sync` - clones can be moved across threads
- **Efficient sharing**: Frames are cloned only when consumed by each clone
- **Works with any `Unpin` body**: Compatible with most HTTP body types
- **Preserves HTTP semantics**: Maintains `size_hint()` and `is_end_stream()` behavior

## Use Cases

### Dark Forwarding
Clone the body to send it to a secondary destination for testing in production or shadow traffic analysis:

```rust
use shared_http_body::SharedBody;

async fn dark_forward(body: impl http_body::Body + Unpin) 
where
    <impl http_body::Body>::Data: Clone,
    <impl http_body::Body>::Error: Clone,
{
    let shared = SharedBody::new(body);
    let shadow = shared.clone();
    
    // Send to shadow/dark environment in background
    tokio::spawn(async move {
        send_to_shadow_env(shadow).await;
    });
    
    // Forward to production handler
    production_handler(shared).await;
}
```

### Request Retries
Keep a clone of the request body for retry attempts on failure:

```rust
async fn retry_request(body: impl http_body::Body + Unpin) 
where
    <impl http_body::Body>::Data: Clone,
    <impl http_body::Body>::Error: Clone,
{
    let shared = SharedBody::new(body);
    
    for attempt in 1..=3 {
        let body_clone = shared.clone();
        match send_request(body_clone).await {
            Ok(response) => return Ok(response),
            Err(_) if attempt < 3 => continue,
            Err(e) => return Err(e),
        }
    }
}
```

### Request/Response Logging
Clone the body to log it while still forwarding to the handler:

```rust
use shared_http_body::SharedBody;

async fn log_and_forward(body: impl http_body::Body + Unpin) 
where
    <impl http_body::Body>::Data: Clone,
    <impl http_body::Body>::Error: Clone,
{
    let shared = SharedBody::new(body);
    let logger = shared.clone();
    
    // Log in background
    tokio::spawn(async move {
        let data = logger.collect().await.unwrap().to_bytes();
        log::info!("Request body: {:?}", data);
    });
    
    // Forward the original
    forward_request(shared).await;
}
```

## Requirements

- The underlying body must implement `Unpin`
- Body data type (`Body::Data`) must implement `Clone`
- Body error type (`Body::Error`) must implement `Clone`
- For thread safety, the body and its data/error types must be `Send` + `Sync`

## How It Works

`SharedBody` uses `futures_util::future::Shared` internally to share the state of frame consumption across clones. Each frame is wrapped in a `ClonableFrame` that can clone both data frames and trailer frames. This allows multiple consumers to independently consume the same body without interference.

## License

Licensed under the Apache License, Version 2.0.