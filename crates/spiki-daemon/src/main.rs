mod app;
mod protocol;
mod semantic;
mod session;
mod tools;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = app::parse_args()?;
    app::run(args.socket_path, args.runtime_dir).await
}
