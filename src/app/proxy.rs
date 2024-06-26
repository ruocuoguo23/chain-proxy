use crate::config::ChainState;
use crate::service::proxy::ChainProxyConfig;
use async_trait::async_trait;
use log::info;
use pingora::{
    protocols::ALPN,
    upstreams::peer::{HttpPeer, PeerOptions, TcpKeepalive},
    // ErrorType::HTTPStatus,
    Error,
    Custom,
    Result
};
use pingora_proxy::Session;
use pingora_proxy::ProxyHttp;
use pingora_load_balancing::selection::RoundRobin;
use pingora_load_balancing::LoadBalancer;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Default peer options to be used on every upstream connection
pub const DEFAULT_PEER_OPTIONS: PeerOptions = PeerOptions {
    verify_hostname: true,
    read_timeout: Some(Duration::from_secs(30)),
    connection_timeout: Some(Duration::from_secs(30)),
    tcp_recv_buf: Some(512 * 1024),
    tcp_keepalive: Some(TcpKeepalive {
        count: 5,
        interval: Duration::from_secs(10),
        idle: Duration::from_secs(30),
    }),
    bind_to: None,
    total_connection_timeout: Some(Duration::from_secs(5)),
    idle_timeout: None,
    write_timeout: Some(Duration::from_secs(5)),
    verify_cert: false,
    alternative_cn: None,
    alpn: ALPN::H1,
    ca: None,
    no_header_eos: false,
    h2_ping_interval: None,
    max_h2_streams: 5,
    extra_proxy_headers: BTreeMap::new(),
    curves: None,
    second_keyshare: true, // default true and noop when not using PQ curves
    tracer: None,
};

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
                let current_block_number = block_numbers.get(config.proxy_uri.as_str());
                if let None = current_block_number {
                    info!(
                        "Host: {} is not eligible, block number not found",
                        config.proxy_uri.as_str()
                    );
                    return false;
                }
                let current_block_number = current_block_number.unwrap();

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
            .max_by_key(|config| config.priority);
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

        let session = session.as_downstream_mut();
        let req = session.req_header_mut();
        let current_host = req
            .headers
            .get("host")
            .map_or("", |v| v.to_str().unwrap());
        info!("Selected request current host: {current_host}");

        // set session header to host name
        let result = req.insert_header("host", selected_cluster.proxy_hostname.as_str());
        if let Err(e) = result {
            log::error!("Failed to set host header: {e}");
        }

        // sometimes we need to set the request path to the cluster path
        req.set_uri(selected_cluster.proxy_uri.as_str().parse().unwrap());

        let proxy_to = HttpPeer::new(
            selected_cluster.proxy_addr.as_str(),
            selected_cluster.proxy_tls,
            selected_cluster.proxy_hostname.clone(),
        );
        let mut peer = Box::new(proxy_to);
        peer.options = DEFAULT_PEER_OPTIONS;

        // log the selected peer
        info!("Selected peer: {peer}");
        Ok(peer)
    }
}
