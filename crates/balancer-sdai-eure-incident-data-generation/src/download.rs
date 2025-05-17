mod sdai_price;
mod swap;

use alloy::primitives::BlockNumber;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
};
use alloy::providers::{Identity, ProviderBuilder, RootProvider};
use alloy::rpc::client::RpcClient;
use eyre::Result;
pub use sdai_price::SdaiCsv;
pub use swap::SwapCsv;

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
    max_concurrent_fetch: usize,
    is_download_swap: bool,
    is_download_sdai: bool,
) -> Result<()> {
    let client = RpcClient::builder().http(rpc_url.parse()?);
    let provider = ProviderBuilder::new().connect_client(client);

    if is_download_swap {
        swap::download_swap(provider.clone(), start_block_download, max_concurrent_fetch).await?;
    }

    if is_download_sdai {
        sdai_price::download_sdai_price(provider.clone(), start_block_download).await?;
    }

    Ok(())
}
