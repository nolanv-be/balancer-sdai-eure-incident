# Balancer sDAI/EURe incident report

This repository contains all the source code used to download and generate the incident report for Balancer's sDAI/EURe
pool on GnosisChain.
You can view the report here => [REDACTED]

## Retrieve data

### Download snapshot

You can download the data here => https://drive.proton.me/urls/MABRAGQ8M8#GSXC5ZtmGfCl

### Or fetch on-chain data

If you want to compute everything yourself, you must have access to a **LOCAL** GnosisChain archive node.
It's absolutely crucial for this node to be local because you will have to make an extremely large number of requests.

1. Compile => `cargo build --bin balancer-sdai-eure-incident-data-generation --release`

2. Download swaps. You can configure the number of parallel requests. **WARNING:** Some requests
   consume an enormous amount of RAM. With 32GB, I recommend a maximum of `4`. =>
   `RUST_LOG=info ./target/release/balancer-sdai-eure-incident-data-generation -r http://localhost:8545 --download-swap --max-concurrent-fetch 4`

3. Download sDAI price =>
   `RUST_LOG=info ./target/release/balancer-sdai-eure-incident-data-generation -r http://localhost:8545 --download-sdai`

4. Fetch Binance kLines from 10/2023 to 04/2025, and save them to data/binance-spot =>
   `https://data.binance.vision/?prefix=data/spot/monthly/klines/BTCEUR/1m/`
   `https://data.binance.vision/?prefix=data/spot/monthly/klines/BTCUSDT/1m/`

5. Add the title column to each file =>

```shell
find data/binance-spot -type f -name '*.csv' -exec sh -c '
  for file do
    cat <(echo "open_timestamp,open_price,high_price,low_price,close_price,volume,close_timestamp,quote_asset_volume,number_of_trades,taker_buy_base_asset_volume,taker_buy_quote_asset_volume,ignore") "$file" > "$file.new" &&
    mv "$file.new" "$file"
  done
' sh {} +
```

6. Generate all the data required for the incident report charts =>
   `RUST_LOG=info ./target/release/balancer-sdai-eure-incident-data-generation --process-sma --process-swap-dai-spot --process-chart-cumulative-profit-loss --process-chart-plot-price-divergence`

## Start webserver

1. Compile => `cargo build --bin balancer-sdai-eure-incident-app --release`
2. Start the websever => `RUST_LOG=info ./target/release/balancer-sdai-eure-incident-app`
3. Map Unix Domain Socket to a TCP port =>
   `socat TCP-LISTEN:8080,fork UNIX:"$XDG_RUNTIME_DIR"/balancer-sdai-eure-incident-app.socket`
4. Access the incident report => `http://localhost:8080`