use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::{Router, routing};
use balancer_sdai_eure_incident_data_generation::process::{
    CumulativeProfitLossChartData, PlotPriceDivergenceData, SwapWithDaiAndSpotCsv,
    u256_str_to_float_str_6_decimals,
};
use eyre::{OptionExt, Result};
use log::info;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

const APP_UNIX_SOCKET: &str = "balancer-sdai-eure-incident-app.socket";
const MAX_TIME_SINCE_LAST_UPDATE_CACHE: u64 = 10800;
const PLOT_GRANULARITY: usize = 108;
const TIMESTAMP_CACHE_EXPIRATION_UPDATED: u64 = 1744051806;

#[derive(Clone)]
struct AppCache {
    index_html: Html<String>,
    cumulative_profit_loss_html: Html<String>,
    plot_price_divergence_bp_html: Html<String>,
    price_spot_vs_pool_html: Html<String>,
    plot_price_divergence_bp_pre_post_fix_html: Html<String>,
    volume_by_price_divergence_html: Html<String>,
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
#[template(path = "components/plot-price-divergence.html")]
struct PlotPriceDivergenceBPTemplate {
    plot_price_divergence_bp_vec: Vec<Vec<String>>,
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
#[template(path = "components/plot-price-divergence-pre-post-fix.html")]
struct PlotPriceDivergenceBPPrePostFixTemplate {
    plot_price_divergence_bp_pre_fix_vec: Vec<String>,
    plot_price_divergence_bp_post_fix_vec: Vec<String>,
}

#[derive(Debug, Template, Clone)]
#[template(path = "components/volume-by-price-divergence.html")]
struct VolumeByPriceDivergenceTemplate {
    volume_by_price_divergence: Vec<VolumeByPriceDivergence>,
}

#[derive(Debug, Clone)]
struct VolumeByPriceDivergence {
    price_divergence_bp: String,
    price_divergence_value_profit: f64,
    price_divergence_value_loss: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;

    let app_socket_path = PathBuf::from(format!("{runtime_dir}/{APP_UNIX_SOCKET}"));

    let listener = tokio::net::UnixListener::from_std(get_nonblocking_unix_listener(
        app_socket_path.clone(),
    )?)?;

    let app_state = load_app_cache()?;

    let app = Router::new()
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new("crates/balancer-sdai-eure-incident-app/assets"),
        )
        .route("/", routing::get(get_report))
        .route(
            "/cumulative-profit-loss",
            routing::get(get_chart_cumulative_profit_loss),
        )
        .route(
            "/plot-price-divergence",
            routing::get(get_chart_plot_price_divergence),
        )
        .route(
            "/price-spot-vs-pool",
            routing::get(get_chart_price_spot_vs_pool),
        )
        .route(
            "/plot-price-divergence-pre-post",
            routing::get(get_chart_plot_price_pre_post_fix_divergence),
        )
        .route(
            "/volume-by-price-divergence",
            routing::get(get_chart_volume_by_price_divergence),
        )
        .with_state(app_state);

    info!("Listening on {}", app_socket_path.display());
    axum::serve(listener, app).await?;

    Ok(())
}

fn load_app_cache() -> Result<AppCache> {
    let plot_price_divergence_data_vec = PlotPriceDivergenceData::load()?;
    let swap_with_dai_and_spot_data_vec = SwapWithDaiAndSpotCsv::load()?;

    let plot_price_divergence_bp_vec: Vec<Vec<String>> = (0..MAX_TIME_SINCE_LAST_UPDATE_CACHE)
        .step_by(PLOT_GRANULARITY)
        .map(|i| {
            plot_price_divergence_data_vec
                .iter()
                .filter(|c| {
                    c.time_since_last_update_cache >= i
                        && c.time_since_last_update_cache < i + PLOT_GRANULARITY as u64
                })
                .map(|c| c.divergence_bp.to_string())
                .collect()
        })
        .collect();

    let (plot_price_divergence_bp_pre_fix_vec, plot_price_divergence_bp_post_fix_vec): (
        Vec<String>,
        Vec<String>,
    ) = (
        plot_price_divergence_data_vec
            .iter()
            .filter(|c| c.timestamp < TIMESTAMP_CACHE_EXPIRATION_UPDATED)
            .map(|c| c.divergence_bp.to_string())
            .collect(),
        plot_price_divergence_data_vec
            .iter()
            .filter(|c| c.timestamp >= TIMESTAMP_CACHE_EXPIRATION_UPDATED)
            .map(|c| c.divergence_bp.to_string())
            .collect(),
    );

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
    let plot_price_divergence_bp_template = PlotPriceDivergenceBPTemplate {
        plot_price_divergence_bp_vec,
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
    let plot_price_divergence_bp_pre_post_fix_template = PlotPriceDivergenceBPPrePostFixTemplate {
        plot_price_divergence_bp_pre_fix_vec,
        plot_price_divergence_bp_post_fix_vec,
    };

    let mut volume_by_price_divergence_map: HashMap<String, VolumeByPriceDivergence> =
        HashMap::new();
    for divergence_data in plot_price_divergence_data_vec {
        let price_divergence_bp = f64::from_str(&divergence_data.divergence_bp)?
            .round()
            .to_string();

        if !volume_by_price_divergence_map.contains_key(&price_divergence_bp) {
            volume_by_price_divergence_map.insert(
                price_divergence_bp.clone(),
                VolumeByPriceDivergence {
                    price_divergence_bp: price_divergence_bp.clone(),
                    price_divergence_value_profit: 0f64,
                    price_divergence_value_loss: 0f64,
                },
            );
        }

        let volume_by_price_divergence = volume_by_price_divergence_map
            .get_mut(&price_divergence_bp)
            .ok_or_eyre("No volume by price divergence for price divergence bp")?;

        if divergence_data.is_profit_pool {
            volume_by_price_divergence.price_divergence_value_profit +=
                f64::from_str(&divergence_data.divergence_value)?;
        } else {
            volume_by_price_divergence.price_divergence_value_loss +=
                f64::from_str(&divergence_data.divergence_value)?;
        }
    }

    let volume_by_price_divergence_template = VolumeByPriceDivergenceTemplate {
        volume_by_price_divergence: volume_by_price_divergence_map.into_values().collect(),
    };

    Ok(AppCache {
        index_html: Html(index_template.render()?),
        cumulative_profit_loss_html: Html(cumulative_profit_loss_template.render()?),
        plot_price_divergence_bp_html: Html(plot_price_divergence_bp_template.render()?),
        price_spot_vs_pool_html: Html(price_spot_vs_pool_template.render()?),
        plot_price_divergence_bp_pre_post_fix_html: Html(
            plot_price_divergence_bp_pre_post_fix_template.render()?,
        ),
        volume_by_price_divergence_html: Html(volume_by_price_divergence_template.render()?),
    })
}

async fn get_report(
    State(app): State<AppCache>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(app.index_html)
}

async fn get_chart_cumulative_profit_loss(
    State(app): State<AppCache>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(app.cumulative_profit_loss_html)
}

async fn get_chart_plot_price_divergence(
    State(app): State<AppCache>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(app.plot_price_divergence_bp_html)
}

async fn get_chart_price_spot_vs_pool(
    State(app): State<AppCache>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(app.price_spot_vs_pool_html)
}

async fn get_chart_plot_price_pre_post_fix_divergence(
    State(app): State<AppCache>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(app.plot_price_divergence_bp_pre_post_fix_html)
}

async fn get_chart_volume_by_price_divergence(
    State(app): State<AppCache>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(app.volume_by_price_divergence_html)
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
