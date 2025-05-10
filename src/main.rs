mod download;
pub mod helper;
mod process;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();
    if let Some(rpc_url) = args.rpc_url {
        download::start(&rpc_url, args.start_block_download).await?;
    }
    process::start()?;

    Ok(())
}
