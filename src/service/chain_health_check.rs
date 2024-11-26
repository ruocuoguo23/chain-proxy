use crate::config::ChainState;
use async_trait::async_trait;
use pingora_load_balancing::health_check::HealthCheck;
use pingora_load_balancing::Backend;
use pingora::{Custom, Error, Result};
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::collections::HashMap;
use crate::metrics::set_node_height_gauge;

type Validator = Arc<dyn Fn(&[u8]) -> Result<u64> + Send + Sync>;

#[derive(Clone)]
pub struct ChainChecker {
    pub validator: Validator,
    pub request_body: Vec<u8>,
}

lazy_static! {
    static ref CHAIN_CHECKERS: Mutex<HashMap<String, ChainChecker>> = Mutex::new(HashMap::new());
}

/// register a chain checker
pub fn register_chain_checker(chain_type: &str, checker: ChainChecker) {
    let mut checkers = CHAIN_CHECKERS.lock().unwrap();
    checkers.insert(chain_type.to_string(), checker);
}

/// get a chain checker
/// return None if the chain checker is not found
pub fn get_chain_checker(chain_type: &str) -> Option<ChainChecker> {
    let checkers = CHAIN_CHECKERS.lock().unwrap();
    checkers.get(chain_type).cloned()
}

pub fn init_chain_checker() {
    // register the eth chain checker
    let ethereum_checker = ChainChecker {
        validator: Arc::new(eth_validator),
        request_body: r#"
                {
                    "jsonrpc":"2.0",
                    "method":"eth_blockNumber",
                    "id":1
               }
               "#
            .as_bytes()
            .to_vec(),
    };
    register_chain_checker("ethereum", ethereum_checker);

    // register the ripple chain checker
    let ripple_checker = ChainChecker {
        validator: Arc::new(ripple_validator),
        request_body: r#"
                {
                    "jsonrpc":"2.0",
                    "method":"ledger_closed",
                    "params":[{}],
                    "id":1
               }
               "#
            .as_bytes()
            .to_vec(),
    };
    register_chain_checker("ripple", ripple_checker);

    // register the cosmos chain checker
    let cosmos_checker = ChainChecker {
        validator: Arc::new(cosmos_validator),
        request_body: "".as_bytes().to_vec(),
    };
    register_chain_checker("cosmos", cosmos_checker);
}

/// Define various response validators for different chain, like ethereum, bitcoin, etc.
/// Eth response and validator
#[derive(Debug, Serialize, Deserialize)]
struct EthJsonResponse {
    /// The key to check in the JSON response
    jsonrpc: String,
    id: u64,
    result: String,
}

pub(crate) fn eth_validator(body: &[u8]) -> Result<u64> {
    // try to parse the JSON response
    let parsed = serde_json::from_slice(body);
    if parsed.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed: EthJsonResponse = parsed.unwrap();
    // check if the JSON response is valid
    if parsed.jsonrpc != "2.0" {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        Error::e_explain(Custom("invalid jsonrpc"), "during http healthcheck")
    } else {
        // from hex string to u64
        let block_number = u64::from_str_radix(&parsed.result[2..], 16);
        if block_number.is_err() {
            // log the body
            log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
            return Error::e_explain(Custom("invalid block number"), "during http healthcheck");
        }

        Ok(block_number.unwrap())
    }
}

/// ripple response and validator
#[derive(Debug, Serialize, Deserialize)]
struct RippleJsonResponse {
    /// The key to check in the JSON response
    result: RippleResult,
}

#[derive(Debug, Serialize, Deserialize)]
struct RippleResult {
    /// The key to check in the JSON response
    ledger_hash: String,
    ledger_index: u64,
    status: String,
}

pub(crate) fn ripple_validator(body: &[u8]) -> Result<u64> {
    // try to parse the JSON response
    let parsed: Result<RippleJsonResponse, serde_json::Error> = serde_json::from_slice(body);
    if parsed.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed = parsed.unwrap();

    // check if the JSON response is valid
    if parsed.result.status != "success" {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        Error::e_explain(Custom("invalid status"), "during http healthcheck")
    } else {
        Ok(parsed.result.ledger_index)
    }
}

/// cosmos response and validator
#[derive(Debug, Serialize, Deserialize)]
struct CosmosJsonResponse {
    /// The key to check in the JSON response
    block: CosmosBlock,
}

#[derive(Debug, Serialize, Deserialize)]
struct CosmosBlock {
    /// The key to check in the JSON response
    header: CosmosHeader,
}

#[derive(Debug, Serialize, Deserialize)]
struct CosmosHeader {
    /// The key to check in the JSON response
    height: String,
}

pub(crate) fn cosmos_validator(body: &[u8]) -> Result<u64> {
    // try to parse the JSON response
    let parsed: Result<CosmosJsonResponse, serde_json::Error> = serde_json::from_slice(body);
    if parsed.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed = parsed.unwrap();

    // from string to u64
    let block_number = parsed.block.header.height.parse::<u64>();
    if block_number.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid block number"), "during http healthcheck");
    }

    Ok(block_number.unwrap())
}

/// Chain health check
///
/// This health check checks if it can receive the expected HTTP(s) response from the given backend.
pub struct ChainHealthCheck {
    /// Number of successful checks to flip from unhealthy to healthy.
    pub consecutive_success: usize,
    /// Number of failed checks to flip from healthy to unhealthy.
    pub consecutive_failure: usize,

    pub chain_state: Arc<Mutex<ChainState>>,

    pub request_method: String,

    pub request_url: String,

    pub request_body: Option<Vec<u8>>,

    pub request_timeout: Duration,

    pub client: Arc<Client>,

    /// Optional field to define how to validate the response from the server.
    ///
    /// If not set, any response with a `200 OK` is considered a successful check.
    pub validator: Option<Validator>,

    pub host: String,
}

impl ChainHealthCheck {
    /// Create a new [ChainHealthCheck] with the following default settings
    /// * req: a GET/POST to the given path of the given host name
    /// * request_body: None
    /// * consecutive_success: 1
    /// * consecutive_failure: 1
    /// * validator: `None`, any 200 response is considered successful
    pub fn new(host: &str, path: &str, method: &str, state: Arc<Mutex<ChainState>>) -> Box<Self> {
        let request_url = format!("{}{}", host, path);

        Box::new(ChainHealthCheck {
            consecutive_success: 1,
            consecutive_failure: 1,
            chain_state: Arc::clone(&state),
            request_method: method.to_string(),
            request_url: request_url.to_string(),
            request_body: None,
            request_timeout: Duration::from_secs(60),
            client: Arc::new(Client::new()),
            validator: None,
            host: host.to_string(),
        })
    }

    /// Set the request body to send to the backend
    pub fn with_request_body(mut self, body: Vec<u8>) -> Box<Self> {
        self.request_body = Some(body);
        Box::new(self)
    }

    /// Set the response body validator
    pub fn with_response_body_validator(mut self, validator: Validator) -> Box<Self> {
        self.validator = Some(validator);
        Box::new(self)
    }
}

#[async_trait]
impl HealthCheck for ChainHealthCheck {
    async fn check(&self, _target: &Backend) -> Result<()> {
        let client = self.client.clone();

        let method_result = reqwest::Method::from_bytes(self.request_method.as_bytes());
        let method = match method_result {
            Ok(m) => m,
            Err(e) => {
                log::error!(
                    "invalid request method: {}, error: {}",
                    self.request_method,
                    e
                );
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
                return Error::e_explain(Custom("failed to send request"), "reqwest error");
            }
        };

        let response_body = response.bytes().await;
        let response_body = match response_body {
            Ok(b) => b,
            Err(_e) => {
                log::error!("failed to read response body, error: {}", _e);
                return Error::e_explain(Custom("failed to read response body"), "reqwest error");
            }
        };

        if let Some(validator) = self.validator.as_ref() {
            let chain_state_result = validator(&response_body);
            if chain_state_result.is_err() {
                log::error!("failed to validate response body");

                return Error::e_explain(
                    Custom("failed to validate response body"),
                    "validator error",
                );
            }

            // update the chain state
            let chain_state_result = chain_state_result?;

            {
                let mut state = self.chain_state.lock().unwrap();
                state.update_block_number(&self.host, chain_state_result);

                // metrics
                set_node_height_gauge(&state.chain_name, &self.host, chain_state_result);
            }
        }

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

#[cfg(test)]
mod test {
    use super::*;
    use once_cell::sync::OnceCell;
    use pingora::protocols::l4::socket::SocketAddr;

    static INIT: OnceCell<()> = OnceCell::new();

    // todo: move this to a common place
    pub fn initialize_logger() {
        INIT.get_or_init(|| {
            if let Err(e) = env_logger::builder().is_test(true).try_init() {
                eprintln!("Logger already initialized: {}", e);
            }
        });
    }

    #[tokio::test]
    async fn test_https_check_get() {
        initialize_logger();

        // create a health check that connects to httpbin.org over HTTPS
        let chain_health_check = ChainHealthCheck::new(
            "https://httpbin.org",
            "/get",
            "GET",
            Arc::new(Mutex::new(ChainState::new("test"))),
        );
        let backend = Backend {
            addr: SocketAddr::Inet("23.23.165.157:443".parse().unwrap()),
            weight: 1,
        };

        assert!(chain_health_check.check(&backend).await.is_ok());
    }

    #[tokio::test]
    async fn test_https_check_post() {
        initialize_logger();

        // create a health check that connects to httpbin.org over HTTPS
        let chain_health_check = ChainHealthCheck::new(
            "https://httpbin.org",
            "/post",
            "POST",
            Arc::new(Mutex::new(ChainState::new("test"))),
        );
        let http_check = chain_health_check.with_request_body(
            r#"
               {
                    "key":"value"
               }
               "#
            .as_bytes()
            .to_vec(),
        );
        let backend = Backend {
            addr: SocketAddr::Inet("23.23.165.157:443".parse().unwrap()),
            weight: 1,
        };

        assert!(http_check.check(&backend).await.is_ok());
    }

    #[tokio::test]
    async fn test_optimism_check() {
        initialize_logger();
        log::info!("running optimism check");
        let http_check = ChainHealthCheck::new(
            "https://practical-green-butterfly.optimism.quiknode.pro",
            "/d02f8d49bde8ccbbcec3c9a8962646db998ade83",
            "POST",
            Arc::new(Mutex::new(ChainState::new("test"))),
        );
        let http_check = http_check.with_response_body_validator(Arc::new(eth_validator));
        let http_check = http_check.with_request_body(
            r#"
                {
                    "jsonrpc":"2.0",
                    "method":"eth_blockNumber",
                    "id":1
               }
               "#
            .as_bytes()
            .to_vec(),
        );

        let backend = Backend {
            addr: SocketAddr::Inet("158.178.243.130:443".parse().unwrap()),
            weight: 1,
        };

        assert!(http_check.check(&backend).await.is_ok());
    }
}
