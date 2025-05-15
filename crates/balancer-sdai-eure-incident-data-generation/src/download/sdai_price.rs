use crate::download::ProviderFiller;
use crate::download::block_timestamp::{BlockTimestampFetcher, TryIntoBlockTimestamp};
use crate::helper::ONE_18;
use alloy::primitives::{Address, BlockNumber, address};
use alloy::providers::Provider;
use alloy::sol;
use eyre::{Context, Result};
use log::info;
use std::fs::OpenOptions;

const SDAI_PRICE_CSV_FILE: &str = "data/sdai_price.csv";
const SDAI_SAMPLING_INTERVAL: usize = 100;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct SdaiCsv {
    pub block_timestamp: u64,
    pub price: String,
}

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    contract SDAI {
        function convertToAssets(uint256 shares) public view virtual returns (uint256);
    }
);

pub async fn download_sdai_price(
    provider: ProviderFiller,
    mut block_timestamp_fetcher: BlockTimestampFetcher,
    start_block_download: BlockNumber,
) -> Result<()> {
    info!("Downloading sdai price from rpc...");

    const SDAI_ADDRESS: Address = address!("af204776c7245bF4147c2612BF6e5972Ee483701");

    let sdai_contract = SDAI::new(SDAI_ADDRESS, &provider);
    let latest_block = provider.get_block_number().await?;

    let mut csv_writer = match std::fs::exists(SDAI_PRICE_CSV_FILE).unwrap_or(false) {
        false => csv::Writer::from_path(SDAI_PRICE_CSV_FILE)?,
        true => csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(OpenOptions::new().append(true).open(SDAI_PRICE_CSV_FILE)?),
    };

    for current_block in (start_block_download..=latest_block).step_by(SDAI_SAMPLING_INTERVAL) {
        let block_timestamp = current_block
            .try_into_block_timestamp(&mut block_timestamp_fetcher)
            .await?;

        if current_block == start_block_download
            || current_block % 10_000 < SDAI_SAMPLING_INTERVAL as u64
        {
            info!(
                "Downloading sDAI price for block [{}/{}] ({})",
                current_block,
                latest_block,
                chrono::DateTime::<chrono::Utc>::from_timestamp(block_timestamp as i64, 0)
                    .unwrap()
                    .to_rfc3339()
            );
            csv_writer.flush()?;
        }

        let price = sdai_contract
            .convertToAssets(ONE_18)
            .block(current_block.into())
            .call()
            .await?;
        csv_writer.serialize(SdaiCsv {
            block_timestamp,
            price: price.to_string(),
        })?;
    }
    csv_writer.flush()?;

    info!("Downloading sdai price from rpc done.");
    Ok(())
}

impl SdaiCsv {
    pub fn load() -> Result<Vec<Self>> {
        let mut csv_reader = csv::Reader::from_path(SDAI_PRICE_CSV_FILE)
            .wrap_err("Error reading sDAI price csv file")?;

        let mut sdai_prices: Vec<SdaiCsv> = csv_reader
            .deserialize::<SdaiCsv>()
            .collect::<Result<Vec<_>, _>>()?;

        sdai_prices.sort_by(|a, b| a.block_timestamp.cmp(&b.block_timestamp));

        info!("Reading sDAI price file done.({})", sdai_prices.len());

        Ok(sdai_prices)
    }
}
