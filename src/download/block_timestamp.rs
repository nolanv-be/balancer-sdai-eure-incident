use super::ProviderFiller;
use alloy::primitives::BlockTimestamp;
use alloy::providers::Provider;
use eyre::{Context, ContextCompat, Result};
use log::info;
use std::collections::HashMap;
use std::fs::OpenOptions;

const BLOCKS_CSV_FILE: &str = "blocks.csv";
pub struct BlockTimestampFetcher {
    provider: ProviderFiller,
    csv_writer: csv::Writer<std::fs::File>,
    block_timestamp_by_number: HashMap<BlockNumber, Timestamp>,
    block_number_by_timestamp: HashMap<Timestamp, BlockNumber>,
}
type Timestamp = u64;
type BlockNumber = u64;
impl BlockTimestampFetcher {
    pub fn try_new(provider: ProviderFiller) -> Result<Self> {
        let Ok(mut csv_reader) = csv::Reader::from_path(BLOCKS_CSV_FILE) else {
            let csv_writer = csv::Writer::from_path(BLOCKS_CSV_FILE)?;

            info!("No blocks timestamp file found");
            return Ok(Self {
                csv_writer,
                provider,
                block_timestamp_by_number: HashMap::new(),
                block_number_by_timestamp: HashMap::new(),
            });
        };
        info!("Reading blocks timestamp file...");

        let mut block_timestamp_by_number = HashMap::new();
        let mut block_number_by_timestamp = HashMap::new();
        for block in csv_reader.deserialize::<BlockWithTimestamp>() {
            let block = block?;
            block_timestamp_by_number.insert(block.number, block.timestamp);
            block_number_by_timestamp.insert(block.timestamp, block.number);
        }
        info!(
            "Reading blocks timestamp file done.({})",
            block_timestamp_by_number.len()
        );

        let csv_writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(OpenOptions::new().append(true).open(BLOCKS_CSV_FILE)?);

        Ok(Self {
            csv_writer,
            provider,
            block_timestamp_by_number,
            block_number_by_timestamp,
        })
    }
    pub async fn fetch_timestamp(&mut self, block_number: u64) -> Result<Timestamp> {
        if let Some(timestamp) = self.block_timestamp_by_number.get(&block_number) {
            return Ok(*timestamp);
        }

        let block_timestamp: Timestamp = self
            .provider
            .get_block_by_number(block_number.into())
            .await?
            .wrap_err("Block number not found")?
            .header
            .timestamp;
        self.block_timestamp_by_number
            .insert(block_number, block_timestamp);
        self.block_number_by_timestamp
            .insert(block_timestamp, block_number);

        self.csv_writer.serialize(BlockWithTimestamp {
            number: block_number,
            timestamp: block_timestamp,
        })?;

        Ok(block_timestamp)
    }

    pub async fn flush(&mut self) -> Result<()> {
        self.csv_writer.flush()?;
        Ok(())
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct BlockWithTimestamp {
    timestamp: u64,
    number: u64,
}

pub trait TryIntoBlockTimestamp
where
    Self: From<u64>,
{
    async fn try_into_block_timestamp(
        &self,
        block_timestamp_fetcher: &mut BlockTimestampFetcher,
    ) -> Result<BlockTimestamp>;
}
impl TryIntoBlockTimestamp for BlockNumber {
    async fn try_into_block_timestamp(
        &self,
        block_timestamp_fetcher: &mut BlockTimestampFetcher,
    ) -> Result<BlockTimestamp> {
        block_timestamp_fetcher
            .fetch_timestamp(*self)
            .await
            .wrap_err(format!("Failed to fetch block timestamp {:?}", self))
    }
}
