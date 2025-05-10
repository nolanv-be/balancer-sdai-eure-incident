use alloy::primitives::U256;
use alloy::sol_types::private::u256;
use eyre::{OptionExt, Result};
use log::{debug, info};

const SMA_LENGTH: usize = 10;
const SMA_CSV_FILE: &str = "data/sma-eur-usdt.csv";

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct Kline {
    open_timestamp: u64,
    open_price: String,
    high_price: String,
    low_price: String,
    close_price: String,
    volume: String,
    close_timestamp: u64,
    quote_asset_volume: String,
    number_of_trades: u64,
    taker_buy_base_asset_volume: String,
    taker_buy_quote_asset_volume: String,
    ignore: String,
}
impl Kline {
    fn price_to_u256(&self) -> Result<U256> {
        let price_f64: f64 = self.close_price.parse()?;
        let price_f64_no_decimal: f64 = price_f64 * 10u64.pow(8) as f64;

        U256::from(price_f64_no_decimal as u64)
            .checked_mul(u256(10).pow(u256(10)))
            .ok_or_eyre("Failed to put price to base 18")
    }

    fn load() -> Result<Vec<Kline>> {
        let mut klines = Vec::new();
        info!("Loading klines");
        for year in 2023..=2025 {
            for month in 1..=12 {
                let Ok(mut csv_reader) = csv::Reader::from_path(format!(
                    "data/binance-eur-usdt-klines/EURUSDT-1m-{year}-{:02}.csv",
                    month
                )) else {
                    debug!("Skip loading klines for year {} month {}", year, month);
                    continue;
                };

                let len_before = klines.len();
                for kline in csv_reader.deserialize::<Kline>() {
                    let kline = kline?;
                    klines.push(kline);
                }
                info!(
                    "Loaded {} klines for year {} month {}",
                    klines.len() - len_before,
                    year,
                    month
                );
            }
        }

        Ok(klines)
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct SmaEurUsdtCsv {
    timestamp: u64,
    sma_price: String,
}

pub fn generate_sma_eur_usdt_csv() -> Result<()> {
    info!("Generating sma-eur-usdt.csv");

    let mut csv_writer = csv::Writer::from_path(SMA_CSV_FILE)?;

    let mut klines = Kline::load()?;
    klines.sort_by(|a, b| a.open_timestamp.cmp(&b.open_timestamp));

    for (id, kline) in klines.iter().enumerate() {
        let window = klines
            .get(id.saturating_sub(SMA_LENGTH - 1)..=id)
            .ok_or_eyre("cant get window")?;

        let sma: U256 = window
            .iter()
            .map(|k| k.price_to_u256().unwrap())
            .sum::<U256>()
            .checked_div(u256(window.len() as u64))
            .ok_or_eyre("Failed to calculate sma")?;

        let sma_eur_usdt_csv = SmaEurUsdtCsv {
            timestamp: kline.open_timestamp / 1000,
            sma_price: sma.to_string(),
        };

        if id % 1000 == 0 {
            info!("SMA [{}/{}]", id, klines.len());
        }

        csv_writer.serialize(sma_eur_usdt_csv)?;
    }

    csv_writer.flush()?;

    Ok(())
}
