use async_trait::async_trait;
use http::uri::Uri;
use pingora_proxy::{ProxyHttp, Session};
use pingora::{upstreams::peer::HttpPeer, Custom, Error, Result};

use crate::app::proxy_base::ProxyCtx;
use crate::config::UnifyProxyConfig;
use log::info;

/// Unified proxy application
pub struct UnifyProxyApp {
    config: UnifyProxyConfig,
}

impl UnifyProxyApp {
    /// Create a new UnifyProxyApp
    pub fn new(config: UnifyProxyConfig) -> Self {
        UnifyProxyApp { config }
    }
}

#[async_trait]
impl ProxyHttp for UnifyProxyApp {
    type CTX = ProxyCtx;

    fn new_ctx(&self) -> Self::CTX {
        ProxyCtx {
            request_body: Vec::new(),
            response_body: Vec::new(),
        }
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let req = session.req_header_mut();
        let path = req.uri.path();

        // Split path into segments
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if segments.len() < 2 {
            log::error!("Invalid request path: {}", path);
            return Error::e_explain(Custom("Invalid request path"), "proxy error");
        }

        // Extract chain_type and chain_name
        let chain_type = segments[0];
        let chain_name = segments[1];

        // Lookup port
        let port = self.config.get_port(chain_type, chain_name);

        if port.is_none() {
            log::error!("No matching chain found for {}/{}", chain_type, chain_name);
            return Error::e_explain(
                Custom("No matching chain found"),
                "proxy error",
            );
        }

        let port = port.unwrap();

        // Construct new URI
        let new_path = if segments.len() > 2 {
            format!("/{}", segments[2..].join("/"))
        } else {
            path.to_string() // No change, jsonrpc path
        };

        let uri = Uri::builder().path_and_query(&new_path).build();
        if uri.is_err() {
            log::error!("Invalid URI: {}", new_path);
            return Error::e_explain(Custom("Invalid URI"), "proxy error");
        }

        req.set_uri(uri.unwrap());

        let host = "127.0.0.1";

        let peer = Box::new(HttpPeer::new((host, port), false, host.to_string()));
        req.insert_header("Host", host).ok();

        Ok(peer)
    }
}
