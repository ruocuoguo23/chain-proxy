use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;

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
use crate::service::proxy::{ChainProxyConfig, SpecialMethodConfig};
use crate::app::proxy_base::ProxyBase;

pub struct CommonProxyApp {
    chain_name: String,

    protocol: String,

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
        host_configs: Vec<ChainProxyConfig>,
        special_method_configs: Vec<SpecialMethodConfig>,
        clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
    ) -> Self {
        CommonProxyApp {
            chain_name,
            protocol,
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
    type CTX = ();
    fn new_ctx(&self) {}

    async fn upstream_peer(&self, session: &mut Session, ctx: &mut ()) -> Result<Box<HttpPeer>> {
        ProxyBase::upstream_peer(self, session, ctx).await
    }

    async fn logging(&self, session: &mut Session, e: Option<&Error>, ctx: &mut Self::CTX) {
        ProxyBase::logging(self, session, e, ctx).await
    }
}
