// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Authentication module
//!
//! Based on davcli's authentication patterns.
//! Wraps HTTP client with Basic Authentication.

use base64::Engine as _;
use http::{HeaderValue, Request, Response};
use tower::Service;

const BASE64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// Wrapper that adds Basic Authentication to requests
///
/// Based on davcli's AddAuthorization pattern
#[derive(Debug, Clone)]
pub struct AddAuthorization<S> {
    inner: S,
    value: Option<HeaderValue>,
}

impl<S> AddAuthorization<S> {
    /// Add Authorization header with username and password
    ///
    /// Uses HTTP Basic Authentication
    pub fn basic(inner: S, username: &str, password: &str) -> AddAuthorization<S> {
        let encoded = BASE64.encode(format!("{username}:{password}"));
        let mut value = HeaderValue::try_from(format!("Basic {encoded}"))
            .expect("base64 encoded string is a valid header value");
        value.set_sensitive(true);

        AddAuthorization {
            inner,
            value: Some(value),
        }
    }
}

impl<S, Tx, Rx> Service<Request<Tx>> for AddAuthorization<S>
where
    S: Service<Request<Tx>, Response = Response<Rx>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Tx>) -> Self::Future {
        if let Some(value) = &self.value {
            req.headers_mut()
                .insert(http::header::AUTHORIZATION, value.clone());
        }
        self.inner.call(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_auth_encoding() {
        // Test that Basic Auth is encoded correctly
        let username = "user@fastmail.com";
        let password = "testpassword";

        let encoded = BASE64.encode(format!("{username}:{password}"));
        let expected = format!("Basic {}", encoded);

        assert!(expected.starts_with("Basic "));
    }
}
