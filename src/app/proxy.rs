use crate::service::proxy::HostConfigPlain;
use async_trait::async_trait;
use http::HeaderName;
use log::info;
use pingora::prelude::{HttpPeer, ProxyHttp, Result, Session};

pub struct ProxyApp {
    host_configs: Vec<HostConfigPlain>,
}

impl ProxyApp {
    pub fn new(host_configs: Vec<HostConfigPlain>) -> Self {
        ProxyApp { host_configs }
    }
}

#[async_trait]
impl ProxyHttp for ProxyApp {
    type CTX = ();
    fn new_ctx(&self) {}

    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut ()) -> Result<Box<HttpPeer>> {
        let host_header = session
            .get_header(HeaderName::from_static("host"))
            .unwrap()
            .to_str()
            .unwrap();
        info!("host header: {host_header}");

        // Find the host config that matches the current state best
        let mut best_host_config = None;
        for host_config in &self.host_configs {
            // first check priority
            if best_host_config.is_none() {
                best_host_config = Some(host_config);
                continue;
            }

            if host_config.priority > best_host_config.as_ref().unwrap().priority {
                best_host_config = Some(host_config);
            }
        }

        if best_host_config.is_none() {
            log::error!("No host config found");
            panic!("No host config found");
        }

        let best_host_config = best_host_config.unwrap();
        let proxy_to = HttpPeer::new(
            best_host_config.proxy_addr.as_str(),
            best_host_config.proxy_tls,
            best_host_config.proxy_hostname.clone(),
        );
        let peer = Box::new(proxy_to);
        // log the selected peer
        info!("Selected peer: {peer}");
        Ok(peer)
    }
}
