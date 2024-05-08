use crate::config::ChainState;
use crate::service::proxy::ChainProxyConfig;
use async_trait::async_trait;
use http::HeaderName;
use log::info;
use pingora::prelude::*;
use std::sync::{Arc, Mutex};

pub struct ProxyApp {
    // currently we only support two clusters, maybe with different priority
    cluster_one: Arc<LoadBalancer<RoundRobin>>,
    cluster_two: Arc<LoadBalancer<RoundRobin>>,

    host_configs: Vec<ChainProxyConfig>,
    chain_state: Arc<Mutex<ChainState>>,
}

impl ProxyApp {
    pub fn new(
        host_configs: Vec<ChainProxyConfig>,
        cluster_one: Arc<LoadBalancer<RoundRobin>>,
        cluster_two: Arc<LoadBalancer<RoundRobin>>,
        chain_state: Arc<Mutex<ChainState>>,
    ) -> Self {
        ProxyApp {
            cluster_one: cluster_one.clone(),
            cluster_two: cluster_two.clone(),
            host_configs,
            chain_state: Arc::clone(&chain_state),
        }
    }
}

#[async_trait]
impl ProxyHttp for ProxyApp {
    type CTX = ();
    fn new_ctx(&self) {}

    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut ()) -> Result<Box<HttpPeer>> {
        // first check chain state of the cluster
        {
            let mut state = self.chain_state.lock().unwrap();
            let block_numbers = state.get_block_numbers();

            // Now you can iterate over the `block_numbers` HashMap reference
            for (host_name, block_number) in block_numbers {
                info!("Host: {}, Block Number: {}", host_name, block_number);
            }
        }

        let host_header = session
            .get_header(HeaderName::from_static("host"))
            .unwrap()
            .to_str()
            .unwrap();
        info!("host header: {host_header}");

        // First select healthy upstream from the balancer, and then select the best one
        let backends = self.cluster_one.backends();
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
