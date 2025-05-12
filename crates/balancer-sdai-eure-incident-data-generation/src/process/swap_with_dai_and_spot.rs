use crate::download::{SdaiCsv, SwapCsv};
use crate::process::SmaEurUsdtCsv;
use eyre::{OptionExt, Result};
use log::info;

const SWAP_WITH_DAI_AND_SPOT_FILE: &str = "data/swap-with-dai-and-spot.csv";

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct SwapWithDaiAndSpotCsv {
    pub is_buy_eure: bool,
    pub sdai_amount: String,
    pub eure_amount: String,
    pub block_number: u64,
    pub block_timestamp: u64,
    pub tx_hash: String,
    pub trace_path: String,
    pub sdai_last_update: u64,
    pub eure_last_update: u64,
    pub sdai_duration: u64,
    pub eure_duration: u64,
    pub sdai_price_old: String,
    pub eure_price_old: String,
    pub sdai_price_new: String,
    pub eure_price_new: String,
    pub swap_fee_percentage: String,
    pub last_sdai_price: String,
    pub last_sma_eur_usdt_price: String,
}

pub fn generate_swap_with_dai_and_spot_csv() -> Result<()> {
    info!("Generating csv of swap with dai and spot EUR/USDT...");

    let mut csv_writer = csv::Writer::from_path(SWAP_WITH_DAI_AND_SPOT_FILE)?;
    let swaps = SwapCsv::load()?;
    let sdai_prices = SdaiCsv::load()?;
    let mut last_pos_sdai = 0;
    let sma_eur_usdt_vec = SmaEurUsdtCsv::load()?;
    let mut last_pos_sma_eur_usdt = 0;

    for (pos_swap, swap) in swaps.iter().enumerate() {
        if pos_swap % 10_000 == 0 {
            info!("Processing swap [{}/{}]", pos_swap, swaps.len());
            csv_writer.flush()?;
        }

        let last_sdai_price;
        {
            let mut last_sdai_price_maybe = None;

            for (pos_sdai, sdai) in sdai_prices.iter().enumerate().skip(last_pos_sdai) {
                let next_sdai = sdai_prices
                    .get(pos_sdai + 1)
                    .ok_or_eyre("Missing some sDAI price")?;
                if next_sdai.block_timestamp > swap.block_timestamp {
                    last_sdai_price_maybe = Some(sdai.price.clone());
                    last_pos_sdai = pos_sdai;
                    break;
                }
            }

            last_sdai_price = last_sdai_price_maybe.ok_or_eyre("Missing sDAI price")?;
        }

        let last_sma_eur_usdt_price;
        {
            let mut last_sma_eur_usdt_price_maybe = None;

            for (pos_eur_usdt, eur_usdt) in sma_eur_usdt_vec
                .iter()
                .enumerate()
                .skip(last_pos_sma_eur_usdt)
            {
                let next_eur_usdt = sma_eur_usdt_vec
                    .get(pos_eur_usdt + 1)
                    .ok_or_eyre("Missing some sma EUR/USDT")?;
                if next_eur_usdt.timestamp > swap.block_timestamp {
                    last_sma_eur_usdt_price_maybe = Some(eur_usdt.sma_price.clone());
                    last_pos_sma_eur_usdt = pos_eur_usdt;
                    break;
                }
            }

            last_sma_eur_usdt_price =
                last_sma_eur_usdt_price_maybe.ok_or_eyre("Missing sma price")?;
        }

        let swap = swap.clone();
        csv_writer.serialize(SwapWithDaiAndSpotCsv {
            is_buy_eure: swap.is_buy_eure,
            sdai_amount: swap.sdai_amount,
            eure_amount: swap.eure_amount,
            block_number: swap.block_number,
            block_timestamp: swap.block_timestamp,
            tx_hash: swap.tx_hash,
            trace_path: swap.trace_path,
            sdai_last_update: swap.sdai_last_update,
            eure_last_update: swap.eure_last_update,
            sdai_duration: swap.sdai_duration,
            eure_duration: swap.eure_duration,
            sdai_price_old: swap.sdai_price_old,
            eure_price_old: swap.eure_price_old,
            sdai_price_new: swap.sdai_price_new,
            eure_price_new: swap.eure_price_new,
            swap_fee_percentage: swap.swap_fee_percentage,
            last_sdai_price,
            last_sma_eur_usdt_price,
        })?
    }

    info!("Generating csv of swap with dai and spot EUR/USDT done.");
    Ok(())
}
