use crate::service::proxy::{ChainProxyConfig, SpecialMethodConfig};
use async_trait::async_trait;
use log::{debug, info};
use pingora::{
    upstreams::peer::{HttpPeer},
    Error,
    Custom,
    Result
};

use pingora_proxy::Session;
use pingora::protocols::ALPN;
use pingora_load_balancing::selection::RoundRobin;
use pingora_load_balancing::LoadBalancer;
use std::collections::{HashMap};
use std::sync::{Arc};
use rand::seq::SliceRandom;
use rand::thread_rng;

use crate::app::config::{DEFAULT_PEER_OPTIONS};
use crate::metrics::inc_proxy_result_counter;

pub struct ProxyCtx {
    pub(crate) request_body:  Vec<u8>,
    pub(crate) response_body:  Vec<u8>,
}

#[async_trait]
pub trait ProxyBase: Send + Sync {
    fn get_clusters(&self) -> &HashMap<String, Arc<LoadBalancer<RoundRobin>>>;
    fn get_chain_name(&self) -> &str;

    async fn upstream_peer(&self,
                           session: &mut Session
    ) -> Result<Box<HttpPeer>> {
        let clusters_by_priority = self.get_eligible_clusters(session).await?;
        
        // Find the highest priority clusters
        let max_priority = clusters_by_priority.keys().max().unwrap();
        let highest_priority_clusters = clusters_by_priority.get(max_priority).unwrap();

        // Select a cluster from the highest priority clusters
        let selected_cluster = if highest_priority_clusters.len() == 1 {
            highest_priority_clusters[0]
        } else {
            // Random selection
            let mut rng = thread_rng();
            highest_priority_clusters.choose(&mut rng).unwrap()

            // if you want to use round robin selection, you can add here
        };

        // check the cluster
        let cluster = self.get_clusters().get(selected_cluster.proxy_uri.as_str());
        if let None = cluster {
            log::error!("Cluster not found");
            return Error::e_explain(Custom("Cluster not found"), "proxy error");
        }

        let session = session.as_downstream_mut();
        let req = session.req_header_mut();

        // set session header to host name
        let result = req.insert_header("host", selected_cluster.proxy_hostname.as_str());
        if let Err(e) = result {
            log::error!("Failed to set host header: {e}");
        }

        // sometimes we need to set the request path to the cluster path
        // if protocol is jsonrpc, we need to set the path to the cluster path
        // if protocol is http or grpc, we need to combine the cluster path with the request path, and proxy query string
        if self.get_protocol() == "jsonrpc" {
            req.set_uri(selected_cluster.proxy_uri.as_str().parse().unwrap());
        } else {
            let mut new_uri = if req.uri.path().is_empty() || req.uri.path() == "/" {
                selected_cluster.proxy_uri.clone()
            } else {
                format!("{}{}", selected_cluster.proxy_uri, req.uri.path())
            };

            // // extract the query string
            if let Some(query) = req.uri.query() {
                new_uri.push_str("?");
                new_uri.push_str(query);
            }

            req.set_uri(new_uri.as_str().parse().unwrap());
        }

        let proxy_to = HttpPeer::new(
            selected_cluster.proxy_addr.as_str(),
            selected_cluster.proxy_tls,
            selected_cluster.proxy_hostname.clone(),
        );
        let mut peer = Box::new(proxy_to);

        // if protocol is grpc, peer should be set to grpc
        if self.get_protocol() == "grpc" {
            // peer.options = GRPC_PEER_OPTIONS;
            info!("grpc using h2");
            peer.options.alpn = ALPN::H2;
        } else {
            peer.options = DEFAULT_PEER_OPTIONS;
        }

        // log the selected peer
        debug!("Selected peer: {peer}");
        Ok(peer)
    }

    fn metrics(&self, session: &mut Session) {
        let response_code = session
            .response_written()
            .map_or(0, |resp| resp.status.as_u16());

        // increment the metrics
        let session = session.as_downstream();
        let req = session.req_header();
        if let Some(host) = req.headers.get("host") {
            let host = host.to_str().unwrap_or("unknown");

            inc_proxy_result_counter(
                self.get_chain_name(),
                host,
                response_code.to_string().as_str(),
                req.method.as_str(),
            );
        }
    }

    #[allow(elided_named_lifetimes)]
    async fn get_eligible_clusters(&self, session: &mut Session) -> Result<HashMap<i32, Vec<&ChainProxyConfig>>>;
    fn get_protocol(&self) -> &str;

    fn get_special_method_configs(&self) -> &Vec<SpecialMethodConfig>;

    #[allow(elided_named_lifetimes)]
    async fn get_clusters_by_special_method(&self, session: &mut Session) -> Option<Result<HashMap<i32, Vec<&ChainProxyConfig>>>> {
        let request_headers = session.as_downstream().req_header();
        if !self.get_special_method_configs().is_empty() && request_headers.headers.contains_key("X-Proxy-Jsonrpc-Method") {
            let method = request_headers.headers.get("X-Proxy-Jsonrpc-Method").unwrap();
            let method = method.to_str().unwrap();

            for config in self.get_special_method_configs().iter() {
                if config.method_name == method {
                    let mut clusters_by_priority: HashMap<i32, Vec<&ChainProxyConfig>> = HashMap::new();
                    for config in config.nodes.iter() {
                        clusters_by_priority.entry(config.priority).or_insert_with(Vec::new).push(config);
                    }

                    return Some(Ok(clusters_by_priority));
                }
            }
        }
        None
    }
}
