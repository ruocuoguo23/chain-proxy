# Chain Proxy

## Overview

Chain Proxy is a robust and dynamic proxy service designed for blockchain services. Utilizing the third-party
library `pingora`, Chain Proxy intelligently routes requests to the healthiest nodes in the network, ensuring high
availability and reliability of blockchain operations.

## Features

- **Dynamic Node Selection**: Chain Proxy monitors the health of downstream nodes and dynamically selects the optimal
  one for request forwarding.
- **High Availability**: Designed with fault tolerance in mind, it provides uninterrupted service even when individual
  nodes experience issues.
- **Easy Integration**: Seamlessly integrates with existing blockchain services without extensive configuration.
- **Powered by Pingora**: Leverages the `pingora` library for efficient network health checks and load balancing.

## Getting Started

To get started with Chain Proxy, follow these steps:

### Prerequisites

Ensure you have Rust installed on your system. You can install Rust using `rustup` by following the instructions on
the [official Rust website](https://www.rust-lang.org/tools/install).

### Installation

1. Clone the repository:

```sh
git clone https://github.com/your-username/chain-proxy.git
cd chain-proxy
```

2. Build the project:

```sh
cargo build --release
```

3. Run the proxy:
   set the 'CONFIG_PATH' environment variable to the path of the config.yaml file

```sh
CONFIG_PATH=path/to/config.yaml cargo run --release
```

### Configuration

Chain Proxy can be configured by editing the config.toml file. Here you can specify the nodes, their respective
health check intervals, and other relevant settings.

```yaml
# Example config.toml:
Chains:
  - Name: solana
    Listen: 1017
    Interval: 20
    BlockGap: 20
    Nodes:
      - Address: https://tiniest-wandering-flower.solana-mainnet.quiknode.pro/3f2cf77b66958c08189f7d289df7d0740e554be2
        Priority: 1
      - Address: https://api.mainnet-beta.solana.com
        Priority: 0
  - Name: ethereum
    Listen: 1090
    Interval: 20
    BlockGap: 20
    Nodes:
      - Address: https://tiniest-wandering-flower.solana-mainnet.quiknode.pro/3f2cf77b66958c08189f7d289df7d0740e554be2
        Priority: 1
      - Address: https://api.mainnet-beta.solana.com
        Priority: 0
```

## Usage

Once Chain Proxy is running, it will listen for incoming blockchain requests and forward them to the most suitable node
based on the current health status and response times.

## Contributing

Contributions are welcome! Feel free to open a pull request or an issue if you have suggestions or encounter any
problems.

## License

Chain Proxy is licensed under the MIT License - see the LICENSE file for details.

