mod block_timestamp;
mod swap;

use crate::download::block_timestamp::{BlockTimestampFetcher, TryIntoBlockTimestamp};
use crate::download::swap::SwapFetcher;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
};
use alloy::providers::{Identity, Provider, ProviderBuilder, RootProvider};
use alloy::rpc::client::RpcClient;
use alloy::transports::layers::RetryBackoffLayer;
use eyre::Result;
use log::info;

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
pub async fn start(rpc_url: &str) -> Result<()> {
    info!("Downloading data from rpc...");

    let client = RpcClient::builder()
        .layer(RetryBackoffLayer::new(MAX_RETRY, BACKOFF, CUPS))
        .http(rpc_url.parse()?);
    let provider = ProviderBuilder::new().connect_client(client);

    let block_timestamp_fetcher = BlockTimestampFetcher::try_new(provider.clone())?;
    let mut swap_fetcher = SwapFetcher::try_new(provider.clone(), block_timestamp_fetcher)?;

    let latest_block = provider.get_block_number().await?;

    for current_block in (SDAI_EURE_POOL_CREATION_BLOCK..=latest_block).step_by(STEP) {
        let current_block_timestamp = current_block
            .try_into_block_timestamp(&mut swap_fetcher.block_timestamp_fetcher)
            .await?;
        info!(
            "Downloading block {}/{} ({})",
            current_block,
            latest_block,
            chrono::DateTime::<chrono::Utc>::from_timestamp(current_block_timestamp as i64, 0)
                .unwrap()
                .to_rfc3339()
        );

        swap_fetcher
            .fetch_swap_csv(
                current_block,
                current_block.saturating_add(STEP.saturating_sub(1) as u64),
            )
            .await?;

        swap_fetcher.flush()?
    }

    info!("Downloading data from rpc done.");
    Ok(())
}
