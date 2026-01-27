
#[tokio::main]
async fn main() {
    //
    // Initialize logging.
    //
    tracing_subscriber::fmt::init();

    common::log_info!("Starting Praxis Service");

    if let Err(e) = praxis_service::run().await {
        common::log_error!("Service error: {}", e);
        std::process::exit(1);
    }
}
