use solana_airdrop_utils::Network;
use solana_client::rpc_client::RpcClient;
use solana_keypair::Keypair;
use solana_sdk::{signature::Signer, system_instruction, transaction::Transaction};
use std::time::Duration;
use tokio::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let recipient_keypair = Keypair::new();
    let recipient = recipient_keypair.pubkey();
    println!("Airdrop target address: {}", recipient);

    let network = Network::LocalTestnet;
    let client = RpcClient::new(network.rpc_url());
    let lamports = 100_000;

    let initial_balance = client.get_balance(&recipient)?;
    println!("Initial balance: {} lamports", initial_balance);

    if initial_balance < lamports {
        // 使用 CLI 的默认密钥对初始化账户
        let payer_keypair =
            solana_keypair::read_keypair_file("/Users/qiaopengjun/.config/solana/id.json")
                .map_err(|e| anyhow::anyhow!("Failed to read keypair file: {}", e))?;
        let payer = payer_keypair.pubkey();

        // 计算租金
        let rent = client.get_minimum_balance_for_rent_exemption(0)?;
        println!("Minimum rent for account: {} lamports", rent);

        // 创建账户
        let create_account_ix = system_instruction::create_account(
            &payer,
            &recipient,
            rent,
            0,
            &solana_sdk::system_program::id(),
        );

        let recent_blockhash = client.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            &[create_account_ix],
            Some(&payer),
            &[&payer_keypair, &recipient_keypair],
            recent_blockhash,
        );

        client.send_and_confirm_transaction(&transaction)?;
        println!("Account {} created successfully", recipient);

        // 请求空投
        let signature = client.request_airdrop(&recipient, lamports)?;
        println!("Airdrop requested, tx signature: {}", signature);

        // 等待交易确认
        let start = Instant::now();
        loop {
            match client.get_signature_status(&signature)? {
                Some(Ok(_)) => {
                    println!("Transaction confirmed successfully");
                    break;
                }
                Some(Err(e)) => {
                    return Err(anyhow::anyhow!("Transaction failed: {}", e));
                }
                None => {
                    if start.elapsed().as_secs() > 30 {
                        return Err(anyhow::anyhow!("Transaction confirmation timeout"));
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await; // 等待余额更新
    }

    let final_balance = client.get_balance(&recipient)?;
    println!("Final balance: {} lamports", final_balance);

    if final_balance >= lamports {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Airdrop validation failed"))
    }
}
