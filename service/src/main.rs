
fn main() {
    //
    // Build the tokio runtime with an 8 MB worker thread stack. The client
    // dispatch handler is a single large async match (~2200 lines, 40+
    // variants) whose compiled Future state machine exceeds tokio's default
    // 2 MB worker stack.
    //

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    runtime.block_on(async {
        //
        // Initialize logging.
        //
        tracing_subscriber::fmt::init();

        //
        // Print startup banner.
        //
        praxis_service::print_banner(&common::rabbitmq_url());

        common::log_info!("Starting Praxis Service");

        if let Err(e) = praxis_service::run().await {
            common::log_error!("Service error: {}", e);
            std::process::exit(1);
        }
    });
}
