use std::env;
use std::sync::Arc;

use pingora::server::configuration::ServerConf;
use pingora::{
    server::{configuration::Opt, Server},
    services::Service,
};
use structopt::StructOpt;

// The `jemallocator` crate provides a wrapper around the jemalloc allocator.
// jemalloc is known for its performance and scalability in multi-threaded applications,
// as it can help to reduce memory fragmentation and provide more predictable memory usage patterns.
// By using the `#[global_allocator]` attribute, we are designating the jemalloc allocator
// as the global allocator for this Rust program. All dynamic memory allocations,
// such as those for `Box`, `Vec`, `String`, etc., will now be handled by jemalloc.
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[macro_use]
extern crate lazy_static;

use crate::config::Config;
use std::sync::RwLock;

lazy_static! {
    pub static ref CONFIG: RwLock<Config> = RwLock::new(Config::default());
}

mod app;
mod config;
mod service;

fn create_services_from_config(server_conf: &Arc<ServerConf>) -> Vec<Box<dyn Service>> {
    let mut services: Vec<Box<dyn Service>> = Vec::new();

    let config = CONFIG.read().unwrap();
    for chain in &config.chains {
        let http_port = chain.listen();
        let host_configs = chain
            .nodes()
            .iter()
            .map(|node| service::proxy::HostConfigPlain {
                proxy_addr: format!("{}:{}", node.host(), node.port()),
                proxy_tls: node.tls(),
                proxy_hostname: node.hostname().to_string(),
            })
            .collect();
        let chain_proxy_service = service::proxy::proxy_service_plain(
            server_conf,
            &format!("0.0.0.0:{http_port}"),
            host_configs,
        );

        services.push(Box::new(chain_proxy_service));
    }

    services
}

pub fn main() {
    // init log
    env_logger::init();

    // load config
    let config_path = env::var("CONFIG_PATH").unwrap_or("config.yaml".to_owned());
    match config::Config::load_config(config_path) {
        Ok(_) => {
            log::info!("Config loaded successfully");
        }
        Err(e) => {
            log::error!("Failed to load config: {e}");
            std::process::exit(1);
        }
    }

    let opt = Some(Opt::from_args());
    let mut my_server = Server::new(opt).unwrap();
    my_server.bootstrap();

    // create services from config and add to server
    let services: Vec<Box<dyn Service>> = create_services_from_config(&my_server.configuration);

    my_server.add_services(services);
    my_server.run_forever();
}
