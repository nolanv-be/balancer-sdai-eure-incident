use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::{Router, routing};
use balancer_sdai_eure_incident_data_generation::process::CumulativeProfitLossChartData;
use eyre::Result;
use log::info;
use std::path::PathBuf;

const APP_UNIX_SOCKET: &str = "balancer-sdai-eure-incident-app.socket";

#[derive(Clone)]
struct AppState {
    cumulative_profit_loss_data_vec: Vec<CumulativeProfitLossChartData>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;

    let app_socket_path = PathBuf::from(format!("{runtime_dir}/{APP_UNIX_SOCKET}"));

    let listener =
        tokio::net::UnixListener::from_std(get_nonblocking_unix_listener(app_socket_path.clone())?)
            .expect("Failed to convert to tokio socket listener");

    let app_state = AppState {
        cumulative_profit_loss_data_vec: CumulativeProfitLossChartData::load()?,
    };

    let app = Router::new()
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new("crates/balancer-sdai-eure-incident-app/assets"),
        )
        .route("/", routing::get(get_report))
        .with_state(app_state);

    info!("Listening on {}", app_socket_path.display());
    axum::serve(listener, app).await?;

    Ok(())
}

async fn get_report(
    State(app): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    #[derive(Debug, Template)]
    #[template(path = "index.html")]
    struct Tmpl {
        cumulative_profit_loss_data_vec: Vec<CumulativeProfitLossChartData>,
    }

    let template = Tmpl {
        cumulative_profit_loss_data_vec: app.cumulative_profit_loss_data_vec,
    };
    Ok(Html(template.render().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to render template".into(),
        )
    })?))
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
