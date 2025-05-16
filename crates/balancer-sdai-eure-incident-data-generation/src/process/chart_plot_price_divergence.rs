use crate::process::{
    SwapWithDaiAndSpotCsv, compute_divergence_from_swap, u256_str_to_basis_point,
    u256_str_to_float_str_6_decimals,
};
use eyre::{Context, OptionExt, Result};
use log::info;

const CHART_PLOT_PRICE_DIVERGENCE_FILE: &str = "data/chart-plot-price-divergence.csv";
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct PlotPriceDivergenceData {
    pub timestamp: u64,
    pub time_since_last_update_cache: u64,
    pub is_profit_pool: bool,
    pub divergence_value: String,
    pub divergence_bp: String,
}
impl PlotPriceDivergenceData {
    pub fn load() -> Result<Vec<Self>> {
        let mut csv_reader = csv::Reader::from_path(CHART_PLOT_PRICE_DIVERGENCE_FILE)
            .wrap_err("Error reading plot price divergence csv file")?;

        let mut divergences: Vec<PlotPriceDivergenceData> = csv_reader
            .deserialize::<PlotPriceDivergenceData>()
            .collect::<Result<Vec<_>, _>>()?;
        divergences.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        info!(
            "Reading plot price divergence file done.({})",
            divergences.len()
        );

        Ok(divergences)
    }
}
pub fn generate_chart_plot_price_divergence_csv() -> Result<()> {
    info!("Generating csv for price divergence plot chart...");
    let mut csv_writer = csv::Writer::from_path(CHART_PLOT_PRICE_DIVERGENCE_FILE)?;
    let swap_with_dai_and_spot_vec = SwapWithDaiAndSpotCsv::load()?;

    for (pos_swap, swap) in swap_with_dai_and_spot_vec.iter().enumerate() {
        if pos_swap % 10_000 == 0 {
            info!(
                "Processing swap [{}/{}]",
                pos_swap,
                swap_with_dai_and_spot_vec.len()
            );
            csv_writer.flush()?;
        }

        let prev_update = swap
            .eure_next_update
            .checked_sub(swap.eure_duration)
            .ok_or_eyre("Duration is less than time elapsed since last update.")?;
        let time_since_last_update_cache = swap
            .block_timestamp
            .checked_sub(prev_update)
            .ok_or_eyre("Time of prev update is greater than current time.")?;

        let (divergence_value, divergence_bp) = compute_divergence_from_swap(swap)?;

        csv_writer.serialize(PlotPriceDivergenceData {
            timestamp: swap.block_timestamp,
            time_since_last_update_cache,
            is_profit_pool: divergence_value.is_positive(),
            divergence_value: u256_str_to_float_str_6_decimals(
                &divergence_value.unsigned_abs().to_string(),
            ),
            divergence_bp: u256_str_to_basis_point(&divergence_bp.unsigned_abs().to_string()),
        })?;
    }

    info!("Generating csv for price divergence plot chart done.");
    Ok(())
}
