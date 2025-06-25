use rand::Rng;
use solana_airdrop_utils::{AirdropError, Network, airdrop, airdrop_lamports};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::Keypair};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_devnet_airdrop() {
    let mut rng = rand::rng();
    let recipient = Pubkey::from(rng.random::<[u8; 32]>());
    let lamports = 1_000_000; // 0.001 SOL
    let network = Network::Devnet;

    let result = airdrop(network, &recipient, lamports, 3).await;
    if let Err(AirdropError::RpcError(e)) = &result {
        eprintln!("RPC error: {}", e);
    }
    assert!(
        result.is_ok() || matches!(result, Err(AirdropError::RpcError(_))),
        "Devnet airdrop failed: {:?}",
        result.err()
    );

    if result.is_ok() {
        let client = RpcClient::new(network.rpc_url());
        let balance = client
            .get_balance(&recipient)
            .expect("Failed to get balance");
        assert!(
            balance >= lamports,
            "Balance {} is less than requested {} lamports",
            balance,
            lamports
        );
    }
}

#[test]
fn test_airdrop_lamports_local_fails_without_validator() {
    let network = Network::LocalTestnet;
    let client = RpcClient::new(network.rpc_url());
    let keypair = Keypair::new();
    let lamports = 1_000_000;

    let result = airdrop_lamports(network, &client, &keypair, lamports, 3);
    assert!(
        result.is_err(),
        "Local airdrop should fail without validator: {:?}",
        result
    );
}

#[test]
fn test_airdrop_lamports_devnet_fails() {
    let network = Network::Devnet;
    let client = RpcClient::new(network.rpc_url());
    let keypair = Keypair::new();
    let lamports = 1_000_000;

    let result = airdrop_lamports(network, &client, &keypair, lamports, 3);
    assert!(
        matches!(result, Err(AirdropError::FaucetError(_))),
        "Devnet does not support faucet airdrop: {:?}",
        result
    );
}
