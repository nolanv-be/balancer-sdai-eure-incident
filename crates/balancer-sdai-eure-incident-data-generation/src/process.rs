mod chart_cumulative_profit_loss;
mod sma_eur_usdt;
mod swap_with_dai_and_spot;

use crate::process::chart_cumulative_profit_loss::generate_chart_cumulative_profit_loss_csv;
use crate::process::sma_eur_usdt::generate_sma_eur_usdt_csv;
use crate::process::swap_with_dai_and_spot::generate_swap_with_dai_and_spot_csv;
pub use chart_cumulative_profit_loss::CumulativeProfitLossChartData;
use eyre::Result;
pub use sma_eur_usdt::SmaEurUsdtCsv;
pub use swap_with_dai_and_spot::SwapWithDaiAndSpotCsv;

pub fn start(
    is_process_sma: bool,
    is_process_swap_dai_spot: bool,
    is_process_chart_cumulative_profit_loss: bool,
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

    Ok(())
}
