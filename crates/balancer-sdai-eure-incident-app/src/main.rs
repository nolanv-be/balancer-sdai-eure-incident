use askama::Template;
use async_compression::tokio::bufread::GzipEncoder;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::{Router, routing};
use balancer_sdai_eure_incident_data_generation::process::{
    CumulativeProfitLossChartData, PlotPriceDivergenceData, SwapWithDaiAndSpotCsv,
    u256_str_to_float_str_6_decimals,
};
use eyre::OptionExt;
use log::{debug, info};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::io::{AsyncReadExt, BufReader};

const APP_UNIX_SOCKET: &str = "balancer-sdai-eure-incident-app.socket";
const TIMESTAMP_CACHE_EXPIRATION_UPDATED: u64 = 1744051806;

#[derive(Clone)]
struct AppCache {
    index: (Html<String>, Vec<u8>),
    cumulative_profit_loss: (Html<String>, Vec<u8>),
    price_spot_vs_pool: (Html<String>, Vec<u8>),
    volume_by_price_divergence: (Html<String>, Vec<u8>),
}

#[derive(Debug, Template, Clone)]
#[template(path = "index.html")]
struct IndexTemplate {
    date_cache_expiration_updated: String,
}

#[derive(Debug, Template, Clone)]
#[template(path = "components/cumulative-profit-loss.html")]
struct CumulativeProfitLossTemplate {
    cumulative_profit_loss_data_vec: Vec<CumulativeProfitLossChartData>,
}

#[derive(Debug, Template, Clone)]
#[template(path = "components/price-spot-vs-pool.html")]
struct PriceSpotVsPoolTemplate {
    prices: Vec<PriceSpotVsPoolWithDate>,
}

#[derive(Debug, Clone)]
struct PriceSpotVsPoolWithDate {
    date: String,
    price_spot: String,
    price_pool: String,
}

#[derive(Debug, Template, Clone)]
#[template(path = "components/volume-by-price-divergence.html")]
struct VolumeByPriceDivergenceTemplate {
    price_divergences_bp: Vec<VolumeByPriceDivergence>,
}

#[derive(Debug, Clone)]
struct VolumeByPriceDivergence {
    price_divergence_bp: String,
    price_divergence_value_pre: f64,
    price_divergence_value_post: f64,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    env_logger::init();

    let app_socket_path = PathBuf::from(APP_UNIX_SOCKET);

    let listener = tokio::net::UnixListener::from_std(get_nonblocking_unix_listener(
        app_socket_path.clone(),
    )?)?;

    let app_state = load_app_cache().await?;

    let app = Router::new()
        .nest_service("/assets", tower_http::services::ServeDir::new("assets"))
        .route(
            "/",
            routing::get(|header_map: HeaderMap, State(app): State<AppCache>| async {
                get_raw_or_gzip_response(header_map, app.index.0, app.index.1).await
            }),
        )
        .route(
            "/cumulative-profit-loss",
            routing::get(|header_map: HeaderMap, State(app): State<AppCache>| async {
                get_raw_or_gzip_response(
                    header_map,
                    app.cumulative_profit_loss.0,
                    app.cumulative_profit_loss.1,
                )
                .await
            }),
        )
        .route(
            "/price-spot-vs-pool",
            routing::get(|header_map: HeaderMap, State(app): State<AppCache>| async {
                get_raw_or_gzip_response(
                    header_map,
                    app.price_spot_vs_pool.0,
                    app.price_spot_vs_pool.1,
                )
                .await
            }),
        )
        .route(
            "/volume-by-price-divergence",
            routing::get(|header_map: HeaderMap, State(app): State<AppCache>| async {
                get_raw_or_gzip_response(
                    header_map,
                    app.volume_by_price_divergence.0,
                    app.volume_by_price_divergence.1,
                )
                .await
            }),
        )
        .with_state(app_state);

    info!("Listening on {}", app_socket_path.display());
    axum::serve(listener, app).await?;

    Ok(())
}

async fn load_app_cache() -> eyre::Result<AppCache> {
    let plot_price_divergence_data_vec = PlotPriceDivergenceData::load()?;
    let swap_with_dai_and_spot_data_vec = SwapWithDaiAndSpotCsv::load()?;

    let index_template = IndexTemplate {
        date_cache_expiration_updated: chrono::DateTime::<chrono::Utc>::from_timestamp(
            TIMESTAMP_CACHE_EXPIRATION_UPDATED as i64,
            0,
        )
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .to_string(),
    };
    let cumulative_profit_loss_template = CumulativeProfitLossTemplate {
        cumulative_profit_loss_data_vec: CumulativeProfitLossChartData::load()?,
    };

    let price_spot_vs_pool_template = PriceSpotVsPoolTemplate {
        prices: swap_with_dai_and_spot_data_vec
            .iter()
            .map(|s| PriceSpotVsPoolWithDate {
                date: chrono::DateTime::<chrono::Utc>::from_timestamp(s.block_timestamp as i64, 0)
                    .unwrap()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                    .to_string(),
                price_spot: u256_str_to_float_str_6_decimals(&s.last_sma_eur_usdt_price),
                price_pool: u256_str_to_float_str_6_decimals(&s.eure_price_new),
            })
            .collect(),
    };

    let mut volume_by_price_divergence_map: HashMap<String, VolumeByPriceDivergence> =
        HashMap::new();
    for divergence_data in plot_price_divergence_data_vec {
        let price_divergence_bp = if divergence_data.is_profit_pool {
            f64::from_str(&divergence_data.divergence_bp)?
        } else {
            -f64::from_str(&divergence_data.divergence_bp)?
        }
        .round()
        .to_string();

        if !volume_by_price_divergence_map.contains_key(&price_divergence_bp) {
            volume_by_price_divergence_map.insert(
                price_divergence_bp.clone(),
                VolumeByPriceDivergence {
                    price_divergence_bp: price_divergence_bp.clone(),
                    price_divergence_value_pre: 0.0,
                    price_divergence_value_post: 0.0,
                },
            );
        }

        let volume_by_price_divergence = volume_by_price_divergence_map
            .get_mut(&price_divergence_bp)
            .ok_or_eyre("No volume by price divergence for price divergence bp")?;

        if divergence_data.timestamp < TIMESTAMP_CACHE_EXPIRATION_UPDATED {
            volume_by_price_divergence.price_divergence_value_pre +=
                f64::from_str(&divergence_data.divergence_value)?;
        } else {
            volume_by_price_divergence.price_divergence_value_post +=
                f64::from_str(&divergence_data.divergence_value)?;
        }
    }
    let volume_by_price_divergence_template = VolumeByPriceDivergenceTemplate {
        price_divergences_bp: volume_by_price_divergence_map.into_values().collect(),
    };

    let index_html = Html(index_template.render()?);
    let cumulative_profit_loss_html = Html(cumulative_profit_loss_template.render()?);
    let price_spot_vs_pool_html = Html(price_spot_vs_pool_template.render()?);
    let volume_by_price_divergence_html = Html(volume_by_price_divergence_template.render()?);
    Ok(AppCache {
        index: (index_html.clone(), html_to_gzip_response(index_html).await?),
        cumulative_profit_loss: (
            cumulative_profit_loss_html.clone(),
            html_to_gzip_response(cumulative_profit_loss_html).await?,
        ),
        price_spot_vs_pool: (
            price_spot_vs_pool_html.clone(),
            html_to_gzip_response(price_spot_vs_pool_html).await?,
        ),
        volume_by_price_divergence: (
            volume_by_price_divergence_html.clone(),
            html_to_gzip_response(volume_by_price_divergence_html).await?,
        ),
    })
}

async fn html_to_gzip_response(html: Html<String>) -> eyre::Result<Vec<u8>> {
    let reader = BufReader::new(html.0.as_bytes());
    let mut gzip = GzipEncoder::new(reader);
    let mut compressed_body = Vec::new();
    gzip.read_to_end(&mut compressed_body).await?;

    Ok(compressed_body)
}

async fn get_raw_or_gzip_response(
    header_map: HeaderMap,
    html: Html<String>,
    gzip: Vec<u8>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if let Some(accept_encoding) = header_map.get(header::ACCEPT_ENCODING) {
        if accept_encoding.to_str().unwrap().contains("gzip") {
            debug!("Gzip response");

            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                "text/html; charset=utf-8".parse().unwrap(),
            );
            headers.insert(header::CONTENT_ENCODING, "gzip".parse().unwrap());

            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(header::CONTENT_ENCODING, "gzip")
                .body(gzip.into_response().into_body())
                .unwrap());
        }
    }
    Ok(html.into_response())
}

fn get_nonblocking_unix_listener(
    socket_path: PathBuf,
) -> std::io::Result<std::os::unix::net::UnixListener> {
    let is_socket_exist = std::fs::exists(socket_path.clone())?;
    match is_socket_exist {
        true => {
            std::fs::remove_file(&socket_path)?;
        }
        false => {
            std::fs::create_dir_all(socket_path.parent().ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No parent directory for socket path",
            ))?)?;
        }
    }

    let listener = std::os::unix::net::UnixListener::bind(socket_path)?;
    listener.set_nonblocking(true)?;

    Ok(listener)
}
