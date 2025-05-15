use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::{Router, routing};
use balancer_sdai_eure_incident_data_generation::process::{
    CumulativeProfitLossChartData, PlotPriceDivergenceData,
};
use eyre::Result;
use log::info;
use std::path::PathBuf;

const APP_UNIX_SOCKET: &str = "balancer-sdai-eure-incident-app.socket";
const MAX_TIME_SINCE_LAST_UPDATE_CACHE: u64 = 10800;
const PLOT_GRANULARITY: usize = 108;

#[derive(Clone)]
struct AppState {
    index_template: IndexTemplate,
    cumulative_profit_loss_template: CumulativeProfitLossTemplate,
    plot_price_divergence_bp_template: PlotPriceDivergenceBPTemplate,
}

#[derive(Debug, Template, Clone)]
#[template(path = "index.html")]
struct IndexTemplate {
    dates: Vec<String>,
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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;

    let app_socket_path = PathBuf::from(format!("{runtime_dir}/{APP_UNIX_SOCKET}"));

    let listener = tokio::net::UnixListener::from_std(get_nonblocking_unix_listener(
        app_socket_path.clone(),
    )?)?;

    let app_state = load_app_state()?;

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
        .with_state(app_state);

    info!("Listening on {}", app_socket_path.display());
    axum::serve(listener, app).await?;

    Ok(())
}

fn load_app_state() -> Result<AppState> {
    let plot_price_divergence_data_vec = PlotPriceDivergenceData::load()?;

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

    let cumulative_profit_loss_chart_data_vec = CumulativeProfitLossChartData::load()?;

    Ok(AppState {
        index_template: IndexTemplate {
            dates: cumulative_profit_loss_chart_data_vec
                .iter()
                .map(|c| c.date.to_string())
                .collect(),
        },
        cumulative_profit_loss_template: CumulativeProfitLossTemplate {
            cumulative_profit_loss_data_vec: CumulativeProfitLossChartData::load()?,
        },
        plot_price_divergence_bp_template: PlotPriceDivergenceBPTemplate {
            plot_price_divergence_bp_vec,
        },
    })
}

async fn get_report(
    State(app): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(Html(app.index_template.render().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to render template".into(),
        )
    })?))
}

async fn get_chart_cumulative_profit_loss(
    State(app): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(Html(app.cumulative_profit_loss_template.render().map_err(
        |_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to render template".into(),
            )
        },
    )?))
}

async fn get_chart_plot_price_divergence(
    State(app): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok(Html(
        app.plot_price_divergence_bp_template
            .render()
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to render template".into(),
                )
            })?,
    ))
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
