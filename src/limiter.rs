use std::{
  collections::HashMap,
  sync::Mutex,
  time::{Duration, Instant},
};

/// Fixed-window request counter per client. Small enough to hold in memory: only
/// token holders are counted, and that is the status prober plus an operator.
pub struct RateLimiter {
  limit: u32,
  window: Duration,
  windows: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
  pub fn new(limit: u32, window: Duration) -> Self {
    Self {
      limit,
      window,
      windows: Mutex::default(),
    }
  }

  pub fn admit(&self, client: &str) -> bool {
    let now = Instant::now();
    let mut windows = self.windows.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    windows.retain(|_, (opened, _)| now.duration_since(*opened) < self.window);

    let (_, count) = windows.entry(client.to_owned()).or_insert((now, 0));

    if *count >= self.limit {
      return false;
    }

    *count += 1;
    true
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn admits_up_to_the_limit_then_refuses() {
    let limiter = RateLimiter::new(3, Duration::from_secs(60));

    assert!((0..3).all(|_| limiter.admit("10.0.0.1")));
    assert!(!limiter.admit("10.0.0.1"));
  }

  #[test]
  fn counts_each_client_separately() {
    let limiter = RateLimiter::new(1, Duration::from_secs(60));

    assert!(limiter.admit("10.0.0.1"));
    assert!(limiter.admit("10.0.0.2"));
    assert!(!limiter.admit("10.0.0.1"));
  }

  #[test]
  fn a_new_window_admits_again() {
    let limiter = RateLimiter::new(1, Duration::from_millis(20));

    assert!(limiter.admit("10.0.0.1"));
    std::thread::sleep(Duration::from_millis(30));
    assert!(limiter.admit("10.0.0.1"));
  }
}
