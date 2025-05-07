mod on_exit_pool;
mod on_join_pool;
mod on_swap;

use crate::download::{ProviderFiller, block_timestamp::BlockTimestampFetcher};
use crate::helper::{
    DivUp, MulUp, Position, StateBySubPath, StringifyArrayUsize, extract_sub_vm_trace,
    fetch_sub_vm_trace, save_trace_to_file,
};
use alloy::providers::Provider;
use alloy::{
    primitives::{Address, B256, BlockNumber, U256, address, b256},
    providers::ext::TraceApi,
    rpc::types::trace::filter::TraceFilter,
    rpc::types::trace::parity::LocalizedTransactionTrace,
};
use eyre::{Context, OptionExt, Result, bail};
use log::{debug, info};
use std::collections::HashMap;
use std::fs::OpenOptions;

const BALANCER_SDAI_EURE_POOL_ADDRESS: Address =
    address!("dd439304a77f54b1f7854751ac1169b279591ef7");
const SDAI_ADDRESS: Address = address!("af204776c7245bF4147c2612BF6e5972Ee483701");
const SDAI_ARRAY_INDEX: usize = 0;
const EURE_ADDRESS: Address = address!("cB444e90D8198415266c6a2724b7900fb12FC56E");
const EURE_ARRAY_INDEX: usize = 1;
const SWAPS_CSV_FILE: &str = "data/swaps.csv";

pub struct SwapFetcher {
    pub csv_writer: csv::Writer<std::fs::File>,
    pub provider: ProviderFiller,
    pub block_timestamp_fetcher: BlockTimestampFetcher,
    pub swap_csv_by_tx_hash_trace_path: HashMap<(String, String), SwapCsv>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct SwapCsv {
    pub is_buy_eure: bool,
    pub sdai_amount: String,
    pub eure_amount: String,
    pub block_number: u64,
    pub block_timestamp: u64,
    pub tx_hash: String,
    pub trace_path: String,
}

impl SwapFetcher {
    pub fn try_new(
        provider: ProviderFiller,
        block_timestamp_fetcher: BlockTimestampFetcher,
    ) -> Result<Self> {
        let Ok(mut csv_reader) = csv::Reader::from_path(SWAPS_CSV_FILE) else {
            let csv_writer = csv::Writer::from_path(SWAPS_CSV_FILE)?;

            info!("No swap file found");
            return Ok(Self {
                csv_writer,
                provider,
                block_timestamp_fetcher,
                swap_csv_by_tx_hash_trace_path: HashMap::new(),
            });
        };
        info!("Reading swap file...");

        let mut swap_csv_by_tx_hash_trace_path = HashMap::new();
        for swap_result in csv_reader.deserialize::<SwapCsv>() {
            let swap = swap_result?;
            swap_csv_by_tx_hash_trace_path
                .insert((swap.tx_hash.clone(), swap.trace_path.clone()), swap);
        }
        info!(
            "Reading swap file done.({})",
            swap_csv_by_tx_hash_trace_path.len()
        );

        let csv_writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(OpenOptions::new().append(true).open(SWAPS_CSV_FILE)?);

        Ok(Self {
            csv_writer,
            provider,
            block_timestamp_fetcher,
            swap_csv_by_tx_hash_trace_path,
        })
    }

    pub async fn fetch_swap_csv(
        &mut self,
        from_block: BlockNumber,
        to_block: BlockNumber,
    ) -> Result<Vec<SwapCsv>> {
        let localized_traces = self
            .provider
            .trace_filter(
                &TraceFilter::default()
                    .to_address(vec![BALANCER_SDAI_EURE_POOL_ADDRESS])
                    .from_block(from_block)
                    .to_block(to_block),
            )
            .await?;

        let mut swap_csv_vec = Vec::new();

        for localized_trace in localized_traces {
            if localized_trace.trace.error.is_some() {
                continue;
            }
            let tx_hash = localized_trace.transaction_hash.ok_or_eyre("no tx_hash")?;
            let trace_path = localized_trace.trace.trace_address.stringify_vec_usize();

            if self
                .swap_csv_by_tx_hash_trace_path
                .contains_key(&(tx_hash.to_string(), trace_path.to_string()))
            {
                debug!("Skip tx already fetched");
                continue;
            }

            if !self
                .provider
                .get_transaction_receipt(tx_hash)
                .await?
                .ok_or_eyre("Failed to get receipt by hash {tx_hash}")?
                .status()
            {
                debug!("Skip tx due to status");
                continue;
            }

            let Some(call_action) = localized_trace.trace.action.as_call() else {
                continue;
            };
            let block_number = localized_trace
                .block_number
                .ok_or_eyre("Block number is missing")?;

            match self
                .process_on_swap_trace(
                    &localized_trace,
                    &tx_hash,
                    &trace_path,
                    block_number,
                    call_action,
                )
                .await
            {
                Ok(Some(swap_csv)) => {
                    debug!("onSwap() => {:?}", swap_csv);
                    self.insert_swap_csv(swap_csv.clone())?;
                    swap_csv_vec.push(swap_csv);
                }
                Err(e) => {
                    self.log_processing_failed(&localized_trace, &tx_hash).await;
                    bail!("Failed to process onSwap trace\n{:?}", e);
                }
                Ok(None) => {}
            }

            match self
                .process_on_join_pool_trace(
                    &localized_trace,
                    &tx_hash,
                    &trace_path,
                    block_number,
                    call_action,
                )
                .await
            {
                Ok(Some(swap_csv)) => {
                    debug!("onJoinPool() => {:?}", swap_csv);
                    self.insert_swap_csv(swap_csv.clone())?;
                    swap_csv_vec.push(swap_csv);
                }
                Err(e) => {
                    self.log_processing_failed(&localized_trace, &tx_hash).await;
                    bail!("Failed to process onJoinPool trace\n{:?}", e);
                }
                Ok(None) => {}
            }

            match self
                .process_on_exit_pool_trace(
                    &localized_trace,
                    &tx_hash,
                    &trace_path,
                    block_number,
                    call_action,
                )
                .await
            {
                Ok(Some(swap_csv)) => {
                    debug!("onExitPool() => {:?}", swap_csv);
                    self.insert_swap_csv(swap_csv.clone())?;
                    swap_csv_vec.push(swap_csv);
                }
                Err(e) => {
                    self.log_processing_failed(&localized_trace, &tx_hash).await;
                    bail!("Failed to process onExitPool trace\n{:?}", e);
                }
                Ok(None) => {}
            }
        }

        Ok(swap_csv_vec)
    }
    async fn log_processing_failed(
        &self,
        localized_trace: &LocalizedTransactionTrace,
        tx_hash: &B256,
    ) {
        let vm_trace = fetch_sub_vm_trace(&self.provider, *tx_hash, &[])
            .await
            .expect("Failed to fetch sub vm trace");
        save_trace_to_file(vm_trace.clone(), tx_hash, "full")
            .expect("Failed to save trace to file");

        let (trace_address, _) = localized_trace
            .trace
            .trace_address
            .split_at(localized_trace.trace.trace_address.len() - 1);
        let sub_vm_trace = extract_sub_vm_trace(vm_trace.clone(), trace_address)
            .expect("Failed to extract sub vm trace");
        save_trace_to_file(sub_vm_trace.clone(), tx_hash, "sub")
            .expect("Failed to save trace to file");

        let state_by_sub_path = StateBySubPath::new(&vm_trace);
        debug!("{:#?}", &state_by_sub_path);
    }

    fn insert_swap_csv(&mut self, swap_csv: SwapCsv) -> Result<()> {
        self.swap_csv_by_tx_hash_trace_path.insert(
            (swap_csv.tx_hash.clone(), swap_csv.trace_path.clone()),
            swap_csv.clone(),
        );

        self.csv_writer.serialize(&swap_csv)?;

        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.csv_writer.flush()?;
        self.block_timestamp_fetcher.flush()
    }
}

fn compute_bpt_ratio(
    state_by_sub_path: &StateBySubPath,
    bpt_in_out: U256,
    is_store: bool,
    bpt_balance_pool_trace_address: &[usize],
    bpt_balance_pool_position: Position,
    bpt_total_supply_trace_address: &[usize],
    bpt_total_supply_position: Position,
) -> Result<U256> {
    const BPT_BALANCE_POOL_STORAGE_KEY: B256 =
        b256!("7ece16e0df962b5f0d12e93168ea433e7ad6d26c1059a153571c768eab6a5271");
    const BPT_TOTAL_SUPPLY_STORAGE_KEY: B256 =
        b256!("0000000000000000000000000000000000000000000000000000000000000002");

    let get_storage = match is_store {
        true => StateBySubPath::get_store_value,
        false => StateBySubPath::get_load_value,
    };

    let bpt_balance_pool = U256::from_be_slice(
        get_storage(
            state_by_sub_path,
            &BPT_BALANCE_POOL_STORAGE_KEY,
            bpt_balance_pool_trace_address,
            &bpt_balance_pool_position,
        )
        .ok_or_eyre(format!(
            "Failed to get bpt_balance_pool for trace_address {:?} in this position {:?}",
            bpt_balance_pool_trace_address, bpt_balance_pool_position
        ))?
        .split_at(16)
        .1,
    );
    let bpt_total_supply = U256::from_be_slice(
        get_storage(
            state_by_sub_path,
            &BPT_TOTAL_SUPPLY_STORAGE_KEY,
            bpt_total_supply_trace_address,
            &bpt_total_supply_position,
        )
        .ok_or_eyre(format!(
            "Failed to get bpt_total_supply for trace_address {:?} in this position {:?}",
            bpt_total_supply_trace_address, bpt_total_supply_position
        ))?
        .split_at(16)
        .1,
    );

    let bpt_virtual_supply = bpt_total_supply
        .checked_sub(bpt_balance_pool)
        .ok_or_eyre("bpt_balance_pool is bigger than bpt_total_supply")?;

    bpt_in_out
        .div_up(bpt_virtual_supply)
        .wrap_err("Failed to div_up bpt_swap by bpt_virtual_supply")
}
pub fn compute_sdai_eure_from_bpt(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    bpt_mint_burn: U256,
    is_bpt_mint: bool,
    balances: &[U256],
) -> Result<(U256, U256)> {
    let bpt_ratio = compute_bpt_ratio(
        state_by_sub_path,
        bpt_mint_burn,
        is_bpt_mint,
        &[],
        Position::First,
        sub_trace_address,
        Position::Last,
    )
    .wrap_err("Failed to compute bpt ratio")?;

    let sdai_balance_pool = balances
        .get(SDAI_ARRAY_INDEX)
        .ok_or_eyre("sDAI balance of the pool not found")?;
    let eure_balance_pool = balances
        .get(EURE_ARRAY_INDEX)
        .ok_or_eyre("EURe balance of the pool not found")?;

    let bpt_hold_sdai = sdai_balance_pool
        .mul_up(bpt_ratio)
        .wrap_err("Failed to mul_up sdai_balance_pool by bpt_ratio")?;
    let bpt_hold_eure = eure_balance_pool
        .mul_up(bpt_ratio)
        .wrap_err("Failed to mul_up eure_balance_pool by bpt_ratio")?;

    Ok((bpt_hold_sdai, bpt_hold_eure))
}

/*#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::{BACKOFF, CUPS, MAX_RETRY};
    use alloy::providers::ProviderBuilder;
    use alloy::rpc::client::RpcClient;
    use alloy::transports::layers::RetryBackoffLayer;

    #[tokio::test]
    async fn test_process_on_swap_trace() {
        env_logger::init();

        let rpc_url = std::env::var("RPC_URL").unwrap();

        let client = RpcClient::builder()
            .layer(RetryBackoffLayer::new(MAX_RETRY, BACKOFF, CUPS))
            .http(rpc_url.parse().unwrap());
        let provider = ProviderBuilder::new().connect_client(client);

        let mut block_timestamp_fetcher = BlockTimestampFetcher::try_new(provider.clone()).unwrap();

        let result_aura_multiple_swap = fetch_swap_csv(
            &provider,
            &mut block_timestamp_fetcher,
            BlockNumber::from(30649625u64),
            0,
        )
        .await;

        assert!(result_aura_multiple_swap.is_ok());

        let result_staticcall_eoa = fetch_swap_csv(
            &provider,
            &mut block_timestamp_fetcher,
            BlockNumber::from(30629227u64),
            0,
        )
        .await;
        assert!(result_staticcall_eoa.is_ok());

        let result_swap_join_in_out = fetch_swap_csv(
            &provider,
            &mut block_timestamp_fetcher,
            BlockNumber::from(30615088u64),
            0,
        )
        .await;
        assert!(result_swap_join_in_out.is_ok());
    }
}*/
