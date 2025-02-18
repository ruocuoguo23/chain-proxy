use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const LOG_CONFIG: &str = r#"
refresh_rate: 30 seconds
appenders:
  stdout:
    kind: console
  file:
    kind: rolling_file
    path: "logs/chain_proxy.log"
    policy:
      kind: compound
      trigger:
        kind: time
        interval: 1 day # rotate log file every day
      roller:
        kind: fixed_window
        pattern: "logs/chain_proxy.{}.log"
        base: 1
        count: 10 # ten days logs will be kept
root:
  level: info
  appenders:
    - stdout
    - file
"#;

#[derive(Debug, Serialize, Deserialize)]
pub struct Node {
    #[serde(rename = "Address")]
    address: String,
    #[serde(rename = "Priority")]
    priority: i32,
    #[serde(rename = "UserName", default)]
    user_name: Option<String>,
    #[serde(rename = "Pass", default)]
    pass: Option<String>,
    #[serde(rename = "CustomHeaders", default)]
    custom_headers: Option<HashMap<String, String>>, // New field for custom headers
}

impl Node {
    pub fn address(&self) -> &str {
        self.address.as_str()
    }

    pub fn priority(&self) -> i32 {
        self.priority
    }

    pub fn user_name(&self) -> Option<&String> {
        self.user_name.as_ref()
    }

    pub fn pass(&self) -> Option<&String> {
        self.pass.as_ref()
    }

    pub fn custom_headers(&self) -> Option<&HashMap<String, String>> {
        self.custom_headers.as_ref()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthCheck {
    #[serde(rename = "Path")]
    path: String,
    #[serde(rename = "Method")]
    method: String,
    #[serde(rename = "RequestBody", default)]
    request_body: String,
}

impl HealthCheck {
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn method(&self) -> &str {
        self.method.as_str()
    }

    pub fn request_body(&self) -> &str {
        self.request_body.as_str()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpecialMethodConfig {
    #[serde(rename = "MethodName")]
    pub method_name: String,
    #[serde(rename = "Nodes")]
    pub nodes: Vec<Node>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Chain {
    #[serde(rename = "Name")]
    name: String,

    // Protocol is used to distinguish different proxy protocol, for example, "http", "jsonrpc"
    #[serde(rename = "Protocol")]
    protocol: String,

    // ChainType is used to distinguish different chains, for example, "ethereum", "solana"
    // different chain may have different health check api
    #[serde(rename = "ChainType")]
    chain_type: String,
    #[serde(rename = "Listen")]
    listen: u16,
    #[serde(rename = "Interval")]
    interval: u64,
    #[serde(rename = "BlockGap")]
    block_gap: u64,

    // LogRequest is used to log the request and response, default is false
    #[serde(rename = "LogRequest", default)]
    log_request: bool,

    #[serde(rename = "Nodes")]
    nodes: Vec<Node>,
    #[serde(rename = "HealthCheck")]
    health_check: HealthCheck,
    #[serde(rename = "SpecialMethods")]
    special_methods: Option<Vec<SpecialMethodConfig>>
}

impl Chain {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    pub fn chain_type(&self) -> &str {
        &self.chain_type
    }

    pub fn listen(&self) -> u16 {
        self.listen
    }

    pub fn interval(&self) -> u64 {
        self.interval
    }

    pub fn block_gap(&self) -> u64 {
        self.block_gap
    }

    pub fn log_request(&self) -> bool {
        self.log_request
    }

    pub fn nodes(&self) -> &Vec<Node> {
        &self.nodes
    }

    pub fn health_check(&self) -> &HealthCheck {
        &self.health_check
    }

    pub fn special_methods(&self) -> Option<&Vec<SpecialMethodConfig>> {
        self.special_methods.as_ref()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Common {
    #[serde(rename = "Name")]
    name: String,

    // Protocol is used to distinguish different proxy protocol, for example, "http", "jsonrpc"
    #[serde(rename = "Protocol")]
    protocol: String,

    #[serde(rename = "Listen")]
    listen: u16,

    #[serde(rename = "Interval")]
    interval: u64,

    // LogRequest is used to log the request and response, default is false
    #[serde(rename = "LogRequest", default)]
    log_request: bool,

    #[serde(rename = "Nodes")]
    nodes: Vec<Node>,

    #[serde(rename = "HealthCheck")]
    health_check: HealthCheck,

    #[serde(rename = "SpecialMethods")]
    special_methods: Option<Vec<SpecialMethodConfig>>,
}

impl Common {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    pub fn listen(&self) -> u16 {
        self.listen
    }

    pub fn interval(&self) -> u64 {
        self.interval
    }

    pub fn log_request(&self) -> bool {
        self.log_request
    }

    pub fn nodes(&self) -> &Vec<Node> {
        &self.nodes
    }

    pub fn health_check(&self) -> &HealthCheck {
        &self.health_check
    }

    pub fn special_methods(&self) -> Option<&Vec<SpecialMethodConfig>> {
        self.special_methods.as_ref()
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Monitor {
    #[serde(rename = "Listen")]
    listen: u16,
    #[serde(rename = "System")]
    system: String,
}

impl Monitor {
    pub fn listen(&self) -> u16 {
        self.listen
    }

    pub fn system(&self) -> &str {
        self.system.as_str()
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(rename = "Chains", default)]
    pub(crate) chains: Vec<Chain>,

    #[serde(rename = "Commons", default)]
    pub(crate) commons: Vec<Common>,

    #[serde(rename = "Monitor")]
    pub(crate) monitor: Monitor,

    #[serde(rename = "UnifyProxyListenPort", default)]
    pub(crate) unify_proxy_listen_port: Option<u16>,
}

impl Config {
    pub fn load_config<P: AsRef<Path>>(path: P) -> Result<(), Box<dyn Error>> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config = serde_yaml::from_str(&contents)?;
        *crate::CONFIG.write().unwrap() = config;

        Ok(())
    }
}

#[derive(Debug)]
pub struct ChainState {
    // store the chain name
    pub(crate) chain_name: String,

    // store chain node hostname and current block number
    pub(crate) block_numbers: HashMap<String, u64>,
}

impl ChainState {
    pub fn new(chain_name: &str) -> Self {
        ChainState {
            chain_name: chain_name.to_string(),
            block_numbers: HashMap::new(),
        }
    }

    pub fn update_block_number(&mut self, host_name: &str, block_number: u64) {
        self.block_numbers
            .insert(host_name.to_string(), block_number);
    }

    pub fn get_block_numbers(&self) -> &HashMap<String, u64> {
        &self.block_numbers
    }
}

#[derive(Debug)]
pub struct NodeState {
    // store the node name
    pub(crate) node_name: String,

    // host name and health status
    pub(crate) health_status: HashMap<String, bool>,
}

impl NodeState {
    pub fn new(node_name: &str) -> Self {
        NodeState {
            node_name: node_name.to_string(),
            health_status: HashMap::new(),
        }
    }

    pub fn update_health_status(&mut self, host_name: &str, is_healthy: bool) {
        self.health_status.insert(host_name.to_string(), is_healthy);
    }
}

/// Stores mapping of `chain_type -> chain_name -> port`
#[derive(Debug, Clone)]
pub struct UnifyProxyConfig {
    chain_ports: HashMap<String, HashMap<String, u16>>, // chain_type -> chain_name -> port

    listen_port: u16, // port for the proxy
}

impl UnifyProxyConfig {
    /// Create a new `UnifyProxyConfig` from `Config`
    pub fn from_config(config: &Config) -> Self {
        let mut chain_ports = HashMap::new();

        for chain in &config.chains {
            let chain_type = chain.chain_type().to_string();
            let chain_name = chain.name().to_string();
            let port = chain.listen();

            chain_ports
                .entry(chain_type)
                .or_insert_with(HashMap::new)
                .insert(chain_name, port);
        }

        let listen_port = config.unify_proxy_listen_port.unwrap_or(9999);

        UnifyProxyConfig { chain_ports, listen_port }
    }

    /// Get the port for a given `chain_type` and `chain_name`
    pub fn get_port(&self, chain_type: &str, chain_name: &str) -> Option<u16> {
        self.chain_ports
            .get(chain_type)
            .and_then(|chains| chains.get(chain_name))
            .cloned()
    }

    /// Get the listen port for the unified proxy
    pub fn listen_port(&self) -> u16 {
        self.listen_port
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    // Helper function to create a temporary config file
    fn create_temp_config(content: &str) -> std::io::Result<tempfile::NamedTempFile> {
        let mut file = tempfile::Builder::new().suffix(".yaml").tempfile()?;
        file.write_all(content.as_bytes())?;
        Ok(file)
    }

    #[test]
    fn test_load_config() {
        // YAML content to be used for testing.
        let yaml_content = r#"
Chains:
  - Name: solana
    Protocol: "jsonrpc"
    Listen: 1017
    Interval: 20
    BlockGap: 20
    ChainType: "solana"
    Nodes:
      - Address: https://example.com/solana
        Priority: 1
      - Address: https://api.mainnet-beta.solana.com
        Priority: 0
    HealthCheck:
      Path: /health1
      Method: GET
  - Name: ethereum
    Protocol: "jsonrpc"
    Listen: 1090
    Interval: 20
    BlockGap: 20
    ChainType: "ethereum"
    Nodes:
      - Address: https://example.com/ethereum
        Priority: 1
      - Address: https://api.ethereum.org
        Priority: 0
    SpecialMethods:
      - MethodName: "debug_"
        Nodes:
          - Address: http://127.0.0.1:22260
            Priority: 1
          - Address: https://special-node.infura.io/v3/559af310b68646d8accf0cf36111f2eb
            Priority: 0
      - MethodName: "/special"
        Nodes:
          - Address: http://127.0.0.1:33360
            Priority: 1
          - Address: https://another-special-node.infura.io/v3/559af310b68646d8accf0cf36111f2eb
            Priority: 0
    HealthCheck:
      Path: /health2
      Method: GET
Commons:
  - Name: common1
    Protocol: "jsonrpc"
    Listen: 2020
    Interval: 30
    Nodes:
      - Address: https://example.com/common1
        Priority: 1
      - Address: https://api.common1.com
        Priority: 0
    HealthCheck:
      Path: /health3
      Method: GET
      RequestBody: "test"
  - Name: common2
    Protocol: "jsonrpc"
    Listen: 2030
    Interval: 40
    Nodes:
      - Address: https://example.com/common2
        Priority: 1
      - Address: https://api.common2.com
        Priority: 0
    SpecialMethods:
      - MethodName: "trace_"
        Nodes:
          - Address: http://127.0.0.1:44460
            Priority: 1
          - Address: https://special-node.common2.com/v3/559af310b68646d8accf0cf36111f2eb
            Priority: 0
    HealthCheck:
      Path: /health4
      Method: GET

Monitor:
    Listen: 1018
    System: "test"
"#;

        // Create a temporary config file
        let file = create_temp_config(yaml_content).unwrap();

        // Load the config from the temporary file
        assert!(Config::load_config(file.path()).is_ok());

        let config = crate::CONFIG.read().unwrap();
        // Assert the config values
        assert_eq!(config.chains.len(), 2);
        assert_eq!(config.chains[0].name(), "solana");
        assert_eq!(config.chains[0].listen(), 1017);
        assert_eq!(config.chains[0].interval(), 20);
        assert_eq!(config.chains[0].block_gap(), 20);
        assert_eq!(config.chains[0].nodes().len(), 2);
        assert_eq!(
            config.chains[0].nodes()[0].address,
            "https://example.com/solana"
        );
        assert_eq!(config.chains[0].nodes()[0].priority, 1);

        assert_eq!(config.chains[0].health_check().path(), "/health1");
        assert_eq!(config.chains[0].health_check().method(), "GET");

        assert_eq!(config.monitor.listen(), 1018);

        // Assert SpecialMethods for ethereum chain
        let special_methods = config.chains[1].special_methods().unwrap();
        assert_eq!(special_methods.len(), 2);

        assert_eq!(special_methods[0].nodes.len(), 2);
        assert_eq!(special_methods[0].nodes[0].address, "http://127.0.0.1:22260");
        assert_eq!(special_methods[0].nodes[0].priority, 1);
        assert_eq!(special_methods[0].nodes[1].address, "https://special-node.infura.io/v3/559af310b68646d8accf0cf36111f2eb");
        assert_eq!(special_methods[0].nodes[1].priority, 0);

        assert_eq!(special_methods[1].nodes.len(), 2);
        assert_eq!(special_methods[1].nodes[0].address, "http://127.0.0.1:33360");
        assert_eq!(special_methods[1].nodes[0].priority, 1);
        assert_eq!(special_methods[1].nodes[1].address, "https://another-special-node.infura.io/v3/559af310b68646d8accf0cf36111f2eb");
        assert_eq!(special_methods[1].nodes[1].priority, 0);

        // Assert Commons
        assert_eq!(config.commons.len(), 2);
        assert_eq!(config.commons[0].name(), "common1");
        assert_eq!(config.commons[0].listen(), 2020);
        assert_eq!(config.commons[0].interval(), 30);
        assert_eq!(config.commons[0].nodes().len(), 2);
        assert_eq!(
            config.commons[0].nodes()[0].address,
            "https://example.com/common1"
        );
        assert_eq!(config.commons[0].nodes()[0].priority, 1);

        assert_eq!(config.commons[0].health_check().path(), "/health3");
        assert_eq!(config.commons[0].health_check().method(), "GET");

        let special_methods_common2 = config.commons[1].special_methods().unwrap();
        assert_eq!(special_methods_common2.len(), 1);

        assert_eq!(special_methods_common2[0].nodes.len(), 2);
        assert_eq!(special_methods_common2[0].nodes[0].address, "http://127.0.0.1:44460");
        assert_eq!(special_methods_common2[0].nodes[0].priority, 1);
        assert_eq!(special_methods_common2[0].nodes[1].address, "https://special-node.common2.com/v3/559af310b68646d8accf0cf36111f2eb");
        assert_eq!(special_methods_common2[0].nodes[1].priority, 0);
    }
}
