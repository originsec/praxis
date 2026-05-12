#[tokio::main]
async fn main() {
    use tracing_subscriber::EnvFilter;

    //
    // Cap noisy third-party crates at info by default so debug-level logs
    // aren't drowned by per-frame h2/hyper/rustls output. RUST_LOG still
    // overrides (e.g. RUST_LOG=h2=trace re-enables frame tracing).
    //
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("h2=info".parse().unwrap())
        .add_directive("hyper=info".parse().unwrap())
        .add_directive("rustls=info".parse().unwrap());

    tracing_subscriber::fmt().with_env_filter(filter).init();

    praxis_service::print_banner(&common::rabbitmq_url());

    common::log_info!("Starting Praxis Service");

    if let Err(e) = praxis_service::run().await {
        common::log_error!("Service error: {}", e);
        std::process::exit(1);
    }
}
