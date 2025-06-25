use log::{debug, error};
use reqwest::Url;
use solana_client::{client_error::ClientError, rpc_client::RpcClient};
use solana_faucet::faucet::{FAUCET_PORT, request_airdrop_transaction};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{net::SocketAddr, time::Duration};
use thiserror::Error;
use url::ParseError;

#[derive(Error, Debug)]
pub enum AirdropError {
    #[error("RPC request failed: {0}")]
    RpcError(#[from] ClientError),
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("Faucet error: {0}")]
    FaucetError(String),
    #[error("Insufficient balance: current {current}, needed {needed}")]
    InsufficientBalance { current: u64, needed: u64 },
    #[error("Max retries exceeded: {max_retries}")]
    MaxRetriesExceeded { max_retries: usize },
}

impl From<ParseError> for AirdropError {
    fn from(e: ParseError) -> Self {
        AirdropError::FaucetError(format!("Failed to parse faucet URL: {}", e))
    }
}

/// Supported blockchain networks
#[derive(Debug, Clone, Copy)]
pub enum Network {
    LocalTestnet,
    Devnet,
}

impl Network {
    /// Returns the RPC URL for the network
    pub fn rpc_url(&self) -> &'static str {
        match self {
            Self::LocalTestnet => "http://localhost:8899",
            Self::Devnet => "https://api.devnet.solana.com",
        }
    }

    /// Returns the faucet URL for LocalTestnet, None for Devnet
    fn faucet_url(&self) -> Option<Url> {
        match self {
            Self::LocalTestnet => Some(Url::parse("http://localhost:8899").unwrap()),
            Self::Devnet => None,
        }
    }

    /// Returns the faucet address for LocalTestnet, None for Devnet
    pub fn faucet_addr(&self) -> Option<SocketAddr> {
        match self {
            Self::LocalTestnet => Some(SocketAddr::new(
                std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
                FAUCET_PORT,
            )),
            Self::Devnet => None,
        }
    }
}

/// Asynchronously airdrops lamports to the specified address
pub async fn airdrop(
    network: Network,
    recipient: &Pubkey,
    lamports: u64,
    max_retries: usize,
) -> Result<(), AirdropError> {
    let client = RpcClient::new(network.rpc_url());

    // Check current balance
    let current_balance = client.get_balance(recipient)?;
    debug!(
        "Network: {:?}, Recipient: {}, Current balance: {}, Requested: {}",
        network, recipient, current_balance, lamports
    );
    if current_balance >= lamports {
        debug!(
            "Sufficient balance, skipping airdrop for recipient: {}",
            recipient
        );
        return Ok(());
    }

    // Handle airdrop based on network
    match network {
        Network::Devnet => {
            debug!("Attempting RPC airdrop for recipient: {}", recipient);
            request_airdrop_rpc(&client, recipient, lamports, max_retries).await
        }
        Network::LocalTestnet => match network.faucet_url() {
            Some(faucet_url) => {
                debug!(
                    "Attempting faucet airdrop via: {}, recipient: {}",
                    faucet_url, recipient
                );
                request_airdrop_http(faucet_url, recipient, lamports, max_retries).await
            }
            None => {
                debug!("Faucet not supported for network: {:?}", network);
                Err(AirdropError::InsufficientBalance {
                    current: current_balance,
                    needed: lamports,
                })
            }
        },
    }
}

/// Synchronously airdrops lamports to a keypair using the network's faucet
pub fn airdrop_lamports(
    network: Network,
    client: &RpcClient,
    id: &Keypair,
    desired_balance: u64,
    max_retries: usize,
) -> Result<(), AirdropError> {
    let recipient = id.pubkey();
    let starting_balance = client.get_balance(&recipient)?;
    debug!(
        "Initial balance: {} for keypair: {}",
        starting_balance, recipient
    );

    if starting_balance >= desired_balance {
        debug!("Sufficient balance for keypair: {}", recipient);
        return Ok(());
    }

    let airdrop_amount = desired_balance - starting_balance;
    let faucet_addr = network.faucet_addr().ok_or_else(|| {
        AirdropError::FaucetError(format!("Faucet not supported for network: {:?}", network))
    })?;
    debug!(
        "Requesting {} lamports from faucet: {} for keypair: {}",
        airdrop_amount, faucet_addr, recipient
    );

    for attempt in 0..max_retries {
        debug!(
            "Airdrop attempt {}/{} for keypair: {}",
            attempt + 1,
            max_retries,
            recipient
        );
        let blockhash = client.get_latest_blockhash()?;
        match request_airdrop_transaction(&faucet_addr, &recipient, airdrop_amount, blockhash) {
            Ok(transaction) => {
                if client.send_and_confirm_transaction(&transaction).is_ok() {
                    debug!("Airdrop successful for keypair: {}", recipient);
                    let current_balance = client.get_balance(&recipient)?;
                    if current_balance >= desired_balance {
                        return Ok(());
                    } else {
                        error!(
                            "Airdrop failed: expected at least {}, got {}",
                            desired_balance, current_balance
                        );
                        return Err(AirdropError::FaucetError(format!(
                            "Insufficient airdrop amount: got {}",
                            current_balance
                        )));
                    }
                }
            }
            Err(e) => {
                error!("Airdrop transaction failed: {}", e);
                if attempt == max_retries - 1 {
                    return Err(AirdropError::FaucetError(format!(
                        "Failed to request airdrop: {}",
                        e
                    )));
                }
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    debug!("Max retries exceeded for keypair: {}", recipient);
    Err(AirdropError::MaxRetriesExceeded { max_retries })
}

async fn request_airdrop_rpc(
    client: &RpcClient,
    recipient: &Pubkey,
    lamports: u64,
    max_retries: usize,
) -> Result<(), AirdropError> {
    for attempt in 0..max_retries {
        debug!(
            "RPC airdrop attempt {}/{} for recipient: {}",
            attempt + 1,
            max_retries,
            recipient
        );
        match client.request_airdrop(recipient, lamports) {
            Ok(_) => {
                debug!("RPC airdrop successful for recipient: {}", recipient);
                return Ok(());
            }
            Err(e) => {
                error!("RPC airdrop failed: {}", e);
                if attempt == max_retries - 1 {
                    return Err(AirdropError::RpcError(e));
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    debug!("Max retries exceeded for recipient: {}", recipient);
    Err(AirdropError::MaxRetriesExceeded { max_retries })
}

async fn request_airdrop_http(
    faucet_url: Url,
    recipient: &Pubkey,
    lamports: u64,
    max_retries: usize,
) -> Result<(), AirdropError> {
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    for attempt in 0..max_retries {
        debug!(
            "HTTP airdrop attempt {}/{} for recipient: {}",
            attempt + 1,
            max_retries,
            recipient
        );
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "requestAirdrop",
            "params": [recipient.to_string(), lamports],
        });

        debug!("Sending request to faucet: {}", faucet_url);

        let response = http_client
            .post(faucet_url.clone())
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();
        debug!("Faucet response: status={}, body={}", status, response_text);

        if !status.is_success() {
            error!(
                "Faucet request failed, status {}: {}",
                status, response_text
            );
            return Err(AirdropError::FaucetError(format!(
                "Faucet returned status {}: {}",
                status, response_text
            )));
        }

        match serde_json::from_str::<serde_json::Value>(&response_text) {
            Ok(json) => {
                if json.get("error").is_none() {
                    debug!("HTTP airdrop successful for recipient: {}", recipient);
                    return Ok(());
                } else {
                    error!("Faucet returned error: {}", json);
                    return Err(AirdropError::FaucetError(json.to_string()));
                }
            }
            Err(e) => {
                error!("Failed to parse faucet response as JSON: {}", e);
                return Err(AirdropError::FaucetError(format!(
                    "Invalid JSON response: {}",
                    response_text
                )));
            }
        }
    }

    debug!("Max retries exceeded for recipient: {}", recipient);
    Err(AirdropError::MaxRetriesExceeded { max_retries })
}
