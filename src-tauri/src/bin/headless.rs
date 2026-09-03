#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Ensure demo output goes to stdout
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    nexus_backend::run_headless_demo().await
}
