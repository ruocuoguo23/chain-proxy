use crate::service::proxy::ChainProxyConfig;
use async_trait::async_trait;
use http::HeaderName;
use log::info;
use pingora::prelude::*;
use std::sync::Arc;

pub struct ProxyApp {
    balancer: Arc<LoadBalancer<RoundRobin>>,
    host_configs: Vec<ChainProxyConfig>,
}

impl ProxyApp {
    pub fn new(
        host_configs: Vec<ChainProxyConfig>,
        balancer: Arc<LoadBalancer<RoundRobin>>,
    ) -> Self {
        ProxyApp {
            balancer,
            host_configs,
        }
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

        // First select healthy upstream from the balancer, and then select the best one
        let backends = self.balancer.backends();
        let peers = backends.get_backend();
        let healthy_peers = peers
            .iter()
            .filter(|p| backends.ready(p))
            .collect::<Vec<_>>();

        if healthy_peers.is_empty() {
            log::error!("No healthy upstream found");
            panic!("No healthy upstream found");
        }

        // Find the host config that matches the current state best
        let mut best_host_config = None;
        for host_config in &self.host_configs {
            let mut is_healthy = false;
            // first check health
            for peer in &healthy_peers {
                let peer_addr = peer.addr.as_inet();
                let peer_addr = peer_addr.unwrap().to_string();
                if peer_addr == host_config.proxy_addr {
                    is_healthy = true;
                    break;
                }
            }
            if !is_healthy {
                continue;
            }

            // then check priority
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
