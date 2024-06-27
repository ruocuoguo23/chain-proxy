use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use url::{ParseError, Url};

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
        kind: size
        limit: 10mb # when the file size exceeds 10mb, a rollover will be triggered
      roller:
        kind: fixed_window
        pattern: "logs/chain_proxy.{}.log"
        base: 1
        count: 5
root:
  level: debug
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
}

impl Node {
    pub fn address(&self) -> &str {
        self.address.as_str()
    }

    pub fn tls(&self) -> bool {
        self.address.starts_with("https")
    }

    pub fn hostname(&self) -> Result<String, ParseError> {
        let url = Url::parse(self.address.as_str())?;
        url.host_str()
            .map(|host| host.to_string())
            .ok_or(ParseError::EmptyHost)
    }

    pub fn priority(&self) -> i32 {
        self.priority
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthCheck {
    #[serde(rename = "Path")]
    path: String,
    #[serde(rename = "Method")]
    method: String,
}

impl HealthCheck {
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn method(&self) -> &str {
        self.method.as_str()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Chain {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Listen")]
    listen: u16,
    #[serde(rename = "Interval")]
    interval: u64,
    #[serde(rename = "BlockGap")]
    block_gap: u64,
    #[serde(rename = "Nodes")]
    nodes: Vec<Node>,
    #[serde(rename = "HealthCheck")]
    health_check: HealthCheck,
}

impl Chain {
    pub fn name(&self) -> &str {
        &self.name
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

    pub fn nodes(&self) -> &Vec<Node> {
        &self.nodes
    }

    pub fn health_check(&self) -> &HealthCheck {
        &self.health_check
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(rename = "Chains")]
    pub(crate) chains: Vec<Chain>,
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
    // store chain node hostname and current block number
    pub(crate) block_numbers: HashMap<String, u64>,
}

impl ChainState {
    pub fn new() -> Self {
        ChainState {
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
    Listen: 1017
    Interval: 20
    BlockGap: 20
    Nodes:
      - Address: https://example.com/solana
        Priority: 1
      - Address: https://api.mainnet-beta.solana.com
        Priority: 0
    HealthCheck:
      Path: /health1
      Method: GET
  - Name: ethereum
    Listen: 1090
    Interval: 20
    BlockGap: 20
    Nodes:
      - Address: https://example.com/ethereum
        Priority: 1
      - Address: https://api.ethereum.org
        Priority: 0
    HealthCheck:
      Path: /health2
      Method: GET
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
    }
}
