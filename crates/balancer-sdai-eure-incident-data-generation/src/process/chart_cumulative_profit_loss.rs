use crate::helper::{DivUp, MulUp};
use crate::process::SwapWithDaiAndSpotCsv;
use alloy::primitives::{I256, U256};
use alloy::sol_types::private::u256;
use eyre::{Context, OptionExt, Result};
use log::info;
use std::ops::Div;
use std::str::FromStr;

const CHART_CUMULATIVE_PROFIT_LOSS_FILE: &str = "data/chart-cumulative-profit-loss.csv";
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct CumulativeProfitLossChartData {
    pub date: String,
    pub block_number: String,
    pub cumulative_profit_loss: String,
    pub profit_loss: String,
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
            .to_rfc3339();

        let profit_loss = match swap.is_buy_eure {
            true => {
                let dai_swap = U256::from_str(&swap.sdai_amount)?
                    .mul_up(U256::from_str(&swap.last_sdai_price)?)?;
                let eure_from_cache = dai_swap.div_up(U256::from_str(&swap.eure_price_new)?)?;
                let eure_from_sma =
                    dai_swap.div_up(U256::from_str(&swap.last_sma_eur_usdt_price)?)?;

                I256::from(eure_from_sma)
                    .checked_sub(I256::from(eure_from_cache))
                    .ok_or_eyre("Profit loss overflow.")?
            }
            false => {
                let eure_swap = U256::from_str(&swap.eure_amount)?;
                let dai_from_cache = eure_swap.mul_up(U256::from_str(&swap.eure_price_new)?)?;
                let dai_from_sma =
                    eure_swap.mul_up(U256::from_str(&swap.last_sma_eur_usdt_price)?)?;
                I256::from(dai_from_sma)
                    .checked_sub(I256::from(dai_from_cache))
                    .ok_or_eyre("Profit loss overflow.")?
            }
        };
        cumulative_profit_loss = cumulative_profit_loss
            .checked_add(profit_loss)
            .ok_or_eyre("Cumulative profit loss overflow.")?;

        csv_writer.serialize(CumulativeProfitLossChartData {
            date,
            block_number: swap.block_number.to_string(),
            cumulative_profit_loss: i256_to_float_str_6_decimals(&cumulative_profit_loss),
            profit_loss: i256_to_float_str_6_decimals(&profit_loss),
        })?
    }

    info!("Generating csv for cumulative P&L chart done.");
    Ok(())
}
fn i256_to_float_str_6_decimals(val_i256: &I256) -> String {
    let mut val_u256 = val_i256.unsigned_abs().to_string();
    let zero_to_add = 19usize.saturating_sub(val_u256.len());
    val_u256 = format!("{}{val_u256}", "0".repeat(zero_to_add));
    let (int_str, dec_str) = val_u256.split_at(val_u256.len().saturating_sub(18));
    let sign_to_add = if val_i256.is_negative() { "-" } else { "" };
    format!("{sign_to_add}{int_str}.{}", dec_str.split_at(6).0)
}
