//! Requests for a browser, which cannot wait for a reply inside a plain function call.
//!
//! The review code asks a service for translations and expects the answer back from the same
//! call. A browser can only send a request and be told later. This module bridges the two by
//! replaying: [`lookup`] answers from replies already received, or records the request it
//! needs and returns [`TranslateError::Pending`]. [`drive`] then sends that request, stores the
//! reply and runs the review again from the start; it asks for the same requests in the same
//! order of content, so each round gets one request further, until nothing is missing.
//!
//! Replies are kept only while [`drive`] runs, and are dropped when it ends. A request holds the
//! API key (Google takes it in the URL), so [`Pending`] never prints its contents.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;

use crate::review::TranslateError;

/// The most requests one run may make. A run sends at most 100,000 characters in batches of a few
/// thousand, so this is far above any real run; it only stops a loop that cannot end.
const MAX_ROUNDS: usize = 2000;

/// A request the browser has to send before the run can go on.
#[derive(Clone, PartialEq, Eq)]
pub struct Pending {
    pub url: String,
    /// One extra header, (name, value).
    pub header: Option<(String, String)>,
    pub body: String,
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pending(..)")
    }
}

#[derive(Default)]
struct State {
    replies: HashMap<String, String>,
    pending: Option<Pending>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn key(url: &str, header: Option<(&str, &str)>, body: &str) -> String {
    let (name, value) = header.unwrap_or(("", ""));
    format!("{url}\n{name}\n{value}\n{body}")
}

/// The reply to this request if it has been received; otherwise remember the request and return
/// [`TranslateError::Pending`].
pub fn lookup(url: &str, header: Option<(&str, &str)>, body: &str) -> Result<String, TranslateError> {
    STATE
        .with(|state| {
            let mut state = state.try_borrow_mut().ok()?;
            let k = key(url, header, body);
            if let Some(reply) = state.replies.get(&k) {
                return Some(Ok(reply.clone()));
            }
            state.pending = Some(Pending { url: url.to_owned(), header: header.map(|(n, v)| (n.to_owned(), v.to_owned())), body: body.to_owned() });
            Some(Err(TranslateError::Pending))
        })
        .unwrap_or(Err(TranslateError::Network))
}

/// The request the last [`lookup`] needed, if any. It is taken: asking again returns `None`.
pub fn take_pending() -> Option<Pending> {
    STATE.with(|state| state.try_borrow_mut().ok().and_then(|mut s| s.pending.take()))
}

/// Remember the reply to `request`, so the next [`lookup`] of it succeeds.
pub fn store(request: &Pending, reply: String) {
    STATE.with(|state| {
        if let Ok(mut s) = state.try_borrow_mut() {
            let header = request.header.as_ref().map(|(n, v)| (n.as_str(), v.as_str()));
            s.replies.insert(key(&request.url, header, &request.body), reply);
        }
    });
}

/// Forget every reply and any waiting request.
pub fn clear() {
    STATE.with(|state| {
        if let Ok(mut s) = state.try_borrow_mut() {
            s.replies.clear();
            s.pending = None;
        }
    });
}

/// Run `run` until it no longer needs a request. `send` performs one request and returns the
/// reply text; its error ends the run. Replies are dropped when this returns.
pub async fn drive<T, R, F, Fut>(mut run: R, mut send: F) -> Result<T, TranslateError>
where
    R: FnMut() -> Result<T, TranslateError>,
    F: FnMut(Pending) -> Fut,
    Fut: Future<Output = Result<String, TranslateError>>,
{
    clear();
    let mut result = Err(TranslateError::Network);
    for _ in 0..MAX_ROUNDS {
        match run() {
            Err(TranslateError::Pending) => {
                let Some(request) = take_pending() else { break };
                match send(request.clone()).await {
                    Ok(reply) => store(&request, reply),
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            other => {
                result = other;
                break;
            }
        }
    }
    clear();
    result
}

#[cfg(test)]
mod tests {
    use std::task::{Context, Poll, Waker};

    use super::*;

    fn block_on<F: Future>(f: F) -> F::Output {
        let mut f = Box::pin(f);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    fn two_requests() -> Result<String, TranslateError> {
        let a = lookup("https://svc", None, "one")?;
        let b = lookup("https://svc", Some(("k", "v")), "two")?;
        Ok(format!("{a}+{b}"))
    }

    #[test]
    fn a_run_is_repeated_until_every_reply_is_in() {
        let mut sent = Vec::new();
        let out = block_on(drive(two_requests, |p: Pending| {
            sent.push(p.body.clone());
            async move { Ok(format!("r-{}", p.body)) }
        }));
        assert_eq!(out, Ok("r-one+r-two".to_owned()));
        assert_eq!(sent, ["one", "two"]);
    }

    #[test]
    fn a_failed_request_ends_the_run_and_leaves_nothing_behind() {
        let out = block_on(drive(two_requests, |_: Pending| async { Err::<String, _>(TranslateError::Http(403)) }));
        assert_eq!(out, Err(TranslateError::Http(403)));
        // Nothing from the failed run is kept: the same request is needed again.
        assert_eq!(lookup("https://svc", None, "one"), Err(TranslateError::Pending));
        clear();
    }

    #[test]
    fn a_reply_is_found_by_its_whole_request() {
        clear();
        let p = Pending { url: "u".into(), header: Some(("h".into(), "v".into())), body: "b".into() };
        store(&p, "answer".into());
        assert_eq!(lookup("u", Some(("h", "v")), "b"), Ok("answer".to_owned()));
        assert_eq!(lookup("u", Some(("h", "other")), "b"), Err(TranslateError::Pending));
        assert_eq!(lookup("u", None, "b"), Err(TranslateError::Pending));
        clear();
    }

    #[test]
    fn a_waiting_request_never_prints_the_key() {
        let p = Pending { url: "https://x?key=SECRET-KEY".into(), header: Some(("x-goog-api-key".into(), "SECRET-KEY".into())), body: "b".into() };
        assert!(!format!("{p:?}").contains("SECRET"));
    }

    #[test]
    fn a_run_that_needs_endlessly_new_requests_stops() {
        let mut n = 0_usize;
        let out: Result<(), TranslateError> = block_on(drive(
            || {
                n += 1;
                lookup("u", None, &n.to_string()).map(|_| ())
            },
            |_: Pending| async { Ok("x".to_owned()) },
        ));
        assert_eq!(out, Err(TranslateError::Network));
        assert!(n <= MAX_ROUNDS);
    }

    #[test]
    fn pending_without_a_request_is_an_error_not_a_loop() {
        let out = block_on(drive(|| Err::<(), _>(TranslateError::Pending), |_: Pending| async { Ok(String::new()) }));
        assert_eq!(out, Err(TranslateError::Network));
    }
}
