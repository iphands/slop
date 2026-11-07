//! # qbots::jev::transport — the seam between the hub and the socket (Plan 72 T5)
//!
//! The hub's workers send through a [`DecisionTransport`]. Production uses [`HttpTransport`]
//! (`net::post_json`); the hub's tests use a scripted transport under a paused tokio clock.
//! Without this seam the fake-clock tests could not drive a worker at all.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use super::net::{post_json, Endpoint, PostError};

/// A boxed, `Send` future — the object-safe shape of an async method, without `async_trait`.
pub type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Sends one rendered `/v1/systemone` request body and returns the 200 response body.
pub trait DecisionTransport: Send + Sync {
    /// Send `body`; the transport owns the deadline.
    fn post(&self, body: String) -> BoxFut<Result<Vec<u8>, PostError>>;
}

/// The production transport: one hand-rolled HTTP/1.1 `POST` per request.
#[derive(Clone, Debug)]
pub struct HttpTransport {
    /// Where.
    pub endpoint: Endpoint,
    /// The hard client-side deadline (wraps resolve + connect + write + read).
    pub deadline: Duration,
}

impl DecisionTransport for HttpTransport {
    fn post(&self, body: String) -> BoxFut<Result<Vec<u8>, PostError>> {
        let me = self.clone();
        Box::pin(async move { post_json(&me.endpoint, "/v1/systemone", &body, me.deadline).await })
    }
}
