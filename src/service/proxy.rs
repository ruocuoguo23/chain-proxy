use std::sync::Arc;

use pingora::{prelude::*, server::configuration::ServerConf};

use crate::app::proxy;

#[derive(Clone)]
pub struct HostConfigPlain {
    pub proxy_addr: String,
    pub proxy_tls: bool,
    pub proxy_hostname: String,
    pub priority: i32,
}

pub fn proxy_service_plain(
    server_conf: &Arc<ServerConf>,
    listen_addr: &str,
    host_configs: Vec<HostConfigPlain>,
) -> (
    impl pingora::services::Service,
    impl pingora::services::Service,
) {
    // We add health check in the background so that the bad server is never selected.
    let proxy_addresses = host_configs
        .iter()
        .map(|host_config| host_config.proxy_addr.clone())
        .collect::<Vec<String>>();
    let mut upstreams = LoadBalancer::try_from_iter(proxy_addresses).unwrap();

    let hc = TcpHealthCheck::new();
    upstreams.set_health_check(hc);
    upstreams.health_check_frequency = Some(std::time::Duration::from_secs(5));

    let background = background_service("health-check", upstreams);
    let upstreams = background.task();

    let proxy_app = proxy::ProxyApp::new(host_configs.clone(), upstreams);
    let mut service = http_proxy_service(server_conf, proxy_app);
    service.add_tcp(listen_addr);

    (service, background)
}
