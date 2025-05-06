mod block_timestamp;
mod swap;

use crate::download::block_timestamp::{BlockTimestampFetcher, TryIntoBlockTimestamp};
use crate::download::swap::fetch_swap_csv;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
};
use alloy::providers::{Identity, Provider, ProviderBuilder, RootProvider};
use alloy::rpc::client::RpcClient;
use alloy::transports::layers::RetryBackoffLayer;
use eyre::Result;
use log::info;
use std::fs::OpenOptions;

const SWAPS_CSV_FILE: &str = "swaps.csv";

const MAX_RETRY: u32 = 10;
const BACKOFF: u64 = 1000;
const CUPS: u64 = 10_000;
const SDAI_EURE_POOL_CREATION_BLOCK: u64 = 30_274_134;
const STEP: usize = 100_000;

pub type ProviderFiller = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    RootProvider,
>;

// TODO Add spot price for EUR/USD, maybe add price_rate infos
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct SwapCsv {
    pub is_buy_eure: bool,
    pub sdai_amount: String,
    pub eure_amount: String,
    pub block_number: u64,
    pub block_timestamp: u64,
    pub tx_hash: String,
}
pub async fn start(rpc_url: &str) -> Result<()> {
    info!("Downloading data from rpc...");

    let client = RpcClient::builder()
        .layer(RetryBackoffLayer::new(MAX_RETRY, BACKOFF, CUPS))
        .http(rpc_url.parse()?);
    let provider = ProviderBuilder::new().connect_client(client);
    let mut swap_csv_writer = csv::WriterBuilder::new()
        .from_writer(OpenOptions::new().append(true).open(SWAPS_CSV_FILE)?);

    let mut block_timestamp_fetcher = BlockTimestampFetcher::try_new(provider.clone())?;

    let latest_block = provider.get_block_number().await?;

    for current_block in (SDAI_EURE_POOL_CREATION_BLOCK..=latest_block).step_by(STEP) {
        let current_block_timestamp = current_block
            .try_into_block_timestamp(&mut block_timestamp_fetcher)
            .await?;
        info!(
            "Downloading block {}/{} ({})",
            current_block,
            latest_block,
            chrono::DateTime::<chrono::Utc>::from_timestamp(current_block_timestamp as i64, 0)
                .unwrap()
                .to_rfc3339()
        );

        let swap_csv_vec =
            fetch_swap_csv(&provider, &mut block_timestamp_fetcher, current_block, STEP).await?;

        // TODO implement proper swap_csv with check skip already downloaded
        for swap_csv in swap_csv_vec {
            swap_csv_writer.serialize(swap_csv)?;
        }
        swap_csv_writer.flush()?;
        block_timestamp_fetcher.flush().await?;
    }

    info!("Downloading data from rpc done.");
    Ok(())
}
