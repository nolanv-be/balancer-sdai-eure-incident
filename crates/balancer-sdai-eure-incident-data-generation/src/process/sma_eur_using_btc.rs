use crate::helper::DivUp;
use alloy::primitives::U256;
use alloy::sol_types::private::u256;
use eyre::{Context, OptionExt, Result, ensure};
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

    fn load_btc_usdt_and_eur() -> Result<(Vec<Kline>, Vec<Kline>)> {
        let klines_usdt = Self::load_klines("BTCUSDT")?;
        let klines_eur = Self::load_klines("BTCEUR")?;

        ensure!(
            klines_usdt.len() == klines_eur.len(),
            "klines_usdt and klines_eur should have same length"
        );

        Ok((klines_usdt, klines_eur))
    }

    fn load_klines(name: &str) -> Result<Vec<Kline>> {
        let mut klines = Vec::new();

        info!("Loading klines {name}");
        for year in 2023..=2025 {
            for month in 1..=12 {
                let Ok(mut csv_reader) = csv::Reader::from_path(format!(
                    "data/binance-spot/{name}-1m-{year}-{:02}.csv",
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
pub struct SmaEurUsdtCsv {
    pub timestamp: u64,
    pub sma_price: String,
}

pub fn generate_sma_eur_usdt_using_btc_csv() -> Result<()> {
    info!("Generating csv of simple moving average of EUR/USDT using BTC...");

    let mut csv_writer = csv::Writer::from_path(SMA_CSV_FILE)?;

    let (mut klines_usdt, mut klines_eur) = Kline::load_btc_usdt_and_eur()?;
    klines_usdt.sort_by(|a, b| a.open_timestamp.cmp(&b.open_timestamp));
    klines_eur.sort_by(|a, b| a.open_timestamp.cmp(&b.open_timestamp));

    for id in 0..klines_usdt.len() {
        let window_usdt = klines_usdt
            .get(id.saturating_sub(SMA_LENGTH - 1)..=id)
            .ok_or_eyre("cant get window BTC/USDT")?;

        let sma_usdt: U256 = window_usdt
            .iter()
            .map(|k| k.price_to_u256().unwrap())
            .sum::<U256>()
            .checked_div(u256(window_usdt.len() as u64))
            .ok_or_eyre("Failed to calculate sma BTC/USDT")?;

        let window_eur = klines_eur
            .get(id.saturating_sub(SMA_LENGTH - 1)..=id)
            .ok_or_eyre("cant get window BTC/EUR")?;

        let sma_eur: U256 = window_eur
            .iter()
            .map(|k| k.price_to_u256().unwrap())
            .sum::<U256>()
            .checked_div(u256(window_eur.len() as u64))
            .ok_or_eyre("Failed to calculate sma BTC/EUR")?;

        let entry_usdt = klines_usdt.get(id).ok_or_eyre("cant get entry BTC/USDT")?;
        let divider_for_second_unix = if entry_usdt.open_timestamp > 9_999_999_999_999 {
            1_000_000
        } else {
            1_000
        };
        let sma_eur_usdt_csv = SmaEurUsdtCsv {
            timestamp: entry_usdt.open_timestamp / divider_for_second_unix,
            sma_price: sma_usdt.div_up(sma_eur)?.to_string(),
        };

        if id % 1000 == 0 {
            info!("SMA [{}/{}]", id, klines_usdt.len());
        }

        csv_writer.serialize(sma_eur_usdt_csv)?;
    }

    csv_writer.flush()?;
    info!("Generating csv of simple moving average of EUR/USDT using BTC done.");

    Ok(())
}

impl SmaEurUsdtCsv {
    pub fn load() -> Result<Vec<Self>> {
        let mut csv_reader = csv::Reader::from_path(SMA_CSV_FILE)
            .wrap_err("Error reading sma EUR/USDT using BTC csv file")?;

        let mut sma_eur_usdt_vec: Vec<SmaEurUsdtCsv> = csv_reader
            .deserialize::<SmaEurUsdtCsv>()
            .collect::<Result<Vec<_>, _>>()?;

        sma_eur_usdt_vec.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        info!(
            "Reading sma EUR/USDt using BTC file done.({})",
            sma_eur_usdt_vec.len()
        );

        Ok(sma_eur_usdt_vec)
    }
}
