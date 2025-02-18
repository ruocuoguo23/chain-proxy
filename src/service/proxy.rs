use crate::config::{ChainState, NodeState, UnifyProxyConfig};
use crate::service::chain_health_check::ChainHealthCheck;
use crate::service::common_health_check::CommonHealthCheck;
use crate::app::node_proxy_app::NodeProxyApp;
use crate::app::grpc_node_proxy_app::GrpcNodeProxyApp;
use crate::app::common_proxy_app::CommonProxyApp;
use crate::app::unify_proxy_app::UnifyProxyApp;
use pingora_load_balancing::{
    selection::{BackendIter, BackendSelection, RoundRobin},
    LoadBalancer
};
use pingora_proxy::http_proxy_service;
use pingora::{
    server::configuration::ServerConf, services::background::{GenBackgroundService, background_service},
    services::Service,
};
use pingora_core::apps::{HttpServerOptions};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct SpecialMethodConfig {
    pub method_name: String,
    pub nodes: Vec<ChainProxyConfig>,
}

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
    // health check request body
    pub request_body: Option<Vec<u8>>,
    // health check interval, in seconds
    pub interval: u64,
    // block gap, if the cluster block number is block_gap behind the max block number, it's considered unhealthy
    pub block_gap: u64,
    // chain type, for example, "ethereum", "bitcoin"
    pub chain_type: String,
    // log request detail, default is false
    pub log_request_detail: bool,
    // Optional username for Basic Auth
    pub username: Option<String>,
    // Optional password for Basic Auth
    pub password: Option<String>,
    pub custom_headers: Option<HashMap<String, String>>,
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
    let mut chain_health_check = ChainHealthCheck::new(
        chain_config.proxy_uri.as_str(),
        chain_config.path.as_str(),
        chain_config.method.as_str(),
        chain_state,
    );

    // Set Basic Auth if username and password are provided
    if let (Some(username), Some(password)) = (&chain_config.username, &chain_config.password) {
        chain_health_check = chain_health_check.with_basic_auth(username, password);
    }

    if let Some(headers) = &chain_config.custom_headers {
        chain_health_check = chain_health_check.with_custom_headers(headers.clone());
    }

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

fn build_common_cluster_service<S: BackendSelection>(
    common_config: &ChainProxyConfig,
    node_state: Arc<Mutex<NodeState>>,
) -> GenBackgroundService<LoadBalancer<S>>
where
    S: BackendSelection + 'static,
    S::Iter: BackendIter,
{
    let upstreams = vec![common_config.proxy_addr.clone()];
    // We add health check in the background so that the bad server is never selected.
    let mut cluster = LoadBalancer::try_from_iter(upstreams).unwrap();

    // using common health check
    let common_health_check = CommonHealthCheck::new(
        common_config.proxy_uri.as_str(),
        common_config.path.as_str(),
        common_config.method.as_str(),
        node_state,
    );
    let common_health_check = common_health_check.with_request_body(
        common_config.request_body.clone().unwrap_or_default(),
    );

    cluster.set_health_check(common_health_check);

    // current no health check for common cluster
    cluster.health_check_frequency = Some(std::time::Duration::from_secs(common_config.interval));
    background_service("cluster health check", cluster)
}


pub fn new_grpc_chain_proxy_service(
    chain_name: &str,
    server_conf: &Arc<ServerConf>,
    listen_addr: &str,
    host_configs: Vec<ChainProxyConfig>,
    special_method_config: Vec<SpecialMethodConfig>,
) -> (Box<dyn Service>, Vec<Box<dyn Service>>) {
    // 创建共享的链状态
    let chain_state = Arc::new(Mutex::new(ChainState::new(chain_name)));

    // 构建集群服务
    let mut cluster_services = Vec::new();
    let mut clusters = HashMap::new();
    for host_config in host_configs.iter() {
        let cluster = build_chain_cluster_service::<RoundRobin>(host_config, chain_state.clone());
        clusters.insert(host_config.proxy_uri.clone(), cluster.task());
        cluster_services.push(Box::new(cluster) as Box<dyn Service>);
    }

    let log_request_detail = host_configs[0].log_request_detail;

    // 创建 GrpcNodeProxyApp
    let proxy_app = GrpcNodeProxyApp::new(
        chain_name.to_string(),
        "grpc".to_string(),
        log_request_detail,
        host_configs.clone(),
        special_method_config.clone(),
        clusters,
    );

    // 创建服务
    let mut service = http_proxy_service(server_conf, proxy_app);
    let http_logic = service.app_logic_mut().unwrap();
    let mut http_server_options = HttpServerOptions::default();
    http_server_options.h2c = true;
    http_logic.server_options = Some(http_server_options);
    service.add_tcp(listen_addr);

    (Box::new(service), cluster_services)
}

pub fn new_chain_proxy_service(
    chain_name: &str,
    protocol: &str,
    server_conf: &Arc<ServerConf>,
    listen_addr: &str,
    host_configs: Vec<ChainProxyConfig>,
    special_method_config: Vec<SpecialMethodConfig>,
) -> (Box<dyn Service>, Vec<Box<dyn Service>>) {
    // 创建共享的链状态
    let chain_state = Arc::new(Mutex::new(ChainState::new(chain_name)));

    // 构建集群服务
    let mut cluster_services = Vec::new();
    let mut clusters = HashMap::new();
    for host_config in host_configs.iter() {
        let cluster = build_chain_cluster_service::<RoundRobin>(host_config, chain_state.clone());
        clusters.insert(host_config.proxy_uri.clone(), cluster.task());
        cluster_services.push(Box::new(cluster) as Box<dyn Service>);
    }

    let log_request_detail = host_configs[0].log_request_detail;

    // 创建 NodeProxyApp
    let proxy_app = NodeProxyApp::new(
        chain_name.to_string(),
        protocol.to_string(),
        log_request_detail,
        host_configs.clone(),
        special_method_config.clone(),
        clusters,
        chain_state,
    );

    // 创建服务
    let mut service = http_proxy_service(server_conf, proxy_app);
    service.add_tcp(listen_addr);

    (Box::new(service), cluster_services)
}


pub fn new_common_proxy_service(
    common_name: &str,
    protocol: &str,
    server_conf: &Arc<ServerConf>,
    listen_addr: &str,
    host_configs: Vec<ChainProxyConfig>,
    special_method_config: Vec<SpecialMethodConfig>,
) -> (impl Service, Vec<Box<dyn Service>>) {
    // first create shared common state for proxy upstream selection
    let common_state = Arc::new(Mutex::new(NodeState::new(common_name)));

    // build a vector of background services from host configs
    let mut cluster_services = Vec::new();
    let mut clusters = HashMap::new();
    for host_config in host_configs.iter() {
        let cluster = build_common_cluster_service::<RoundRobin>(host_config, common_state.clone());
        clusters.insert(host_config.proxy_uri.clone(), cluster.task());
        cluster_services.push(Box::new(cluster) as Box<dyn Service>);
    }

    let log_request_detail = host_configs[0].log_request_detail;

    let proxy_app = CommonProxyApp::new(
                                        common_name.to_string(),
                                        protocol.to_string(),
                                        log_request_detail,
                                        host_configs.clone(),
                                        special_method_config.clone(),
                                        clusters);
    let mut service = http_proxy_service(server_conf, proxy_app);
    service.add_tcp(listen_addr);

    (service, cluster_services)
}

pub fn new_unify_proxy_service(
    server_conf: &Arc<ServerConf>,
    unify_config: UnifyProxyConfig,
) -> impl Service {
    let listen_addr = &format!("0.0.0.0:{}", unify_config.listen_port());

    let proxy_app = UnifyProxyApp::new(unify_config);
    let mut service = http_proxy_service(server_conf, proxy_app);
    service.add_tcp(listen_addr);
    service
}
