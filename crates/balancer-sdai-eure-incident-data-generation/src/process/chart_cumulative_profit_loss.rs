use crate::process::{
    SwapWithDaiAndSpotCsv, compute_divergence_from_swap, i256_to_basis_point,
    i256_to_float_str_6_decimals,
};
use alloy::primitives::I256;
use eyre::{Context, OptionExt, Result};
use log::info;

const CHART_CUMULATIVE_PROFIT_LOSS_FILE: &str = "data/chart-cumulative-profit-loss.csv";
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct CumulativeProfitLossChartData {
    pub date: String,
    pub block_number: String,
    pub cumulative_profit_loss: String,
    pub profit_loss: String,
    pub divergence_bp: String,
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

        let (profit_loss, divergence_bp) = compute_divergence_from_swap(swap)?;
        cumulative_profit_loss = cumulative_profit_loss
            .checked_add(profit_loss)
            .ok_or_eyre("Cumulative profit loss overflow.")?;

        csv_writer.serialize(CumulativeProfitLossChartData {
            date,
            block_number: swap.block_number.to_string(),
            cumulative_profit_loss: i256_to_float_str_6_decimals(&cumulative_profit_loss),
            profit_loss: i256_to_float_str_6_decimals(&profit_loss),
            divergence_bp: i256_to_basis_point(&divergence_bp),
        })?
    }

    info!("Generating csv for cumulative P&L chart done.");
    Ok(())
}
