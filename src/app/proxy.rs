use crate::config::ChainState;
use crate::service::proxy::ChainProxyConfig;
use async_trait::async_trait;
use log::{info, debug};
use pingora::{
    protocols::ALPN,
    upstreams::peer::{HttpPeer, PeerOptions},
    // ErrorType::HTTPStatus,
    Error,
    Custom,
    Result
};
use pingora::protocols::l4::ext::TcpKeepalive;
use pingora_proxy::Session;
use pingora_proxy::ProxyHttp;
use pingora_load_balancing::selection::RoundRobin;
use pingora_load_balancing::LoadBalancer;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rand::seq::SliceRandom;
use rand::thread_rng;
use crate::metrics::inc_proxy_result_counter;

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
    dscp: None,
    tcp_fast_open: false,
};

pub struct ProxyApp {
    chain_name: String,

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
        chain_name: String,
        host_configs: Vec<ChainProxyConfig>,
        clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
        chain_state: Arc<Mutex<ChainState>>,
    ) -> Self {
        ProxyApp {
            chain_name,
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
        let max_block_number = block_numbers.iter().map(|(_, v)| v).max().unwrap_or(&0);
        if max_block_number == &0 {
            log::error!("No block number found");
            return Error::e_explain(Custom("No block number found, maybe health check is unavailable or system is starting"), "proxy error");
        }

        // if block number is not within the range, we should filter out the unhealthy ones
        let block_range = self.host_configs[0].block_gap;

        debug!(
            "Max block number: {}, current block range: {}",
            max_block_number, block_range
        );

        // filter out the eligible clusters by block number
        // and group them by priority
        let mut clusters_by_priority: HashMap<i32, Vec<&ChainProxyConfig>> = HashMap::new();
        for config in self.host_configs.iter() {
            let current_block_number = block_numbers.get(config.proxy_uri.as_str());
            if let None = current_block_number {
                debug!(
                    "Host: {} is not eligible, block number not found",
                    config.proxy_uri.as_str()
                );
                continue;
            }
            let current_block_number = current_block_number.unwrap();

            if max_block_number - current_block_number > block_range {
                info!(
                    "Host: {} is not eligible, block number: {}",
                    config.proxy_uri.as_str(),
                    current_block_number
                );
                continue;
            }

            let entry = clusters_by_priority.entry(config.priority).or_insert(Vec::new());
            entry.push(config);
        }

        if clusters_by_priority.is_empty() {
            log::error!("No eligible cluster found");
            return Error::e_explain(Custom("No eligible cluster found"), "proxy error");
        }

        // Find the highest priority clusters
        let max_priority = clusters_by_priority.keys().max().unwrap();
        let highest_priority_clusters = clusters_by_priority.get(max_priority).unwrap();

        // Select a cluster from the highest priority clusters
        let selected_cluster = if highest_priority_clusters.len() == 1 {
            highest_priority_clusters[0]
        } else {
            // Random selection
            let mut rng = thread_rng();
            highest_priority_clusters.choose(&mut rng).unwrap()

            // if you want to use round robin selection, you can add here
        };

        // check the cluster
        let cluster = self.clusters.get(selected_cluster.proxy_uri.as_str());
        if let None = cluster {
            log::error!("Cluster not found");
            return Error::e_explain(Custom("Cluster not found"), "proxy error");
        }

        let session = session.as_downstream_mut();
        let req = session.req_header_mut();

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
        debug!("Selected peer: {peer}");
        Ok(peer)
    }

    async fn logging(
        &self,
        session: &mut Session,
        _e: Option<&Error>,
        ctx: &mut Self::CTX,
    ) {
        let response_code = session
            .response_written()
            .map_or(0, |resp| resp.status.as_u16());

        if let Some(e) = _e {
            info!(
                "{} response code: {response_code}, error: {e}",
                self.request_summary(session, ctx)
            );
        } else if response_code != 200 {
            info!(
                "{} response code: {response_code}",
                self.request_summary(session, ctx)
            );
        }

        let session = session.as_downstream();
        let req = session.req_header();
        if let Some(host) = req.headers.get("host") {
            let host = host.to_str().unwrap_or("unknown");

            inc_proxy_result_counter(
                self.chain_name.as_str(),
                host,
                response_code.to_string().as_str(),
                req.method.as_str(),
            );
        }
    }
}
