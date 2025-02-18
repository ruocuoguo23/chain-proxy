use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use bytes::Bytes;
use pingora::{
    upstreams::peer::HttpPeer,
    Error, Custom, Result,
};
use pingora_load_balancing::LoadBalancer;
use pingora_load_balancing::prelude::RoundRobin;
use pingora_proxy::{ProxyHttp, Session};
use pingora_core::modules::http::{
    grpc_web::{GrpcWeb, GrpcWebBridge},
    HttpModules,
};
use crate::service::proxy::{ChainProxyConfig, SpecialMethodConfig};
use crate::app::proxy_base::{ProxyCtx, ProxyBase};
use crate::app::proxy_utils;

pub struct GrpcNodeProxyApp {
    chain_name: String,
    protocol: String,
    log_request_detail: bool,
    clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
    host_configs: Vec<ChainProxyConfig>,
    special_method_configs: Vec<SpecialMethodConfig>,
}

impl GrpcNodeProxyApp {
    pub fn new(
        chain_name: String,
        protocol: String,
        log_request_detail: bool,
        host_configs: Vec<ChainProxyConfig>,
        special_method_configs: Vec<SpecialMethodConfig>,
        clusters: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
    ) -> Self {
        GrpcNodeProxyApp {
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
impl ProxyBase for GrpcNodeProxyApp {
    fn get_clusters(&self) -> &HashMap<String, Arc<LoadBalancer<RoundRobin>>> {
        &self.clusters
    }

    fn get_chain_name(&self) -> &str {
        &self.chain_name
    }

    #[allow(elided_named_lifetimes)]
    async fn get_eligible_clusters(&self, _session: &mut Session) -> Result<HashMap<i32, Vec<&ChainProxyConfig>>> {
        let mut clusters_by_priority: HashMap<i32, Vec<&ChainProxyConfig>> = HashMap::new();

        // just get the first config
        if let Some(first_config) = self.host_configs.first() {
            clusters_by_priority
                .entry(first_config.priority)
                .or_insert_with(Vec::new)
                .push(first_config);
        } else {
            // if no config found, return error
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
impl ProxyHttp for GrpcNodeProxyApp {
    type CTX = ProxyCtx;

    fn new_ctx(&self) -> Self::CTX {
        ProxyCtx {
            request_body: Vec::new(),
            response_body: Vec::new(),
        }
    }

    fn init_downstream_modules(&self, modules: &mut HttpModules) {
        // add gRPC Web module
        modules.add_module(Box::new(GrpcWeb));
    }

    async fn early_request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        let grpc = session
            .downstream_modules_ctx
            .get_mut::<GrpcWebBridge>()
            .expect("GrpcWebBridge module added");

        // init gRPC Web bridge module
        grpc.init();
        Ok(())
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        // call the base upstream_peer method
        ProxyBase::upstream_peer(self, session).await
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if self.log_request_detail {
            proxy_utils::request_body_filter(body, ctx).await
        } else {
            Ok(())
        }
    }

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

    async fn logging(
        &self,
        session: &mut Session,
        e: Option<&Error>,
        ctx: &mut Self::CTX,
    ) {
        ProxyBase::metrics(self, session);

        if self.log_request_detail {
            proxy_utils::logging(session, e, ctx).await
        }
    }
}
