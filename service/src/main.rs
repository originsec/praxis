
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    praxis_service::print_banner(&common::rabbitmq_url());

    common::log_info!("Starting Praxis Service");

    if let Err(e) = praxis_service::run().await {
        common::log_error!("Service error: {}", e);
        std::process::exit(1);
    }
}
