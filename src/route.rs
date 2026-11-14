use actix_web::{HttpRequest, HttpResponse, http::header, web};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::{
  check::{Check, run_all},
  limiter::RateLimiter,
  report::Report,
};

const CACHE_FOR: Duration = Duration::from_secs(5);
const REQUESTS_PER_WINDOW: u32 = 10;
const RATE_WINDOW: Duration = Duration::from_secs(60);

/// A service's health checks plus the state the details route shares across
/// actix workers. Build it once and register it as `web::Data<Health>`.
pub struct Health {
  service: String,
  version: String,
  probe_token: String,
  checks: Vec<Box<dyn Check>>,
  last_report: Mutex<Option<(Instant, Report)>>,
  limiter: RateLimiter,
}

impl Health {
  /// An empty `probe_token` leaves the details route answering 404 to everyone.
  pub fn new(service: &str, version: &str, probe_token: &str) -> Self {
    Self {
      service: service.to_owned(),
      version: version.to_owned(),
      probe_token: probe_token.to_owned(),
      checks: Vec::new(),
      last_report: Mutex::default(),
      limiter: RateLimiter::new(REQUESTS_PER_WINDOW, RATE_WINDOW),
    }
  }

  pub fn check(mut self, check: impl Check + 'static) -> Self {
    self.checks.push(Box::new(check));
    self
  }

  fn authorizes(&self, req: &HttpRequest) -> bool {
    let presented = req
      .headers()
      .get(header::AUTHORIZATION)
      .and_then(|value| value.to_str().ok())
      .and_then(|value| value.strip_prefix("Bearer "));

    match presented {
      Some(token) if !self.probe_token.is_empty() => constant_time_eq(token.as_bytes(), self.probe_token.as_bytes()),
      _ => false,
    }
  }

  /// The lock is held while checks run, so concurrent callers wait for one round
  /// instead of each starting their own.
  async fn report(&self) -> Report {
    let mut last_report = self.last_report.lock().await;

    if let Some((taken, report)) = last_report.as_ref()
      && taken.elapsed() < CACHE_FOR
    {
      return report.clone();
    }

    let report = Report::new(&self.service, &self.version, run_all(&self.checks).await);
    *last_report = Some((Instant::now(), report.clone()));

    report
  }
}

pub async fn details(health: web::Data<Health>, req: HttpRequest) -> HttpResponse {
  if !health.authorizes(&req) {
    return HttpResponse::NotFound().finish();
  }

  let client = req
    .connection_info()
    .realip_remote_addr()
    .unwrap_or_default()
    .to_owned();

  if !health.limiter.admit(&client) {
    return HttpResponse::TooManyRequests().finish();
  }

  let report = health.report().await;
  let mut response = if report.is_failed() {
    HttpResponse::ServiceUnavailable()
  } else {
    HttpResponse::Ok()
  };

  response.insert_header((header::CACHE_CONTROL, "no-store")).json(report)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
  left.len() == right.len() && left.iter().zip(right).fold(0, |diff, (l, r)| diff | (l ^ r)) == 0
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::check::{Failure, Reading, tests::Stub};
  use actix_web::{App, http::StatusCode, test};
  use std::sync::atomic::Ordering;

  const TOKEN: &str = "probe-secret";

  async fn call(health: Health, authorization: Option<&str>) -> (StatusCode, Option<serde_json::Value>) {
    let app = test::init_service(
      App::new()
        .app_data(web::Data::new(health))
        .route("/healthcheck/details", web::get().to(details)),
    )
    .await;
    let mut request = test::TestRequest::get().uri("/healthcheck/details");

    if let Some(value) = authorization {
      request = request.insert_header((header::AUTHORIZATION, value));
    }

    let response = test::call_service(&app, request.to_request()).await;
    let status = response.status();
    let body = test::read_body(response).await;

    (status, serde_json::from_slice(&body).ok())
  }

  fn healthy() -> Health {
    Health::new("passwords", "abc123", TOKEN).check(Stub::answering("database", Ok(Reading::Responded)))
  }

  #[actix_web::test]
  async fn hides_the_route_without_a_token() {
    let (status, body) = call(healthy(), None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_none());
  }

  #[actix_web::test]
  async fn hides_the_route_from_a_wrong_token() {
    let (status, _) = call(healthy(), Some("Bearer not-the-token")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
  }

  #[actix_web::test]
  async fn an_unset_token_disables_the_route() {
    let health = Health::new("passwords", "abc123", "");

    let (status, _) = call(health, Some("Bearer ")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
  }

  #[actix_web::test]
  async fn answers_the_report_to_the_token_holder() {
    let (status, body) = call(healthy(), Some("Bearer probe-secret")).await;
    let body = body.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["service"], "passwords");
    assert_eq!(body["version"], "abc123");
    assert_eq!(body["status"], "ok");
    assert_eq!(body["checks"]["database"]["status"], "ok");
  }

  #[actix_web::test]
  async fn a_failed_check_answers_503_with_the_body() {
    let health = healthy().check(Stub::answering("redis", Err(Failure::CONNECT)));

    let (status, body) = call(health, Some("Bearer probe-secret")).await;
    let body = body.unwrap();

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "failed");
    assert_eq!(body["checks"]["redis"]["error"], "connect");
  }

  #[actix_web::test]
  async fn refuses_a_client_past_the_rate_limit() {
    let app = test::init_service(
      App::new()
        .app_data(web::Data::new(healthy()))
        .route("/healthcheck/details", web::get().to(details)),
    )
    .await;
    let request = || {
      test::TestRequest::get()
        .uri("/healthcheck/details")
        .insert_header((header::AUTHORIZATION, "Bearer probe-secret"))
        .insert_header(("x-forwarded-for", "203.0.113.7"))
        .to_request()
    };

    for _ in 0..REQUESTS_PER_WINDOW {
      assert_eq!(test::call_service(&app, request()).await.status(), StatusCode::OK);
    }

    assert_eq!(
      test::call_service(&app, request()).await.status(),
      StatusCode::TOO_MANY_REQUESTS
    );
  }

  #[actix_web::test]
  async fn serves_repeat_calls_from_the_cached_round() {
    let stub = Stub::answering("database", Ok(Reading::Responded));
    let calls = stub.calls.clone();
    let health = web::Data::new(Health::new("passwords", "abc123", TOKEN).check(stub));

    health.report().await;
    health.report().await;

    assert_eq!(calls.load(Ordering::SeqCst), 1);
  }
}
