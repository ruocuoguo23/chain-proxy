use std::env;

use pingora::{
    server::{configuration::Opt, Server},
    services::Service,
};
use structopt::StructOpt;

#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

mod app;
mod service;

pub fn main() {
    let http_port = env::var("HTTP_PORT").unwrap_or("80".to_owned());

    env_logger::init();

    let opt = Some(Opt::from_args());
    let mut my_server = Server::new(opt).unwrap();
    my_server.bootstrap();

    let proxy_service_plain = service::service::proxy_service_plain(
        &my_server.configuration,
        &format!("0.0.0.0:{http_port}"),
        vec![service::service::HostConfigPlain {
            proxy_addr: "127.0.0.1:4000".to_owned(),
            proxy_tls: false,
            proxy_hostname: "someotherdomain.com".to_owned(),
        }],
    );

    let services: Vec<Box<dyn Service>> = vec![
        Box::new(proxy_service_plain),
    ];
    my_server.add_services(services);
    my_server.run_forever();
}


// // We add health check in the background so that the bad server is never selected.
// let hc = TcpHealthCheck::new();
// upstreams.set_health_check(hc);
// upstreams.health_check_frequency = Some(Duration::from_secs(1));
//
// let background = background_service("health check", upstreams);