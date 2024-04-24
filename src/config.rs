use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::error::Error;

#[derive(Debug, Serialize, Deserialize)]
struct Node {
    #[serde(rename = "Address")]
    address: String,
    #[serde(rename = "Priority")]
    priority: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Chain {
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
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    #[serde(rename = "Chains")]
    chains: Vec<Chain>,
}

impl Config {
    pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config = serde_yaml::from_str(&contents)?;
        Ok(config)
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
  - Name: ethereum
    Listen: 1090
    Interval: 20
    BlockGap: 20
    Nodes:
      - Address: https://example.com/ethereum
        Priority: 1
      - Address: https://api.ethereum.org
        Priority: 0
"#;

        // Create a temporary config file
        let file = create_temp_config(yaml_content).unwrap();

        // Load the config from the temporary file
        let config = Config::load_config(file.path()).unwrap();

        // Assert the config values
        assert_eq!(config.chains.len(), 2);
        assert_eq!(config.chains[0].name, "solana");
        assert_eq!(config.chains[0].listen, 1017);
        assert_eq!(config.chains[0].interval, 20);
        assert_eq!(config.chains[0].block_gap, 20);
        assert_eq!(config.chains[0].nodes.len(), 2);
        assert_eq!(config.chains[0].nodes[0].address, "https://example.com/solana");
        assert_eq!(config.chains[0].nodes[0].priority, 1);
    }
}