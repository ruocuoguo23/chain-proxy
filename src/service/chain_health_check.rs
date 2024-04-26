use std::time::Duration;

use async_trait::async_trait;
use pingora::connectors::http::Connector as HttpConnector;
use pingora::http::{RequestHeader, ResponseHeader};
use pingora::lb::health_check::HealthCheck;
use pingora::lb::Backend;
use pingora::prelude::HttpPeer;
use pingora::upstreams::peer::Peer;
use pingora::{Custom, CustomCode, Error, Result};
use serde::{Deserialize, Serialize};

type Validator = Box<dyn Fn(&ResponseHeader) -> Result<()> + Send + Sync>;
type ResponseBodyValidator = Box<dyn Fn(&[u8]) -> Result<()> + Send + Sync>;

/// Define various response validators for different chain, like ethereum, bitcoin, etc.
#[derive(Debug, Serialize, Deserialize)]
struct EthJsonResponse {
    /// The key to check in the JSON response
    jsonrpc: String,
    id: u64,
    result: String,
}

fn eth_validator(body: &[u8]) -> Result<()> {
    // try to parse the JSON response
    let parsed = serde_json::from_slice(body);
    if parsed.is_err() {
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed: EthJsonResponse = parsed.unwrap();
    // check if the JSON response is valid
    if parsed.jsonrpc != "2.0" {
        Error::e_explain(Custom("invalid jsonrpc"), "during http healthcheck")
    } else {
        Ok(())
    }
}
/// HTTP health check
///
/// This health check checks if it can receive the expected HTTP(s) response from the given backend.
pub struct ChainHealthCheck {
    /// Number of successful checks to flip from unhealthy to healthy.
    pub consecutive_success: usize,
    /// Number of failed checks to flip from healthy to unhealthy.
    pub consecutive_failure: usize,
    /// How to connect to the backend.
    ///
    /// This field defines settings like the connect timeout and src IP to bind.
    /// The SocketAddr of `peer_template` is just a placeholder which will be replaced by the
    /// actual address of the backend when the health check runs.
    ///
    /// Set the `scheme` field to use HTTPs.
    pub peer_template: HttpPeer,
    /// Whether the underlying TCP/TLS connection can be reused across checks.
    ///
    /// * `false` will make sure that every health check goes through TCP (and TLS) handshakes.
    /// Established connections sometimes hide the issue of firewalls and L4 LB.
    /// * `true` will try to reuse connections across checks, this is the more efficient and fast way
    /// to perform health checks.
    pub reuse_connection: bool,
    /// The request header to send to the backend
    pub req: RequestHeader,

    /// The request body to send to the backend
    pub request_body: Option<Vec<u8>>,

    connector: HttpConnector,
    /// Optional field to define how to validate the response from the server.
    ///
    /// If not set, any response with a `200 OK` is considered a successful check.
    pub validator: Option<Validator>,

    /// Optional field to define how to validate the response body from the server.
    pub response_body_validator: Option<ResponseBodyValidator>,
}

impl ChainHealthCheck {
    /// Create a new [ChainHealthCheck] with the following default settings
    /// * connect timeout: 1 second
    /// * read timeout: 1 second
    /// * req: a GET to the `/` of the given host name
    /// * request_body: None
    /// * consecutive_success: 1
    /// * consecutive_failure: 1
    /// * reuse_connection: false
    /// * validator: `None`, any 200 response is considered successful
    pub fn new(host: &str, tls: bool, method: &str, path: &str) -> Box<Self> {
        let mut req = RequestHeader::build(method, path.as_bytes(), None).unwrap();
        req.append_header("Host", host).unwrap();
        let sni = if tls { host.into() } else { String::new() };

        let mut peer_template = HttpPeer::new("0.0.0.0:1", tls, sni);
        peer_template.options.connection_timeout = Some(Duration::from_secs(1));
        peer_template.options.read_timeout = Some(Duration::from_secs(1));

        Box::new(ChainHealthCheck {
            consecutive_success: 1,
            consecutive_failure: 1,
            peer_template,
            connector: HttpConnector::new(None),
            reuse_connection: false,
            req,
            request_body: None,
            validator: None,
            response_body_validator: None,
        })
    }

    /// Set the request body to send to the backend
    pub fn with_request_body(mut self, body: Vec<u8>) -> Box<Self> {
        self.request_body = Some(body);
        Box::new(self)
    }

    /// Set the response body validator
    pub fn with_response_body_validator(mut self, validator: ResponseBodyValidator) -> Box<Self> {
        self.response_body_validator = Some(validator);
        Box::new(self)
    }
}

#[async_trait]
impl HealthCheck for ChainHealthCheck {
    async fn check(&self, target: &Backend) -> Result<()> {
        let mut peer = self.peer_template.clone();
        peer._address = target.addr.clone();
        let session = self.connector.get_http_session(&peer).await?;

        let mut session = session.0;
        let req = Box::new(self.req.clone());
        session.write_request_header(req).await?;

        if let Some(body) = self.request_body.as_ref() {
            session
                .write_request_body(body.clone().into(), true)
                .await?;
        }

        if let Some(read_timeout) = peer.options.read_timeout {
            session.set_read_timeout(read_timeout);
        }

        session.read_response_header().await?;

        let resp = session.response_header().expect("just read");

        if let Some(validator) = self.validator.as_ref() {
            validator(resp)?;
        } else if resp.status != 200 {
            return Error::e_explain(
                CustomCode("non 200 code", resp.status.as_u16()),
                "during http healthcheck",
            );
        };

        let mut response_body = Vec::new();
        while let Some(chunk) = session.read_response_body().await? {
            // drain the body if any
            response_body.extend_from_slice(&chunk);

            if let Some(validator) = self.response_body_validator.as_ref() {
                validator(&response_body)?;
            }
        }

        if self.reuse_connection {
            let idle_timeout = peer.idle_timeout();
            self.connector
                .release_http_session(session, &peer, idle_timeout)
                .await;
        }

        Ok(())
    }

    fn health_threshold(&self, success: bool) -> usize {
        if success {
            self.consecutive_success
        } else {
            self.consecutive_failure
        }
    }
}

#[cfg(test)]
mod test {
    use pingora::protocols::l4::socket::SocketAddr;

    use super::*;

    #[tokio::test]
    async fn test_https_check() {
        // create a health check that connects to httpbin.org over HTTPS
        let chain_health_check = ChainHealthCheck::new("httpbin.org", true, "GET", "/get");

        let backend = Backend {
            addr: SocketAddr::Inet("23.23.165.157:443".parse().unwrap()),
            weight: 1,
        };

        assert!(chain_health_check.check(&backend).await.is_ok());
    }

    #[tokio::test]
    async fn test_http_custom_check() {
        let mut http_check = ChainHealthCheck::new("one.one.one.one", false, "GET", "/get");
        http_check.validator = Some(Box::new(|resp: &ResponseHeader| {
            if resp.status == 301 {
                Ok(())
            } else {
                Error::e_explain(
                    CustomCode("non 301 code", resp.status.as_u16()),
                    "during http healthcheck",
                )
            }
        }));

        let backend = Backend {
            addr: SocketAddr::Inet("1.1.1.1:80".parse().unwrap()),
            weight: 1,
        };
        http_check.check(&backend).await.unwrap();

        assert!(http_check.check(&backend).await.is_ok());
    }
}
