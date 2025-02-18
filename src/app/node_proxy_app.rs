use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use log::{debug, info};
use async_trait::async_trait;
use bytes::{Bytes};
use pingora_proxy::ProxyHttp;
use pingora::{
    upstreams::peer::{HttpPeer},
    Error,
    Custom,
    Result
};
use pingora_load_balancing::LoadBalancer;
use pingora_load_balancing::prelude::RoundRobin;
use pingora_proxy::Session;
use crate::config::ChainState;
use crate::service::proxy::{ChainProxyConfig, SpecialMethodConfig};
use crate::app::proxy_base::{ProxyCtx, ProxyBase};
use crate::app::proxy_utils;

pub struct NodeProxyApp {
    chain_name: String,

    protocol: String,

    log_request_detail: bool,

    // currently we only support two clusters, maybe with different priority
    // key is the host name, value is the cluster
    clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,

    // host configs
    host_configs: Vec<ChainProxyConfig>,

    // special method configs
    special_method_configs: Vec<SpecialMethodConfig>,

    // shared chain state
    chain_state: Arc<Mutex<ChainState>>,
}

impl NodeProxyApp {
    pub fn new(
        chain_name: String,
        protocol: String,
        log_request_detail: bool,
        host_configs: Vec<ChainProxyConfig>,
        special_method_configs: Vec<SpecialMethodConfig>,
        clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
        chain_state: Arc<Mutex<ChainState>>,
    ) -> Self {
        NodeProxyApp {
            chain_name,
            protocol,
            log_request_detail,
            clusters,
            host_configs,
            special_method_configs,
            chain_state: Arc::clone(&chain_state),
        }
    }
}

#[async_trait]
impl ProxyBase for NodeProxyApp {
    fn get_clusters(&self) -> &HashMap<String, Arc<LoadBalancer<RoundRobin>>> {
        &self.clusters
    }

    fn get_chain_name(&self) -> &str {
        &self.chain_name
    }

    #[allow(elided_named_lifetimes)]
    async fn get_eligible_clusters(&self, session: &mut Session) -> Result<HashMap<i32, Vec<&ChainProxyConfig>>> {
        if let Some(result) = self.get_clusters_by_special_method(session).await {
            return result;
        }

        // if not a special method, find the eligible clusters by block number
        let block_numbers = {
            let state = self.chain_state.lock().unwrap();
            state.get_block_numbers().clone()
        };

        let max_block_number = block_numbers.values().max().unwrap_or(&0);
        if max_block_number == &0 {
            log::error!("No block number found");
            return Error::e_explain(Custom("No block number found, maybe health check is unavailable or system is starting"), "proxy error");
        }

        let block_range = self.host_configs[0].block_gap;

        debug!(
            "Max block number: {}, current block range: {}",
            max_block_number, block_range
        );

        let mut clusters_by_priority: HashMap<i32, Vec<&ChainProxyConfig>> = HashMap::new();
        for config in self.host_configs.iter() {
            let current_block_number = block_numbers.get(&config.proxy_uri);
            if current_block_number.is_none() {
                debug!(
                    "Host: {} is not eligible, block number not found",
                    config.proxy_uri
                );
                continue;
            }

            let current_block_number = current_block_number.unwrap();

            if max_block_number - current_block_number > block_range {
                info!(
                    "Host: {} is not eligible, block number: {}",
                    config.proxy_uri,
                    current_block_number
                );
                continue;
            }

            clusters_by_priority.entry(config.priority).or_insert_with(Vec::new).push(config);
        }

        if clusters_by_priority.is_empty() {
            log::error!("No eligible cluster found");
            return Error::e_explain(Custom("No eligible cluster found"), "proxy error");
        }

        Ok(clusters_by_priority)
    }

    fn get_protocol(&self) -> &str {
        &self.protocol
    }

    fn get_special_method_configs(&self) -> &Vec<SpecialMethodConfig> {
        &self.special_method_configs
    }
}

#[async_trait]
impl ProxyHttp for NodeProxyApp {
    type CTX = ProxyCtx;
    fn new_ctx(&self) -> Self::CTX{
        ProxyCtx {
            request_body: Vec::new(),
            response_body: Vec::new(),
        }
    }

    async fn upstream_peer(&self,
                           session: &mut Session,
                           _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        ProxyBase::upstream_peer(self, session).await
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        // only log request detail should we need to log the request body
        if self.log_request_detail {
            proxy_utils::request_body_filter(body, ctx).await
        } else {
            Ok(())
        }
    }

    // response body
    fn upstream_response_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX) {
        if self.log_request_detail {
            proxy_utils::upstream_response_body_filter(body, ctx)
        }
    }

    async fn logging(
        &self, session:
        &mut Session,
        e: Option<&Error>,
        ctx: &mut Self::CTX) {
        ProxyBase::metrics(self, session);

        if self.log_request_detail {
            proxy_utils::logging(session, e, ctx).await
        }
    }
}