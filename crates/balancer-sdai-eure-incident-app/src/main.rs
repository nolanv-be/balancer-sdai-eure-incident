use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::{Router, routing};
use eyre::Result;
use log::info;
use std::path::PathBuf;

const APP_UNIX_SOCKET: &str = "balancer-sdai-eure-incident-app.socket";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;

    let app_socket_path = PathBuf::from(format!("{runtime_dir}/{APP_UNIX_SOCKET}"));

    let listener =
        tokio::net::UnixListener::from_std(get_nonblocking_unix_listener(app_socket_path.clone())?)
            .expect("Failed to convert to tokio socket listener");

    let app = Router::new()
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new("crates/balancer-sdai-eure-incident-app/assets"),
        )
        .route("/", routing::get(get_report));

    info!("Listening on {}", app_socket_path.display());
    axum::serve(listener, app).await?;

    Ok(())
}

async fn get_report() -> Result<impl IntoResponse, (StatusCode, String)> {
    #[derive(Debug, Template)]
    #[template(path = "index.html")]
    struct Tmpl {
        dates: Vec<String>,
        losses: Vec<f64>,
        cumulative_losses: Vec<f64>,
    }

    let template = Tmpl {
        dates: vec![
            "2022-01-01".into(),
            "2022-01-02".into(),
            "2022-01-03".into(),
            "2022-01-04".into(),
            "2022-01-05".into(),
        ],
        losses: vec![-0.6, -2.2, -1.1, 0.8, 0.2],
        cumulative_losses: vec![-0.6, -2.8, -3.9, -3.1, -2.9],
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
