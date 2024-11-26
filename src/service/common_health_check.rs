use std::time::Duration;
use std::sync::{Arc, Mutex};
use reqwest::{Client, Method, header::{HeaderMap, HeaderValue, CONTENT_TYPE}};
use async_trait::async_trait;
use pingora_load_balancing::health_check::HealthCheck;
use pingora_load_balancing::Backend;
use pingora::{Custom, Error, Result};

use crate::config::NodeState;
use crate::metrics::set_node_health_gauge;

pub struct CommonHealthCheck {
    consecutive_success: usize,
    consecutive_failure: usize,
    node_state: Arc<Mutex<NodeState>>,
    request_method: String,
    request_url: String,
    request_body: Option<Vec<u8>>,
    request_timeout: Duration,
    client: Arc<Client>,
    host: String,
}

impl CommonHealthCheck {
    pub fn new(host: &str, path: &str, method: &str, state: Arc<Mutex<NodeState>>) -> Box<Self> {
        let request_url = format!("{}{}", host, path);

        Box::new(CommonHealthCheck {
            consecutive_success: 1,
            consecutive_failure: 1,
            node_state: Arc::clone(&state),
            request_method: method.to_string(),
            request_url: request_url.to_string(),
            request_body: None,
            request_timeout: Duration::from_secs(60),
            client: Arc::new(Client::new()),
            host: host.to_string(),
        })
    }

    pub fn with_request_body(mut self, body: Vec<u8>) -> Box<Self> {
        self.request_body = Some(body);
        Box::new(self)
    }

    fn update_health_status(&self, host: &str, is_healthy: bool) {
        let mut state = self.node_state.lock().unwrap();
        state.update_health_status(host, is_healthy);

        // update metrics
        set_node_health_gauge(&*state.node_name, host, is_healthy);
    }
}

#[async_trait]
impl HealthCheck for CommonHealthCheck {
    async fn check(&self, _target: &Backend) -> Result<()> {
        let client = self.client.clone();

        let method_result = Method::from_bytes(self.request_method.as_bytes());
        let method = match method_result {
            Ok(m) => m,
            Err(e) => {
                log::error!(
                    "invalid request method: {}, error: {}",
                    self.request_method,
                    e
                );
                self.update_health_status(&self.host, false);

                return Error::e_explain(Custom("invalid request method"), "reqwest error");
            }
        };

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let request_builder = client
            .request(method, &self.request_url)
            .headers(headers)
            .timeout(self.request_timeout);

        let request_builder = if let Some(body) = self.request_body.as_ref() {
            request_builder.body(body.clone())
        } else {
            request_builder
        };

        let response = request_builder.send().await;
        let response = match response {
            Ok(r) => r,
            Err(_e) => {
                log::error!("failed to send request, error: {}", _e);
                self.update_health_status(&self.host, false);

                return Error::e_explain(Custom("failed to send request"), "reqwest error");
            }
        };

        // only check the status code
        if !response.status().is_success() {
            log::error!(
                "request failed, status code: {}",
                response.status().as_u16()
            );
            self.update_health_status(&self.host, false);

            return Error::e_explain(Custom("request failed"), "reqwest error");
        }

        self.update_health_status(&self.host, true);

        Ok(())
    }

    fn health_threshold(&self, success: bool) -> usize {
        if success {
            self.consecutive_success
        } else {
            self.consecutive_failure
        }
    }
}
