
#[tokio::main]
async fn main() {
    //
    // Initialize logging.
    //
    tracing_subscriber::fmt::init();

    if let Err(e) = praxis_web::run().await {
        common::log_error!("Server error: {}", e);
        std::process::exit(1);
    }
}
