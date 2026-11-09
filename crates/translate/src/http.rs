//! One way to POST JSON, shared by the services. On failure only the HTTP status or a fixed
//! sentence leaves it, so no key, URL or reply text can end up in an error.
//!
//! The browser (wasm32) build has no sockets and no TLS stack, so it carries no HTTP client:
//! there a request is handed to the page through `replay`, which sends it with `fetch`. The
//! native build uses `ureq`.

use std::time::Duration;

use crate::review::TranslateError;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type Agent = ureq::Agent;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn agent(timeout: Duration) -> Agent {
    let config = ureq::Agent::config_builder().timeout_global(Some(timeout)).build();
    config.into()
}

/// POST `body` to `url` and return the reply text. `header` is one extra header (name, value).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn post_json(agent: &Agent, url: &str, header: Option<(&str, &str)>, body: &str) -> Result<String, TranslateError> {
    let mut request = agent.post(url).header("Content-Type", "application/json");
    if let Some((name, value)) = header {
        request = request.header(name, value);
    }
    let mut response = request.send(body).map_err(|e| match e {
        ureq::Error::StatusCode(code) => TranslateError::Http(code),
        _ => TranslateError::Network,
    })?;
    response.body_mut().read_to_string().map_err(|_| TranslateError::Network)
}

/// The browser build's stand-in: it holds nothing and sends nothing.
#[cfg(target_arch = "wasm32")]
pub(crate) struct Agent;

#[cfg(target_arch = "wasm32")]
pub(crate) fn agent(_timeout: Duration) -> Agent {
    Agent
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn post_json(_agent: &Agent, url: &str, header: Option<(&str, &str)>, body: &str) -> Result<String, TranslateError> {
    crate::replay::lookup(url, header, body)
}
