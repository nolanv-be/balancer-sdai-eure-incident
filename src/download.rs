mod block_timestamp;
mod sdai_price;
mod swap;

use crate::download::block_timestamp::BlockTimestampFetcher;
use alloy::primitives::BlockNumber;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
};
use alloy::providers::{Identity, ProviderBuilder, RootProvider};
use alloy::rpc::client::RpcClient;
use alloy::transports::layers::RetryBackoffLayer;
use eyre::Result;

const MAX_RETRY: u32 = 10;
const BACKOFF: u64 = 1000;
const CUPS: u64 = 10_000;
const STEP: usize = 5_000;

pub type ProviderFiller = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    RootProvider,
>;

pub async fn start(
    rpc_url: &str,
    start_block_download: BlockNumber,
    is_download_swap: bool,
    is_download_sdai: bool,
) -> Result<()> {
    let client = RpcClient::builder()
        .layer(RetryBackoffLayer::new(MAX_RETRY, BACKOFF, CUPS))
        .http(rpc_url.parse()?);
    let provider = ProviderBuilder::new().connect_client(client);

    if is_download_swap {
        swap::start(
            provider.clone(),
            BlockTimestampFetcher::try_new(provider.clone())?,
            start_block_download,
        )
        .await?;
    }

    if is_download_sdai {
        sdai_price::start(
            provider.clone(),
            BlockTimestampFetcher::try_new(provider.clone())?,
            start_block_download,
        )
        .await?;
    }

    Ok(())
}
