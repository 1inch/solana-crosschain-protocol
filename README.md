# Solana Cross-Chain Protocol

A secure and efficient cross-chain escrow protocol built on Solana, enabling trustless token swaps between different blockchain networks using hash time-locked contracts (HTLCs).

## Overview

The Solana Cross-Chain Protocol facilitates atomic swaps between Solana and other blockchain networks through a sophisticated escrow system. It implements a two-sided escrow mechanism where tokens are locked on both the source and destination chains, ensuring that either both parties receive their tokens or the swap is cancelled and funds are returned.

## Key Features

- **Hash Time-Locked Contracts (HTLCs)**: Secure cross-chain swaps using cryptographic hash locks and time-based conditions
- **Partial Fills Support**: Orders can be filled partially using Merkle tree verification
- **Dutch Auction Mechanism**: Dynamic pricing for order and cancellation fees
- **Whitelist System**: Authorized resolvers can execute public functions for improved UX
- **Multi-Token Support**: Works with both native SOL and SPL tokens, supports SPL Token 2022 (with limited extensions)
- **Safety Deposits**: Incentivizes proper execution and covers transaction costs
- **Rescue Funds**: Recovery mechanism for stuck tokens after a timeout period

## Architecture

The protocol consists of three main programs:

### 1. Cross-Chain Escrow Source (`cross-chain-escrow-src`)
- **Program ID**: `4yBT18tBcWqCDK8p3RMXdmZMjHr3wJM7jM6HVYemEqGh`
- Handles order creation and escrow initialization on the source chain
- Manages withdrawals using secret revelation
- Implements cancellation logic with Dutch auction for fees

### 2. Cross-Chain Escrow Destination (`cross-chain-escrow-dst`)
- **Program ID**: `AMEAktCrii7mVFQKCM9i5hKES4YrV3zFagrawr8BY8pb`
- Creates matching escrows on the destination chain
- Processes withdrawals when secrets are revealed
- Handles cancellations after timeout periods

### 3. Whitelist Validator (`whitelist`)
- **Program ID**: `5XYZ3LMWECpC6u7BWLskMMNx4xWbXF44dpDxvkVqkHtA`
- Manages authorized resolvers who can execute public functions
- Provides access control for sensitive operations

## Installation

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) (v3.1.14+)
- [Anchor](https://www.anchor-lang.com/docs/installation) (v0.32.2)
- [Node.js](https://nodejs.org/) and Yarn

### Setup

1. Clone the repository:
```bash
git clone https://github.com/1inch/solana-crosschain-protocol.git
cd solana-crosschain-protocol
```

2. Install dependencies:
```bash
yarn install
```

3. Build the programs:
```bash
anchor build
```

4. Run tests:
```bash
yarn test
```

## Usage

### Creating an Order (Source Chain)

```rust
// Create an order on the source chain
pub fn create(
    ctx: Context<Create>,
    hashlock: [u8; 32], // Root of merkle tree if partially filled
    amount: u64,
    safety_deposit: u64,
    timelocks: [u64; 4],
    expiration_time: u32,
    asset_is_native: bool,
    dst_amount: [u64; 4],
    dutch_auction_data_hash: [u8; 32],
    max_cancellation_premium: u64,
    cancellation_auction_duration: u32,
    allow_multiple_fills: bool,
    salt: u64,
    _dst_chain_params: DstChainParams,
);
```

### Creating an Escrow (Source Chain)

```rust
// Taker creates escrow by filling the order
pub fn create_escrow(
    ctx: Context<CreateEscrow>,
    amount: u64,
    merkle_proof: Option<MerkleProof>,
    dutch_auction_data: AuctionData,
);
```

### Creating Matching Escrow (Destination Chain)

```rust
// Create matching escrow on destination chain
pub fn create(
        ctx: Context<Create>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        amount: u64,
        safety_deposit: u64,
        recipient: Pubkey,
        timelocks: [u64; 4],
        src_cancellation_timestamp: u32,
        asset_is_native: bool,
)
```

### Withdrawing Funds

```rust
// Withdraw by revealing the secret
pub fn withdraw(ctx: Context<Withdraw>, secret: [u8; 32])
```

## Timelock Stages

The protocol implements multiple time-based stages for security:

1. **Source Withdrawal**: When the taker can withdraw on the source chain
2. **Source Public Withdrawal**: When anyone can help withdraw (with incentive)
3. **Source Cancellation**: When the maker can cancel and reclaim funds
4. **Source Public Cancellation**: When anyone can help cancel (with incentive)
5. **Destination Withdrawal**: When withdrawal is allowed on destination chain
6. **Destination Public Withdrawal**: Public withdrawal period on destination
7. **Destination Cancellation**: Cancellation period on destination chain

## Security Considerations

- **Atomic Swaps**: Either both parties receive their tokens or the swap is cancelled
- **Time Windows**: Carefully calibrated timelock periods prevent race conditions
- **Safety Deposits**: Incentivize proper execution and cover gas costs
- **Merkle Trees**: Enable secure partial fills with cryptographic proofs
- **Whitelist Access**: Only authorized resolvers can execute public functions
- **Rescue Mechanism**: Funds can be recovered after extended timeout periods

## Development

### Running Tests

```bash
# Run all tests
yarn test

# Run with coverage
yarn coverage

# Run specific test file
yarn test ./programs/cross-chain-escrow-src/tests/test_cross_chain_escrow_src_native.ts
```

### Linting

```bash
# Check code style
yarn lint

# Fix linting issues
yarn lint:fix
```

### Deployment

Deploy individual programs:

```bash
# Deploy whitelist program
yarn deploy:whitelist --provider.cluster <cluster>

# Deploy source escrow program
yarn deploy:src --provider.cluster <cluster>

# Deploy destination escrow program
yarn deploy:dst --provider.cluster <cluster>
```

## Contributing

We welcome contributions to the Solana Cross-Chain Protocol! Please follow these guidelines:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Code Style

- Follow Rust formatting conventions (`cargo fmt`)
- Ensure all tests pass (`yarn test`)
- Add tests for new functionality
- Update documentation as needed

## License

This project is licensed under the ISC License - see the [package.json](package.json) file for details.

## Acknowledgments

Built by the 1inch team to enable secure cross-chain swaps on Solana.

## Contact

For questions and support, please open an issue in the GitHub repository.
