use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use bytes::{Bytes};

use pingora_proxy::{ProxyHttp, Session};
use pingora::{
    upstreams::peer::{HttpPeer},
    Error,
    Custom,
    Result
};
use pingora_load_balancing::LoadBalancer;
use pingora_load_balancing::prelude::RoundRobin;
use crate::service::proxy::{ChainProxyConfig, SpecialMethodConfig};
use crate::app::proxy_base::{ProxyBase, ProxyCtx};
use crate::app::proxy_utils;

pub struct CommonProxyApp {
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
}

impl CommonProxyApp {
    pub fn new(
        chain_name: String,
        protocol: String,
        log_request_detail: bool,
        host_configs: Vec<ChainProxyConfig>,
        special_method_configs: Vec<SpecialMethodConfig>,
        clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
    ) -> Self {
        CommonProxyApp {
            chain_name,
            protocol,
            log_request_detail,
            clusters,
            host_configs,
            special_method_configs,
        }
    }
}

#[async_trait]
impl ProxyBase for CommonProxyApp {
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

        // if not a special method, find the eligible clusters by other criteria
        let mut clusters_by_priority: HashMap<i32, Vec<&ChainProxyConfig>> = HashMap::new();
        for config in self.host_configs.iter() {
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
impl ProxyHttp for CommonProxyApp {
    type CTX = ProxyCtx;
    fn new_ctx(&self) -> Self::CTX{
        ProxyCtx {
            request_body: Vec::new(),
            response_body: Vec::new(),
        }
    }

    async fn upstream_peer(&self,
                           session: &mut Session,
                           _ctx: &mut Self::CTX
    ) -> Result<Box<HttpPeer>> {
        ProxyBase::upstream_peer(self, session).await
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()>
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
        ctx: &mut Self::CTX,
    ) {
        if self.log_request_detail {
            proxy_utils::upstream_response_body_filter(body, ctx)
        }
    }

    async fn logging(&self, session: &mut Session, e: Option<&Error>, ctx: &mut Self::CTX) {
        ProxyBase::metrics(self, session);

        if self.log_request_detail {
            proxy_utils::logging(session, e, ctx).await
        }
    }
}
