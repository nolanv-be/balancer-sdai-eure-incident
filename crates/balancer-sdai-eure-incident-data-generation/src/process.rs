mod chart_cumulative_profit_loss;
mod chart_plot_price_divergence;
mod sma_eur_usdt;
mod swap_with_dai_and_spot;

use crate::helper::{DivUp, MulUp, ONE_18};
use crate::process::chart_cumulative_profit_loss::generate_chart_cumulative_profit_loss_csv;
use crate::process::chart_plot_price_divergence::generate_chart_plot_price_divergence_csv;
use crate::process::sma_eur_usdt::generate_sma_eur_usdt_csv;
use crate::process::swap_with_dai_and_spot::generate_swap_with_dai_and_spot_csv;
use alloy::primitives::{I256, U256};
pub use chart_cumulative_profit_loss::CumulativeProfitLossChartData;
pub use chart_plot_price_divergence::PlotPriceDivergenceData;
use eyre::{OptionExt, Result};
pub use sma_eur_usdt::SmaEurUsdtCsv;
use std::str::FromStr;
pub use swap_with_dai_and_spot::SwapWithDaiAndSpotCsv;

pub fn start(
    is_process_sma: bool,
    is_process_swap_dai_spot: bool,
    is_process_chart_cumulative_profit_loss: bool,
    is_process_chart_plot_price_divergence: bool,
) -> Result<()> {
    if is_process_sma {
        generate_sma_eur_usdt_csv()?
    }

    if is_process_swap_dai_spot {
        generate_swap_with_dai_and_spot_csv()?
    }

    if is_process_chart_cumulative_profit_loss {
        generate_chart_cumulative_profit_loss_csv()?
    }

    if is_process_chart_plot_price_divergence {
        generate_chart_plot_price_divergence_csv()?
    }

    Ok(())
}

pub fn i256_to_float_str_6_decimals(val_i256: &I256) -> String {
    let val_u256 = val_i256.unsigned_abs().to_string();
    let sign_to_add = if val_i256.is_negative() { "-" } else { "" };
    format!(
        "{sign_to_add}{}",
        u256_str_to_float_str_6_decimals(&val_u256)
    )
}

pub fn u256_str_to_float_str_6_decimals(val_u256: &str) -> String {
    let zero_to_add = 19usize.saturating_sub(val_u256.len());
    let val_u256 = format!("{}{val_u256}", "0".repeat(zero_to_add));
    let (int_str, dec_str) = val_u256.split_at(val_u256.len().saturating_sub(18));
    format!("{int_str}.{}", dec_str.split_at(6).0)
}

pub fn i256_to_basis_point(val_i256: &I256) -> String {
    let val_u256 = val_i256.unsigned_abs().to_string();
    let sign_to_add = if val_i256.is_negative() { "-" } else { "" };
    format!("{sign_to_add}{}", u256_str_to_basis_point(&val_u256))
}

pub fn u256_str_to_basis_point(val_u256: &str) -> String {
    let zero_to_add = 15usize.saturating_sub(val_u256.len());
    let val_u256 = format!("{}{val_u256}", "0".repeat(zero_to_add));
    let (int_str, dec_str) = val_u256.split_at(val_u256.len().saturating_sub(14));
    format!("{int_str}.{}", dec_str.split_at(2).0)
}

pub fn compute_divergence_from_swap(swap: &SwapWithDaiAndSpotCsv) -> Result<(I256, I256)> {
    let sdai_price_new = U256::from_str(&swap.sdai_price_new)?;
    let eure_price_new = U256::from_str(&swap.eure_price_new)?;
    let last_sdai_price = U256::from_str(&swap.last_sdai_price)?;
    let last_sma_eur_usdt_price = U256::from_str(&swap.last_sma_eur_usdt_price)?;
    let sdai_swap = U256::from_str(&swap.sdai_amount)?;
    let eure_swap = U256::from_str(&swap.eure_amount)?;

    let (swap_result_spot, swap_result_pool) = match swap.is_buy_eure {
        true => {
            let swap_dai_spot = sdai_swap.mul_up(last_sdai_price)?;
            let swap_dai_pool = sdai_swap.mul_up(sdai_price_new)?;

            let swap_eure_spot = swap_dai_spot.div_up(last_sma_eur_usdt_price)?;
            let swap_eure_pool = swap_dai_pool.div_up(eure_price_new)?;

            (
                swap_eure_spot.mul_up(last_sma_eur_usdt_price)?,
                swap_eure_pool.mul_up(last_sma_eur_usdt_price)?,
            )
        }
        false => (
            eure_swap.mul_up(last_sma_eur_usdt_price)?,
            eure_swap.mul_up(eure_price_new)?,
        ),
    };

    let divergence_profit_loss = I256::from(swap_result_spot)
        .checked_sub(I256::from(swap_result_pool))
        .ok_or_eyre("Profit loss overflow.")?;

    let divergence_basis_point = I256::from(swap_result_spot.div_up(swap_result_pool)?)
        .checked_sub(I256::from(ONE_18))
        .ok_or_eyre("Profit loss overflow.")?;

    Ok((divergence_profit_loss, divergence_basis_point))
}
