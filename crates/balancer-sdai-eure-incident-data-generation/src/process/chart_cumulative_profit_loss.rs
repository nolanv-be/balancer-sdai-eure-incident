use crate::helper::{DivUp, MulUp};
use crate::process::{
    SwapWithDaiAndSpotCsv, compute_divergence_from_swap, i256_to_float_str_6_decimals,
};
use alloy::primitives::{I256, U256};
use eyre::{Context, OptionExt, Result};
use log::info;
use std::str::FromStr;

const CHART_CUMULATIVE_PROFIT_LOSS_FILE: &str = "data/chart-cumulative-profit-loss.csv";
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct CumulativeProfitLossChartData {
    pub date: String,
    pub cumulative_profit_loss: String,
    pub cumulative_profit_loss_raw: String,
}
impl CumulativeProfitLossChartData {
    pub fn load() -> Result<Vec<Self>> {
        let mut csv_reader = csv::Reader::from_path(CHART_CUMULATIVE_PROFIT_LOSS_FILE)
            .wrap_err("Error reading cumulative profit loss csv file")?;

        let mut losses: Vec<CumulativeProfitLossChartData> = csv_reader
            .deserialize::<CumulativeProfitLossChartData>()
            .collect::<Result<Vec<_>, _>>()?;

        losses.sort_by(|a, b| a.date.cmp(&b.date));

        info!(
            "Reading cumulative profit loss file done.({})",
            losses.len()
        );

        Ok(losses)
    }
}
pub fn generate_chart_cumulative_profit_loss_csv() -> Result<()> {
    info!("Generating csv for cumulative P&L chart...");
    let mut csv_writer = csv::Writer::from_path(CHART_CUMULATIVE_PROFIT_LOSS_FILE)?;
    let swap_with_dai_and_spot_vec = SwapWithDaiAndSpotCsv::load()?;
    let mut cumulative_profit_loss = I256::ZERO;
    let mut cumulative_profit_loss_raw = I256::ZERO;

    for (pos_swap, swap) in swap_with_dai_and_spot_vec.iter().enumerate() {
        if pos_swap % 10_000 == 0 {
            info!(
                "Processing swap [{}/{}]",
                pos_swap,
                swap_with_dai_and_spot_vec.len()
            );
            csv_writer.flush()?;
        }

        let date = chrono::DateTime::<chrono::Utc>::from_timestamp(swap.block_timestamp as i64, 0)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            .to_string();

        let (profit_loss, _) = compute_divergence_from_swap(swap)?;
        cumulative_profit_loss = cumulative_profit_loss
            .checked_add(profit_loss)
            .ok_or_eyre("Cumulative profit loss overflow.")?;

        let last_sdai_price = U256::from_str(&swap.last_sdai_price)?;
        let dai_swap = U256::from_str(&swap.sdai_amount)?.mul_up(last_sdai_price)?;
        let eure_swap = U256::from_str(&swap.eure_amount)?;
        let last_sma_eur_usdt_price = U256::from_str(&swap.last_sma_eur_usdt_price)?;

        let profit_loss_raw = if swap.is_buy_eure {
            I256::from(dai_swap.div_up(last_sma_eur_usdt_price)?)
                .checked_sub(I256::from(eure_swap))
                .ok_or_eyre("Divergence (DAI / last EUR/USD) - EURe swap overflow.")?
                .mul_up(I256::from(last_sma_eur_usdt_price))?
        } else {
            I256::from(eure_swap.mul_up(last_sma_eur_usdt_price)?)
                .checked_sub(I256::from(dai_swap))
                .ok_or_eyre("Divergence (EURe * last EUR/USD) - DAI swap overflow.")?
        };
        cumulative_profit_loss_raw = cumulative_profit_loss_raw
            .checked_add(profit_loss_raw)
            .ok_or_eyre("Cumulative raw profit loss overflow.")?;

        csv_writer.serialize(CumulativeProfitLossChartData {
            date,
            cumulative_profit_loss: i256_to_float_str_6_decimals(&cumulative_profit_loss),
            cumulative_profit_loss_raw: i256_to_float_str_6_decimals(&cumulative_profit_loss_raw),
        })?
    }

    info!("Generating csv for cumulative P&L chart done.");
    Ok(())
}
