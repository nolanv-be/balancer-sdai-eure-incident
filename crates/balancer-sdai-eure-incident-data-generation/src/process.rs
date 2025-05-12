mod sma_eur_usdt;
mod swap_with_dai_and_spot;

use crate::process::sma_eur_usdt::generate_sma_eur_usdt_csv;
use crate::process::swap_with_dai_and_spot::generate_swap_with_dai_and_spot_csv;
use eyre::Result;
pub use sma_eur_usdt::SmaEurUsdtCsv;

pub fn start(is_process_sma: bool, is_process_swap_dai_spot: bool) -> Result<()> {
    if is_process_sma {
        generate_sma_eur_usdt_csv()?
    }

    if is_process_swap_dai_spot {
        generate_swap_with_dai_and_spot_csv()?
    }

    Ok(())
}
