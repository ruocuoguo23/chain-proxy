use crate::app::proxy;
use crate::config::ChainState;
use crate::service::chain_health_check::ChainHealthCheck;
use pingora_load_balancing::{
    selection::{BackendIter, BackendSelection, RoundRobin},
    LoadBalancer
};
use pingora_proxy::http_proxy_service;
use pingora::{
    server::configuration::ServerConf, services::background::{GenBackgroundService, background_service},
    services::Service,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct ChainProxyConfig {
    pub proxy_addr: String,
    pub proxy_tls: bool,
    pub proxy_hostname: String,
    pub proxy_uri: String,
    // current proxy priority, the higher the better
    pub priority: i32,
    // health check api path
    pub path: String,
    // health check method, for example, "POST", "GET"
    pub method: String,
    // health check interval, in seconds
    pub interval: u64,
    // block gap, if the cluster block number is block_gap behind the max block number, it's considered unhealthy
    pub block_gap: u64,
    // chain type, for example, "ethereum", "bitcoin"
    pub chain_type: String,
}

fn build_chain_cluster_service<S: BackendSelection>(
    chain_config: &ChainProxyConfig,
    chain_state: Arc<Mutex<ChainState>>,
) -> GenBackgroundService<LoadBalancer<S>>
where
    S: BackendSelection + 'static,
    S::Iter: BackendIter,
{
    let upstreams = vec![chain_config.proxy_addr.clone()];
    // We add health check in the background so that the bad server is never selected.
    let mut cluster = LoadBalancer::try_from_iter(upstreams).unwrap();
    // using chain health check
    let chain_health_check = ChainHealthCheck::new(
        chain_config.proxy_uri.as_str(),
        chain_config.path.as_str(),
        chain_config.method.as_str(),
        chain_state,
    );

    // set health check validator and request body according to the chain type
    if let Some(checker) = crate::service::chain_health_check::get_chain_checker(&chain_config.chain_type) {
        let chain_health_check = chain_health_check
            .with_response_body_validator(checker.validator);

        let chain_health_check = chain_health_check.with_request_body(
            checker.request_body,
        );

        cluster.set_health_check(chain_health_check);
    } else {
        // default health check
        // no validator, no request body
        cluster.set_health_check(chain_health_check);
    }

    cluster.health_check_frequency = Some(std::time::Duration::from_secs(chain_config.interval));
    background_service("cluster health check", cluster)
}

pub fn new_chain_proxy_service(
    chain_name: &str,
    server_conf: &Arc<ServerConf>,
    listen_addr: &str,
    host_configs: Vec<ChainProxyConfig>,
) -> (impl Service, Vec<Box<dyn Service>>) {
    // first create shared chain state for proxy upstream selection
    let chain_state = Arc::new(Mutex::new(ChainState::new(chain_name)));

    // build a vector of background services from host configs
    let mut cluster_services = Vec::new();
    let mut clusters = HashMap::new();
    for host_config in host_configs.iter() {
        let cluster = build_chain_cluster_service::<RoundRobin>(host_config, chain_state.clone());
        clusters.insert(host_config.proxy_uri.clone(), cluster.task());
        cluster_services.push(Box::new(cluster) as Box<dyn Service>);
    }

    let proxy_app = proxy::ProxyApp::new(chain_name.to_string(), host_configs.clone(), clusters, chain_state);
    let mut service = http_proxy_service(server_conf, proxy_app);
    service.add_tcp(listen_addr);

    (service, cluster_services)
}
