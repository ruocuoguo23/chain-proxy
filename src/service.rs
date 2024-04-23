use std::sync::Arc;

use pingora::{
    prelude::http_proxy_service,
    server::configuration::ServerConf
    ,
};

use crate::app::ProxyApp;

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
    let proxy_app = ProxyApp::new(host_configs.clone());
    let mut service = http_proxy_service(server_conf, proxy_app);

    service.add_tcp(listen_addr);

    service
}
