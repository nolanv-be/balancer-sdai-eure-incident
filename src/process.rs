use crate::process::sma_eur_usdt::generate_sma_eur_usdt_csv;
use eyre::Result;

mod sma_eur_usdt;

pub fn start(is_process_sma: bool) -> Result<()> {
    if is_process_sma {
        generate_sma_eur_usdt_csv()?
    }

    Ok(())
}
