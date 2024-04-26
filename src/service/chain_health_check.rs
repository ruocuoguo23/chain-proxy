use async_trait::async_trait;
use pingora::connectors::http::Connector as HttpConnector;
use pingora::http::{RequestHeader, ResponseHeader};
use pingora::lb::health_check::HealthCheck;
use pingora::lb::Backend;
use pingora::prelude::HttpPeer;
use pingora::upstreams::peer::Peer;
use pingora::{CustomCode, Error, Result};
use std::time::Duration;

type Validator = Box<dyn Fn(&ResponseHeader) -> Result<()> + Send + Sync>;

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
    connector: HttpConnector,
    /// Optional field to define how to validate the response from the server.
    ///
    /// If not set, any response with a `200 OK` is considered a successful check.
    pub validator: Option<Validator>,
    /// Sometimes the health check endpoint lives one a different port than the actual backend.
    /// Setting this option allows the health check to perform on the given port of the backend IP.
    pub port_override: Option<u16>,
}

impl ChainHealthCheck {
    /// Create a new [ChainHealthCheck] with the following default settings
    /// * connect timeout: 1 second
    /// * read timeout: 1 second
    /// * req: a GET to the `/` of the given host name
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
            validator: None,
            port_override: None,
        })
    }
}

#[async_trait]
impl HealthCheck for ChainHealthCheck {
    async fn check(&self, target: &Backend) -> Result<()> {
        let mut peer = self.peer_template.clone();
        peer._address = target.addr.clone();
        if let Some(port) = self.port_override {
            peer._address.set_port(port);
        }
        let session = self.connector.get_http_session(&peer).await?;

        let mut session = session.0;
        let req = Box::new(self.req.clone());
        session.write_request_header(req).await?;

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

        while session.read_response_body().await?.is_some() {
            // drain the body if any
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
    use super::*;
    use pingora::protocols::l4::socket::SocketAddr;

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
