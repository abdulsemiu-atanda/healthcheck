use reqwest::Client;
use std::time::Duration;

use crate::check::{Check, Failure, Reading};

/// A service reachable only from the private network, vouched for by a caller
/// that can reach it: a `GET` that answers 2xx is up.
pub struct Http {
  name: String,
  url: String,
  client: Client,
}

impl Http {
  /// `url` is the full address to probe, usually the service's bare
  /// `/healthcheck`. The client has no TLS, since private-network hosts are
  /// plain http.
  pub fn new(name: &str, url: &str) -> Self {
    Self {
      name: name.to_owned(),
      url: url.to_owned(),
      client: Client::new(),
    }
  }
}

#[async_trait::async_trait]
impl Check for Http {
  fn name(&self) -> &str {
    &self.name
  }

  fn timeout(&self) -> Duration {
    Duration::from_secs(1)
  }

  async fn probe(&self) -> Result<Reading, Failure> {
    let response = self.client.get(&self.url).send().await.map_err(|_| Failure::CONNECT)?;

    if response.status().is_success() {
      Ok(Reading::Responded)
    } else {
      Err(Failure::UNEXPECTED_REPLY)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
  };

  /// A server that answers one request with `status_line` and hangs up
  async fn answering(status_line: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
      let (mut stream, _) = listener.accept().await.unwrap();
      let mut request = [0; 1024];
      let _ = stream.read(&mut request).await;
      let response = format!("HTTP/1.1 {status_line}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");

      stream.write_all(response.as_bytes()).await.unwrap();
    });

    format!("http://{address}/healthcheck")
  }

  #[tokio::test]
  async fn a_success_status_responded() {
    let url = answering("200 OK").await;

    assert_eq!(Http::new("metadata", &url).probe().await, Ok(Reading::Responded));
  }

  #[tokio::test]
  async fn an_error_status_is_an_unexpected_reply() {
    let url = answering("503 Service Unavailable").await;

    assert_eq!(
      Http::new("metadata", &url).probe().await,
      Err(Failure::UNEXPECTED_REPLY)
    );
  }

  #[tokio::test]
  async fn nothing_listening_fails_to_connect() {
    let check = Http::new("metadata", "http://127.0.0.1:1/healthcheck");

    assert_eq!(check.probe().await, Err(Failure::CONNECT));
  }

  #[test]
  fn reports_under_the_name_it_is_given() {
    assert_eq!(Http::new("credentials", "http://localhost").name(), "credentials");
  }
}
