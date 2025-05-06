mod download;
pub mod helper;

use eyre::Result;
use clap::Parser;

/// Generate sDAI<>EURe incident report
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// GnosisChain RPC url: If you want to download on-chain data
    #[arg(short, long)]
    rpc_url: Option<String>
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();
    if let Some(rpc_url) = args.rpc_url {
        download::start(&rpc_url).await?;
    }

    Ok(())
}