use std::time::Duration;
use tokio::{
  io::{AsyncBufReadExt, BufReader},
  net::TcpStream,
};

use crate::check::{Check, Failure, Reading};

/// Faktory greets every new connection with `+HI {...}` before any
/// authentication, so reading the greeting proves the server is up without
/// needing its password.
pub struct Faktory {
  address: String,
}

impl Faktory {
  /// Takes the service's `FAKTORY_URL`, e.g. `tcp://:password@host:7419`.
  pub fn new(url: &str) -> Self {
    Self {
      address: socket_address(url),
    }
  }
}

fn socket_address(url: &str) -> String {
  let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
  let without_credentials = without_scheme.rsplit_once('@').map_or(without_scheme, |(_, host)| host);

  without_credentials.trim_end_matches('/').to_owned()
}

#[async_trait::async_trait]
impl Check for Faktory {
  fn name(&self) -> &str {
    "faktory"
  }

  fn timeout(&self) -> Duration {
    Duration::from_millis(500)
  }

  async fn probe(&self) -> Result<Reading, Failure> {
    let stream = TcpStream::connect(&self.address).await.map_err(|_| Failure::CONNECT)?;
    let mut greeting = String::new();

    BufReader::new(stream)
      .read_line(&mut greeting)
      .await
      .map_err(|_| Failure::CONNECT)?;

    if greeting.starts_with("+HI") {
      Ok(Reading::Responded)
    } else {
      Err(Failure::UNEXPECTED_REPLY)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::{io::AsyncWriteExt, net::TcpListener};

  async fn server_greeting_with(greeting: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
      let (mut socket, _) = listener.accept().await.unwrap();
      socket.write_all(greeting.as_bytes()).await.unwrap();
    });

    format!("tcp://{address}")
  }

  #[test]
  fn reads_the_socket_address_out_of_a_faktory_url() {
    assert_eq!(
      socket_address("tcp://localdev-faktory-1:7419"),
      "localdev-faktory-1:7419"
    );
    assert_eq!(
      socket_address("tcp://:s3cret@faktory.railway.internal:7419"),
      "faktory.railway.internal:7419"
    );
    assert_eq!(socket_address("faktory:7419"), "faktory:7419");
  }

  #[tokio::test]
  async fn a_hi_greeting_is_healthy() {
    let url = server_greeting_with("+HI {\"v\":2}\r\n").await;

    assert_eq!(Faktory::new(&url).probe().await, Ok(Reading::Responded));
  }

  #[tokio::test]
  async fn any_other_greeting_is_unexpected() {
    let url = server_greeting_with("-ERR nope\r\n").await;

    assert_eq!(Faktory::new(&url).probe().await, Err(Failure::UNEXPECTED_REPLY));
  }

  #[tokio::test]
  async fn nothing_listening_fails_to_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let probe = Faktory::new(&format!("tcp://{address}")).probe().await;

    assert_eq!(probe, Err(Failure::CONNECT));
  }
}
