use crate::config::ChainState;
use crate::service::proxy::ChainProxyConfig;
use async_trait::async_trait;
use http::HeaderName;
use log::info;
use pingora::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct ProxyApp {
    // currently we only support two clusters, maybe with different priority
    // key is the host name, value is the cluster
    clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,

    // host configs
    host_configs: Vec<ChainProxyConfig>,

    // shared chain state
    chain_state: Arc<Mutex<ChainState>>,
}

impl ProxyApp {
    pub fn new(
        host_configs: Vec<ChainProxyConfig>,
        clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
        chain_state: Arc<Mutex<ChainState>>,
    ) -> Self {
        ProxyApp {
            clusters,
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
        // first get the chain state of the cluster
        let block_numbers = {
            let state = self.chain_state.lock().unwrap();
            state.get_block_numbers().clone()
        };

        // determine the max block number
        let max_block_number = block_numbers.iter().map(|(_, v)| v).max().unwrap();

        // if block number is not within the range, we should filter out the unhealthy ones
        let block_range = self.host_configs[0].block_gap;

        info!(
            "Max block number: {}, current block range: {}",
            max_block_number, block_range
        );

        let eligible_clusters: Vec<_> = self
            .host_configs
            .iter()
            .filter(|config| {
                let current_block_number = block_numbers.get(config.proxy_uri.as_str()).unwrap();
                if max_block_number - current_block_number > block_range {
                    info!(
                        "Host: {} is not eligible, block number: {}",
                        config.proxy_uri.as_str(),
                        current_block_number
                    );
                    return false;
                }
                true
            })
            .collect();

        // probably no case will reach here
        if eligible_clusters.is_empty() {
            log::error!("No eligible cluster found");
            return Error::e_explain(Custom("No eligible cluster found"), "proxy error");
        }

        // select the best cluster based on priority
        // we may combine with other information like healthy or latency
        let selected_cluster_result = eligible_clusters
            .into_iter()
            .min_by_key(|config| config.priority);
        let selected_cluster = match selected_cluster_result {
            None => {
                log::error!("No cluster selected");
                return Error::e_explain(Custom("No cluster selected"), "proxy error");
            }
            Some(cluster) => cluster,
        };

        // check the cluster
        let cluster = self.clusters.get(selected_cluster.proxy_uri.as_str());
        if let None = cluster {
            log::error!("Cluster not found");
            return Error::e_explain(Custom("Cluster not found"), "proxy error");
        }

        let host_header = session
            .get_header(HeaderName::from_static("host"))
            .unwrap()
            .to_str()
            .unwrap();
        info!("host header: {host_header}");

        let proxy_to = HttpPeer::new(
            selected_cluster.proxy_addr.as_str(),
            selected_cluster.proxy_tls,
            selected_cluster.proxy_hostname.clone(),
        );
        let peer = Box::new(proxy_to);

        // log the selected peer
        info!("Selected peer: {peer}");
        Ok(peer)
    }
}
