use log4rs;
use pingora::server::configuration::ServerConf;
use pingora::{
    server::{configuration::Opt, Server},
    services::Service,
};
use std::sync::Arc;
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
use crate::config::LOG_CONFIG;
use std::path::PathBuf;
use std::sync::RwLock;
use url::Url;

lazy_static! {
    pub static ref CONFIG: RwLock<Config> = RwLock::new(Config::default());
}

mod app;
mod config;
mod service;

#[derive(StructOpt, Debug)]
#[structopt(name = "chain-proxy")]
struct ChainOpt {
    /// Path to the configuration file
    #[structopt(short, long, parse(from_os_str))]
    config: Option<PathBuf>,

    /// Perform an upgrade
    #[structopt(long)]
    upgrade: bool,
}

fn create_services_from_config(server_conf: &Arc<ServerConf>) -> Vec<Box<dyn Service>> {
    let mut services: Vec<Box<dyn Service>> = Vec::new();

    let config = CONFIG.read().unwrap();
    for chain in &config.chains {
        let http_port = chain.listen();

        // from chain config to host config
        let mut host_configs = Vec::new();
        for node in chain.nodes().iter() {
            // parse node url
            let node_url = node.address();
            let url = Url::parse(node_url).unwrap();
            if url.host_str().is_none() {
                log::error!("Invalid node url: {node_url}");
                continue;
            }

            let port = match url.scheme() {
                "http" => url.port().unwrap_or(80),
                "https" => url.port().unwrap_or(443),
                _ => {
                    log::error!("Invalid node url: {node_url}");
                    continue;
                }
            };

            let proxy_addr = format!("{}:{}", url.host_str().unwrap(), port);
            let interval = chain.interval();
            let block_gap = chain.block_gap();
            let host_config = service::proxy::ChainProxyConfig {
                proxy_addr: format!("{}", proxy_addr),
                proxy_tls: node.tls(),
                proxy_hostname: node.hostname().unwrap_or_default().to_string(),
                proxy_uri: node_url.to_string(),
                priority: node.priority(),
                path: chain.health_check().path().to_string(),
                method: chain.health_check().method().to_string(),
                interval,
                block_gap,
            };
            log::info!("Host config: {:#?}", host_config);

            host_configs.push(host_config);
        }

        let (chain_proxy_service, cluster_services) = service::proxy::new_chain_proxy_service(
            server_conf,
            &format!("0.0.0.0:{http_port}"),
            host_configs,
        );

        let chain_name = chain.name();
        // print chain proxy info
        log::info!(
            "Chain {} proxy service created, listening on {}, \
            interval: {}, block_gap: {}",
            chain_name,
            http_port,
            chain.interval(),
            chain.block_gap()
        );

        services.push(Box::new(chain_proxy_service));
        for cluster_service in cluster_services {
            services.push(cluster_service);
        }
    }

    services
}

pub fn main() {
    // init log
    let config =
        serde_yaml::from_str::<log4rs::config::RawConfig>(&LOG_CONFIG.to_string()).unwrap();

    // Initialize log4rs with the parsed configuration
    log4rs::init_raw_config(config).unwrap();

    // load config
    let chain_opt = ChainOpt::from_args();
    let config_path = chain_opt.config.unwrap_or_else(|| "config.yaml".into());
    match Config::load_config(&config_path) {
        Ok(_) => {
            log::info!("Config loaded successfully");
        }
        Err(e) => {
            log::error!("Failed to load config: {e}");
            std::process::exit(1);
        }
    }

    let mut opts: Vec<String> = vec![
        "chain-proxy".into(),
        "-c".into(),
        config_path.to_str().unwrap().into(),
    ];

    // if upgrade flag is set, add it to the opts
    if chain_opt.upgrade {
        opts.push("-u".into());
    }

    let opt = Some(Opt::from_iter(opts));
    let mut my_server = Server::new(opt).unwrap();
    my_server.bootstrap();

    // print the server configuration
    log::info!("Server configuration: {:#?}", my_server.configuration);

    // create services from config and add to server
    let services: Vec<Box<dyn Service>> = create_services_from_config(&my_server.configuration);

    my_server.add_services(services);
    my_server.run_forever();
}
