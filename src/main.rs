use log4rs;
use clap::Parser;
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

use crate::config::{Config, Node, Chain, Common};
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
mod metrics;

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

fn create_chain_proxy_config(node: &Node, chain: &Chain) -> Option<service::proxy::ChainProxyConfig> {
    let node_url = node.address();
    let url = Url::parse(node_url).ok()?;
    let host_str = url.host_str()?;
    let port = match url.scheme() {
        "http" => url.port().unwrap_or(80),
        "https" => url.port().unwrap_or(443),
        _ => return None,
    };

    Some(service::proxy::ChainProxyConfig {
        proxy_addr: format!("{}:{}", host_str, port),
        proxy_tls: url.scheme() == "https",
        proxy_hostname: host_str.to_string(),
        proxy_uri: node_url.to_string(),
        priority: node.priority(),
        path: chain.health_check().path().to_string(),
        method: chain.health_check().method().to_string(),
        request_body: Option::from(chain.health_check().request_body().as_bytes().to_vec()),
        chain_type: chain.chain_type().to_string(),
        interval: chain.interval(),
        block_gap: chain.block_gap(),
    })
}

fn create_common_proxy_config(node: &Node, common: &Common) -> Option<service::proxy::ChainProxyConfig> {
    let node_url = node.address();
    let url = Url::parse(node_url).ok()?;
    let host_str = url.host_str()?;
    let port = match url.scheme() {
        "http" => url.port().unwrap_or(80),
        "https" => url.port().unwrap_or(443),
        _ => return None,
    };

    Some(service::proxy::ChainProxyConfig {
        proxy_addr: format!("{}:{}", host_str, port),
        proxy_tls: url.scheme() == "https",
        proxy_hostname: host_str.to_string(),
        proxy_uri: node_url.to_string(),
        priority: node.priority(),
        path: common.health_check().path().to_string(),
        method: common.health_check().method().to_string(),
        request_body: Option::from(common.health_check().request_body().as_bytes().to_vec()),
        interval: common.interval(),
        block_gap: 0,
        chain_type: "".to_string(),
    })
}


fn create_services_from_config(server_conf: &Arc<ServerConf>) -> Vec<Box<dyn Service>> {
    let mut services: Vec<Box<dyn Service>> = Vec::new();

    let config = CONFIG.read().unwrap();

    // create node proxy service
    for chain in &config.chains {
        let http_port = chain.listen();

        // from chain config to host config
        let mut host_configs = Vec::new();
        for node in chain.nodes().iter() {
            if let Some(host_config) = create_chain_proxy_config(node, chain) {
                log::info!("Host config: {:#?}", host_config);
                host_configs.push(host_config);
            } else {
                log::error!("Invalid node url: {}", node.address());
            }
        }

        // from chain config to special method config
        let special_methods = chain.special_methods();
        let mut special_method_configs = Vec::new();
        if let Some(special_methods) = special_methods {
            for special_method in special_methods.iter() {
                let mut method_nodes = Vec::new();
                for node in special_method.nodes.iter() {
                    if let Some(method_node) = create_chain_proxy_config(node, chain) {
                        method_nodes.push(method_node);
                    } else {
                        log::error!("Invalid node url: {}", node.address());
                    }
                }

                let special_method_config = service::proxy::SpecialMethodConfig {
                    method_name: special_method.method_name.clone(),
                    nodes: method_nodes,
                };

                special_method_configs.push(special_method_config);
            }
        }


        let (chain_proxy_service, cluster_services) = service::proxy::new_chain_proxy_service(
            chain.name(),
            chain.protocol(),
            server_conf,
            &format!("0.0.0.0:{http_port}"),
            host_configs,
            special_method_configs,
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

    // create common proxy service
    for common in &config.commons {
        let http_port = common.listen();

        // from common config to host config
        let mut host_configs = Vec::new();
        for node in common.nodes().iter() {
            if let Some(host_config) = create_common_proxy_config(node, common) {
                log::info!("Host config: {:#?}", host_config);
                host_configs.push(host_config);
            } else {
                log::error!("Invalid node url: {}", node.address());
            }
        }

        // from common config to special method config
        let special_methods = common.special_methods();
        let mut special_method_configs = Vec::new();
        if let Some(special_methods) = special_methods {
            for special_method in special_methods.iter() {
                let mut method_nodes = Vec::new();
                for node in special_method.nodes.iter() {
                    if let Some(method_node) = create_common_proxy_config(node, common) {
                        method_nodes.push(method_node);
                    } else {
                        log::error!("Invalid node url: {}", node.address());
                    }
                }

                let special_method_config = service::proxy::SpecialMethodConfig {
                    method_name: special_method.method_name.clone(),
                    nodes: method_nodes,
                };

                special_method_configs.push(special_method_config);
            }
        }

        let (common_proxy_service, cluster_services) = service::proxy::new_common_proxy_service(
            common.name(),
            common.protocol(),
            server_conf,
            &format!("0.0.0.0:{http_port}"),
            host_configs,
            special_method_configs,
        );

        let common_name = common.name();
        // print common proxy info
        log::info!(
            "Common {} proxy service created, listening on {}, \
            interval: {}",
            common_name,
            http_port,
            common.interval()
        );

        services.push(Box::new(common_proxy_service));
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

    // init chain checker
    service::chain_health_check::init_chain_checker();

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

    // let opt = Some(Opt::parse_from(opts));
    let mut my_server = Server::new(Some(Opt::parse_from(opts))).unwrap();
    my_server.bootstrap();

    // print the server configuration
    log::info!("Server configuration: {:#?}", my_server.configuration);

    // create services from config and add to server
    let services: Vec<Box<dyn Service>> = create_services_from_config(&my_server.configuration);

    my_server.add_services(services);

    // init metrics
    metrics::init_metrics(CONFIG.read().unwrap().monitor.system()).unwrap();

    // add prometheus service
    let monitor_listen = CONFIG.read().unwrap().monitor.listen();
    let mut prometheus_service_http =
        pingora::services::listening::Service::prometheus_http_service();
    prometheus_service_http.add_tcp(format!("0.0.0.0:{monitor_listen}").as_str());

    log::info!("Prometheus service created, listening on {monitor_listen}");
    my_server.add_service(prometheus_service_http);

    my_server.run_forever();
}
