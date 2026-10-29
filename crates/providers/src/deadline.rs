//! Database-relative operation budgets measured by a suspend-inclusive clock.
use std::{future::Future, pin::pin, task::Poll, time::Duration};

#[derive(Clone, Copy, Debug)]
pub struct SuspendInstant(Duration);

#[derive(Clone, Copy, Debug)]
pub struct Deadline(Duration);

#[derive(Debug)]
pub struct DeadlineExpired;

impl SuspendInstant {
    /// A missing or unsupported clock is never treated as a live budget.
    pub fn now() -> Option<Self> {
        clock().map(Self)
    }

    pub fn deadline(self, duration: Duration) -> Option<Deadline> {
        self.0.checked_add(duration).map(Deadline)
    }
}

impl Deadline {
    pub fn remaining(self) -> Option<Duration> {
        self.remaining_at(clock()?)
    }

    fn remaining_at(self, now: Duration) -> Option<Duration> {
        self.0.checked_sub(now).filter(|value| !value.is_zero())
    }

    /// Check before every poll and after readiness. Runtime timers only wake the
    /// future; the suspend-inclusive clock decides whether it may continue.
    /// A request already handed to a transport may still reach its provider.
    pub async fn run<F: Future>(self, future: F) -> Result<F::Output, DeadlineExpired> {
        self.run_with_clock(future, clock).await
    }

    async fn run_with_clock<F: Future, C: Fn() -> Option<Duration>>(
        self,
        future: F,
        now: C,
    ) -> Result<F::Output, DeadlineExpired> {
        let mut future = pin!(future);
        let mut wake = pin!(tokio::time::sleep(Duration::from_millis(100)));
        std::future::poll_fn(|cx| {
            if now().and_then(|time| self.remaining_at(time)).is_none() {
                return Poll::Ready(Err(DeadlineExpired));
            }
            if let Poll::Ready(value) = future.as_mut().poll(cx) {
                return if now().and_then(|time| self.remaining_at(time)).is_some() {
                    Poll::Ready(Ok(value))
                } else {
                    Poll::Ready(Err(DeadlineExpired))
                };
            }
            if wake.as_mut().poll(cx).is_ready() {
                wake.as_mut()
                    .reset(tokio::time::Instant::now() + Duration::from_millis(100));
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
        .await
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn clock() -> Option<Duration> {
    #[cfg(target_os = "macos")]
    let id = libc::CLOCK_MONOTONIC_RAW;
    #[cfg(target_os = "linux")]
    let id = libc::CLOCK_BOOTTIME;
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `value` is a valid writable timespec and the platform clock ID is
    // suspend-inclusive. Any system error fails the operation closed.
    if unsafe { libc::clock_gettime(id, &mut value) } != 0
        || value.tv_sec < 0
        || !(0..1_000_000_000).contains(&value.tv_nsec)
    {
        return None;
    }
    Some(Duration::new(value.tv_sec as u64, value.tv_nsec as u32))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn clock() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };

    #[tokio::test]
    async fn expired_after_pause_is_not_polled_again() {
        let time = Arc::new(AtomicU64::new(0));
        let polled_again = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(tokio::sync::Notify::new());
        let running = {
            let time = time.clone();
            let polled_again = polled_again.clone();
            let wake = wake.clone();
            tokio::spawn(async move {
                Deadline(Duration::from_secs(5))
                    .run_with_clock(
                        async {
                            wake.notified().await;
                            polled_again.store(true, Ordering::SeqCst);
                        },
                        || Some(Duration::from_secs(time.load(Ordering::SeqCst))),
                    )
                    .await
            })
        };
        tokio::task::yield_now().await;
        time.store(10, Ordering::SeqCst);
        wake.notify_one();
        assert!(running.await.unwrap().is_err());
        assert!(!polled_again.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn missing_clock_fails_closed_before_polling() {
        let polled = AtomicBool::new(false);
        let result = Deadline(Duration::from_secs(5))
            .run_with_clock(async { polled.store(true, Ordering::SeqCst) }, || None)
            .await;
        assert!(result.is_err());
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn elapsed_budget_prevents_a_new_loopback_transport_poll() {
        use axum::{Json, Router, routing::post};
        use std::sync::atomic::AtomicUsize;
        let writes = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/probe",
            post({
                let writes = writes.clone();
                move || {
                    let writes = writes.clone();
                    async move {
                        writes.fetch_add(1, Ordering::SeqCst);
                        Json(serde_json::json!({"id":"local"}))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = crate::Adapter::fixture(&origin).unwrap();
        let request = || {
            adapter
                .request(reqwest::Method::POST, "https://fixture.invalid/probe", None)
                .unwrap()
        };
        adapter.send(request(), true).await.unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);

        let time = Arc::new(AtomicU64::new(0));
        let wake = Arc::new(tokio::sync::Notify::new());
        let waiting = {
            let time = time.clone();
            let wake = wake.clone();
            tokio::spawn(async move {
                Deadline(Duration::from_secs(5))
                    .run_with_clock(
                        async {
                            wake.notified().await;
                            let request = adapter
                                .request(
                                    reqwest::Method::POST,
                                    "https://fixture.invalid/probe",
                                    None,
                                )
                                .unwrap();
                            adapter.send(request, true).await
                        },
                        || Some(Duration::from_secs(time.load(Ordering::SeqCst))),
                    )
                    .await
            })
        };
        tokio::task::yield_now().await;
        time.store(10, Ordering::SeqCst);
        wake.notify_one();
        assert!(waiting.await.unwrap().is_err());
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
