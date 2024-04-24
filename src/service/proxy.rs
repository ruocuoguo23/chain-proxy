use std::sync::Arc;

use pingora::{prelude::*, server::configuration::ServerConf};

use crate::app::proxy;

#[derive(Clone)]
pub struct HostConfigPlain {
    pub proxy_addr: String,
    pub proxy_tls: bool,
    pub proxy_hostname: String,
}

pub fn proxy_service_plain(
    server_conf: &Arc<ServerConf>,
    listen_addr: &str,
    host_configs: Vec<HostConfigPlain>,
) -> impl pingora::services::Service {
    let proxy_app = proxy::ProxyApp::new(host_configs.clone());

    // We add health check in the background so that the bad server is never selected.
    // let mut upstreams = LoadBalancer::new();
    // let hc = TcpHealthCheck::new();

    let mut service = http_proxy_service(server_conf, proxy_app);
    service.add_tcp(listen_addr);

    service
}
