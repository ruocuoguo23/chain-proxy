use crate::config::ChainState;
use crate::metrics::set_node_height_gauge;
use async_trait::async_trait;
use pingora::{Custom, Error, Result};
use pingora_load_balancing::health_check::HealthCheck;
use pingora_load_balancing::Backend;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, HeaderName};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

    // register the cardano chain checker of cardano
    let cardano_checker = ChainChecker {
        validator: Arc::new(rosetta_validator),
        request_body: r#"
            {
                "network_identifier": {
                    "blockchain": "cardano",
                    "network": "mainnet"
                }
            }
        "#
        .as_bytes()
        .to_vec(),
    };
    register_chain_checker("cardano", cardano_checker);

    // register the icp chain checker of icp
    let icp_checker = ChainChecker {
        validator: Arc::new(rosetta_validator),
        request_body: r#"
            {
                "network_identifier": {
                    "blockchain": "Internet Computer",
                    "network": "00000000000000020101"
                }
            }
        "#
        .as_bytes()
        .to_vec(),
    };
    register_chain_checker("icp", icp_checker);

    // register the solana chain checker
    let solana_checker = ChainChecker {
        validator: Arc::new(solana_validator),
        request_body: r#"
                {
                    "jsonrpc":"2.0",
                    "method":"getEpochInfo",
                    "id":1
               }
               "#
        .as_bytes()
        .to_vec(),
    };
    register_chain_checker("solana", solana_checker);

    // register the bitcoin chain checker
    let bitcoin_checker = ChainChecker {
        validator: Arc::new(bitcoin_validator),
        request_body: r#"
                {
                    "jsonrpc":"1.0",
                    "method":"getblockcount",
                    "id":"1.0",
                    "params":[]
               }
               "#
        .as_bytes()
        .to_vec(),
    };
    register_chain_checker("bitcoin", bitcoin_checker);

    // Register the tron chain checker
    let tron_checker = ChainChecker {
        validator: Arc::new(tron_validator),
        // Request body is empty for Tron
        request_body: "".as_bytes().to_vec(),
    };
    register_chain_checker("tron", tron_checker);

    // Register the tron gRPC chain checker
    let tron_grpc_checker = ChainChecker {
        validator: Arc::new(tron_grpc_validator),
        // Request body is empty for Tron gRPC
        request_body: "".as_bytes().to_vec(),
    };
    register_chain_checker("tron_grpc", tron_grpc_checker);

    // Register the stellar chain checker
    let stellar_checker = ChainChecker {
        validator: Arc::new(stellar_validator),
        request_body: "".as_bytes().to_vec(), // Stellar does not require a request body
    };
    register_chain_checker("stellar", stellar_checker);

    // Register the algorand chain checker
    let algorand_checker = ChainChecker {
        validator: Arc::new(algorand_validator),
        request_body: vec![], // Algorand does not require a request body
    };
    register_chain_checker("algorand", algorand_checker);

    // Register the ton chain checker
    let ton_checker = ChainChecker {
        validator: Arc::new(ton_validator),
        request_body: vec![], // TON does not require a request body
    };
    register_chain_checker("ton", ton_checker);

    // Register Polkadot health check using `system_syncState`
    let polkadot_checker = ChainChecker {
        validator: Arc::new(polkadot_validator),
        request_body: r#"
                {
                    "jsonrpc": "2.0",
                    "method": "system_syncState",
                    "id": 1
                }
                "#
            .as_bytes()
            .to_vec(),
    };
    register_chain_checker("polkadot", polkadot_checker);
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

/// Rosetta response and validator
#[derive(Debug, Serialize, Deserialize)]
struct RosettaJsonResponse {
    current_block_identifier: RosettaBlockIdentifier,
}

#[derive(Debug, Serialize, Deserialize)]
struct RosettaBlockIdentifier {
    index: u64,
    hash: String,
}

pub(crate) fn rosetta_validator(body: &[u8]) -> Result<u64> {
    // Try to parse the JSON response
    let parsed: Result<RosettaJsonResponse, serde_json::Error> = serde_json::from_slice(body);
    if parsed.is_err() {
        // Log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed = parsed.unwrap();
    // Return the block index
    Ok(parsed.current_block_identifier.index)
}

///
/// solana response and validator
#[derive(Debug, Serialize, Deserialize)]
struct SolanaJsonResponse {
    /// The key to check in the JSON response
    jsonrpc: String,
    id: u64,
    result: SolanaSlot,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SolanaSlot {
    absolute_slot: u64,
}

pub(crate) fn solana_validator(body: &[u8]) -> Result<u64> {
    // try to parse the JSON response
    let parsed = serde_json::from_slice(body);
    if parsed.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed: SolanaJsonResponse = parsed.unwrap();
    // check if the JSON response is valid
    if parsed.jsonrpc != "2.0" {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        Error::e_explain(Custom("invalid jsonrpc"), "during http healthcheck")
    } else {
        // from hex string to u64
        let block_number = parsed.result.absolute_slot;
        Ok(block_number)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct BitcoinJsonResponse {
    /// The key to check in the JSON response
    id: String,
    result: u64,
}

pub(crate) fn bitcoin_validator(body: &[u8]) -> Result<u64> {
    // try to parse the JSON response
    let parsed: std::result::Result<BitcoinJsonResponse, serde_json::Error> =
        serde_json::from_slice(body);
    if parsed.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed: BitcoinJsonResponse = parsed.unwrap();
    // check if the JSON response is valid
    if parsed.id != "1.0" {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        Error::e_explain(Custom("invalid jsonrpc"), "during http healthcheck")
    } else {
        // from hex string to u64
        let block_number = parsed.result;
        Ok(block_number)
    }
}

/// Tron response and validator
#[derive(Debug, Serialize, Deserialize)]
struct TronJsonResponse {
    block_header: TronBlockHeader,
}

#[derive(Debug, Serialize, Deserialize)]
struct TronBlockHeader {
    raw_data: TronBlockRawData,
}

#[derive(Debug, Serialize, Deserialize)]
struct TronBlockRawData {
    number: u64,
}

pub(crate) fn tron_validator(body: &[u8]) -> Result<u64> {
    // Try to parse the JSON response
    let parsed: Result<TronJsonResponse, serde_json::Error> = serde_json::from_slice(body);
    if parsed.is_err() {
        // Log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed = parsed.unwrap();

    // Extract the block number
    Ok(parsed.block_header.raw_data.number)
}

pub(crate) fn tron_grpc_validator(_body: &[u8]) -> Result<u64> {
    // Tron gRPC health check always returns 1000
    Ok(1000)
}

#[derive(Debug, Serialize, Deserialize)]
struct StellarLedgerResponse {
    _embedded: EmbeddedRecords,
}

#[derive(Debug, Serialize, Deserialize)]
struct EmbeddedRecords {
    records: Vec<LedgerRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LedgerRecord {
    sequence: u64,
}

pub(crate) fn stellar_validator(body: &[u8]) -> Result<u64> {
    // Attempt to parse the JSON response
    let parsed: Result<StellarLedgerResponse, serde_json::Error> = serde_json::from_slice(body);
    if parsed.is_err() {
        // If parsing fails, log the body and return an error
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed = parsed.unwrap();

    // Ensure the records array contains at least one element
    if parsed._embedded.records.is_empty() {
        log::error!("no records found in response: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("no records found"), "during http healthcheck");
    }

    // Extract the sequence from the first record as the block height
    let block_number = parsed._embedded.records[0].sequence;

    Ok(block_number)
}

/// Algorand response and validator
#[derive(Debug, Serialize, Deserialize)]
struct AlgorandJsonResponse {
    #[serde(rename = "last-round")]
    last_round: u64,
}

pub(crate) fn algorand_validator(body: &[u8]) -> Result<u64> {
    // try to parse the JSON response
    let parsed: std::result::Result<AlgorandJsonResponse, serde_json::Error> =
        serde_json::from_slice(body);
    if parsed.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed: AlgorandJsonResponse = parsed.unwrap();
    // check if the JSON response is valid
    Ok(parsed.last_round)
}

/// TON response and validator
#[derive(Debug, Serialize, Deserialize)]
struct TonJsonResponse {
    ok: bool,
    result: TonResult,
}

#[derive(Debug, Serialize, Deserialize)]
struct TonResult {
    last: TonLastBlock,
}

#[derive(Debug, Serialize, Deserialize)]
struct TonLastBlock {
    seqno: u64,
}

pub(crate) fn ton_validator(body: &[u8]) -> Result<u64> {
    // try to parse the JSON response
    let parsed: std::result::Result<TonJsonResponse, serde_json::Error> = serde_json::from_slice(body);
    if parsed.is_err() {
        // log the body
        log::error!("failed to parse json: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid json"), "during http healthcheck");
    }

    let parsed: TonJsonResponse = parsed.unwrap();

    // check if the JSON response is valid
    if !parsed.ok {
        log::error!("TON API response not ok: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("invalid response"), "during http healthcheck");
    }

    // parse seqno from the response as block number
    let block_number = parsed.result.last.seqno;
    Ok(block_number)
}

/// Polkadot JSON-RPC response structure for `system_syncState`
#[derive(Debug, Serialize, Deserialize)]
struct PolkadotSyncStateResponse {
    jsonrpc: String,
    id: u64,
    result: SyncStateResult,
}

/// Structure containing the sync state details
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")] // Convert JSON camelCase to Rust struct fields
struct SyncStateResult {
    current_block: u64,  // Current block height
    highest_block: u64,  // Highest known block height
    starting_block: u64, // Starting block height
}

/// Parse Polkadot response to extract the latest block height
pub(crate) fn polkadot_validator(body: &[u8]) -> Result<u64> {
    let parsed: PolkadotSyncStateResponse = match serde_json::from_slice(body) {
        Ok(data) => data,
        Err(_) => {
            log::error!("Failed to parse Polkadot JSON: {}", String::from_utf8_lossy(body));
            return Error::e_explain(Custom("Invalid JSON"), "during Polkadot health check");
        }
    };

    if parsed.jsonrpc != "2.0" {
        log::error!("Invalid JSON-RPC response: {}", String::from_utf8_lossy(body));
        return Error::e_explain(Custom("Invalid JSON-RPC"), "during Polkadot health check");
    }

    Ok(parsed.result.highest_block)
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

    /// Optional Basic Auth credentials (username and password)
    pub authorization: Option<(String, String)>,

    /// Optional custom headers for the request
    pub custom_headers: Option<HashMap<String, String>>,
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
            authorization: None,
            custom_headers: None,
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

    /// Set Basic Auth credentials
    pub fn with_basic_auth(mut self, username: &str, password: &str) -> Box<Self> {
        self.authorization = Some((username.to_string(), password.to_string()));
        Box::new(self)
    }

    /// Set custom headers for the request
    pub fn with_custom_headers(mut self, headers: HashMap<String, String>) -> Box<Self> {
        self.custom_headers = Some(headers);
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
                    "Host: {}, invalid request method: {}, error: {}",
                    self.host,
                    self.request_method,
                    e
                );
                return Error::e_explain(Custom("invalid request method"), "reqwest error");
            }
        };

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Add Basic Auth header if authorization is set
        if let Some((username, password)) = &self.authorization {
            let auth_value = format!("Basic {}", base64::encode(format!("{}:{}", username, password)));
            headers.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&auth_value).unwrap(),
            );
        }

        // Add custom headers if provided
        if let Some(custom_headers) = &self.custom_headers {
            for (key, value) in custom_headers {
                headers.insert(
                    HeaderName::from_lowercase(key.as_ref()).unwrap(),
                    HeaderValue::from_str(value).unwrap(),
                );
            }
        }

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
                log::error!(
                    "Host: {}, failed to send request, error: {}",
                    self.host,
                    _e
                );
                return Error::e_explain(Custom("failed to send request"), "reqwest error");
            }
        };

        let response_body = response.bytes().await;
        let response_body = match response_body {
            Ok(b) => b,
            Err(_e) => {
                log::error!(
                    "Host: {}, failed to read response body, error: {}",
                    self.host,
                    _e
                );
                return Error::e_explain(Custom("failed to read response body"), "reqwest error");
            }
        };

        if let Some(validator) = self.validator.as_ref() {
            let chain_state_result = validator(&response_body);
            if chain_state_result.is_err() {
                log::error!(
                    "Host: {}, failed to validate response body",
                    self.host
                );

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
            ext: Default::default(),
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
            ext: Default::default(),
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
            ext: Default::default(),
        };

        assert!(http_check.check(&backend).await.is_ok());
    }

    #[tokio::test]
    async fn test_solana_check() {
        initialize_logger();
        log::info!("running solana check");
        let http_check = ChainHealthCheck::new(
            "https://api.devnet.solana.com",
            "",
            "POST",
            Arc::new(Mutex::new(ChainState::new("solana"))),
        );
        let http_check = http_check.with_response_body_validator(Arc::new(solana_validator));
        let http_check = http_check.with_request_body(
            r#"
                 {
                    "jsonrpc":"2.0",
                    "method":"getEpochInfo",
                    "id":1
               }
               "#
            .as_bytes()
            .to_vec(),
        );

        let backend = Backend {
            addr: SocketAddr::Inet("158.178.243.130:443".parse().unwrap()),
            weight: 1,
            ext: Default::default(),
        };

        assert!(http_check.check(&backend).await.is_ok());
    }

    #[tokio::test]
    async fn test_bitcoin_check() {
        initialize_logger();
        log::info!("running bitcoin check");
        let http_check = ChainHealthCheck::new(
            "https://go.getblock.io/499694bbd2704b6b99fff51ed2b324ab",
            "",
            "POST",
            Arc::new(Mutex::new(ChainState::new("bitcoin"))),
        );
        let http_check = http_check.with_response_body_validator(Arc::new(bitcoin_validator));
        let http_check = http_check.with_request_body(
            r#"
                 {
                    "jsonrpc":"1.0",
                    "method":"getblockcount",
                    "id":"1.0",
                    "params":[]
               }
               "#
            .as_bytes()
            .to_vec(),
        );

        let backend = Backend {
            addr: SocketAddr::Inet("158.178.243.130:443".parse().unwrap()),
            weight: 1,
            ext: Default::default(),
        };

        assert!(http_check.check(&backend).await.is_ok());
    }
}
