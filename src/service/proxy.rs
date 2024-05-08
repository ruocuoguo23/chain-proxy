use crate::app::proxy;
use crate::service::chain_health_check::ChainHealthCheck;
use pingora::lb::{
    selection::{BackendIter, BackendSelection},
    LoadBalancer,
};
use pingora::{
    prelude::*, server::configuration::ServerConf, services::background::GenBackgroundService,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ChainProxyConfig {
    pub proxy_addr: String,
    pub proxy_tls: bool,
    pub proxy_hostname: String,
    pub proxy_uri: String,
    pub priority: i32,
    pub path: String,
    pub method: String,
}

fn build_chain_cluster_service<S: BackendSelection>(
    chain_config: &ChainProxyConfig,
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
    );

    // set health check validator and request body according to the chain type
    // currently we use eth validator for all chains
    let chain_health_check = chain_health_check
        .with_response_body_validator(Box::new(crate::service::chain_health_check::eth_validator));

    // set eth_blockNumber as the request body
    let chain_health_check = chain_health_check.with_request_body(
        r#"
                {
                    "jsonrpc":"2.0",
                    "method":"eth_blockNumber",
                    "id":1
               }
               "#
        .as_bytes()
        .to_vec(),
    );

    cluster.set_health_check(chain_health_check);
    cluster.health_check_frequency = Some(std::time::Duration::from_secs(5));

    background_service("cluster health check", cluster)
}

pub fn new_chain_proxy_service(
    server_conf: &Arc<ServerConf>,
    listen_addr: &str,
    host_configs: Vec<ChainProxyConfig>,
) -> (
    impl pingora::services::Service,
    impl pingora::services::Service,
    impl pingora::services::Service,
) {
    let cluster_one = build_chain_cluster_service::<RoundRobin>(&host_configs[0]);
    let cluster_two = build_chain_cluster_service::<RoundRobin>(&host_configs[1]);

    let proxy_app =
        proxy::ProxyApp::new(host_configs.clone(), cluster_one.task(), cluster_two.task());
    let mut service = http_proxy_service(server_conf, proxy_app);
    service.add_tcp(listen_addr);

    (service, cluster_one, cluster_two)
}
