pub mod download;
pub mod helper;
pub mod process;

use clap::Parser;
use eyre::Result;

/// Generate sDAI<>EURe incident report
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// GnosisChain RPC url: If you want to download on-chain data
    #[arg(short, long)]
    rpc_url: Option<String>,

    /// The starting block for downloading
    #[arg(short, long, default_value = "30274134")]
    start_block_download: u64,

    /// Download swap data
    #[arg(long, default_value = "false")]
    download_swap: bool,

    /// Download sDAI price data
    #[arg(long, default_value = "false")]
    download_sdai: bool,

    /// Process EUR/USDT spot price from binance to generate sma EUR/USDT
    #[arg(long, default_value = "false")]
    process_sma: bool,

    /// Process swap data and sma EUR/USDT to generate swap with DAI and spot price
    #[arg(long, default_value = "false")]
    process_swap_dai_spot: bool,

    /// Process swap with DAI and spot price to generate data for chart cumulative profit loss
    #[arg(long, default_value = "false")]
    process_chart_cumulative_profit_loss: bool,

    /// Process swap with DAI and spot price to generate data for chart plot price divergence between spot and pool
    #[arg(long, default_value = "false")]
    process_chart_plot_price_divergence: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();
    if let Some(rpc_url) = args.rpc_url {
        download::start(
            &rpc_url,
            args.start_block_download,
            args.download_swap,
            args.download_sdai,
        )
        .await?;
    }

    process::start(
        args.process_sma,
        args.process_swap_dai_spot,
        args.process_chart_cumulative_profit_loss,
        args.process_chart_plot_price_divergence,
    )?;

    Ok(())
}
