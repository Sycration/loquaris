//! `loquaris-server [serve]` runs the server; `reset-link` and
//! `create-admin` recover access when no administrator can sign in.
use anyhow::{Context, bail};
use loquaris_server::config::Config;
use loquaris_server::{AppState, admin, auth, serve, util};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
Usage:
  loquaris-server [serve]                       run the server
  loquaris-server reset-link <email>            print a one-time link to set a password
  loquaris-server create-admin <email> [name]   create an administrator (or promote an
                                                existing account) and print its link

Configuration comes from the environment: LOQ_BIND (127.0.0.1:8080),
LOQ_DATA_DIR (./data), LOQ_PUBLIC_URL (http://localhost:5173),
LOQ_REGISTRATION (open | invite | closed), LOQ_TRUST_PROXY (0 | 1),
RUST_LOG (info).";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["serve"] => run().await,
        ["reset-link", email] => reset_link(email).await,
        ["create-admin", email, name @ ..] => create_admin(email, &name.join(" ")).await,
        ["-h" | "--help" | "help"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => bail!("unknown arguments\n\n{USAGE}"),
    }
}

async fn run() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let bind = config.bind;
    tracing::info!(
        data = %config.data_dir.display(),
        public_url = config.public_url,
        registration = ?config.registration,
        "starting Loquaris {}",
        env!("CARGO_PKG_VERSION")
    );
    let state = AppState::open(config).await?;
    let listener = TcpListener::bind(bind).await.with_context(|| format!("cannot listen on {bind}"))?;
    tracing::info!("listening on http://{bind}");
    serve(listener, state, shutdown_signal()).await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

async fn reset_link(email: &str) -> anyhow::Result<()> {
    let state = AppState::open(Config::from_env()?).await?;
    let email = util::normalize_email(email).context("not an email address")?;
    let user: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let Some((id,)) = user else { bail!("no account uses {email}") };
    let mut conn = state.db.acquire().await?;
    let link = auth::create_reset_link(&mut conn, &state.config.public_url, &id).await?;
    println!("{}\n(valid until {})", link.url, link.expires_at);
    Ok(())
}

async fn create_admin(email: &str, name: &str) -> anyhow::Result<()> {
    let state = AppState::open(Config::from_env()?).await?;
    let email = util::normalize_email(email).context("not an email address")?;
    let mut tx = state.db.begin().await?;
    let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
    let link = match existing {
        Some((id,)) => {
            sqlx::query("UPDATE users SET is_admin = 1, disabled = 0 WHERE id = ?")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            println!("{email} is now an administrator.");
            auth::create_reset_link(&mut tx, &state.config.public_url, &id).await?
        }
        None => {
            let name = if name.trim().is_empty() { "Administrator" } else { name.trim() };
            admin::create_account(&mut tx, &state.config.public_url, &email, name, true)
                .await
                .map_err(|error| anyhow::anyhow!("{error}"))?
        }
    };
    tx.commit().await?;
    println!("Set the password at:\n{}\n(valid until {})", link.url, link.expires_at);
    Ok(())
}
