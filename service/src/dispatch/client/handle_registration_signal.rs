async fn handle_registration_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::Registration(registration) => {
            if let Err(e) = ctx.client_handler.handle_client_registration(registration).await {
                common::log_error!("Failed to handle ClientRegistration: {}", e);
            }
            //
            // Broadcast current event logging setting so new clients align.
            //
            let enabled = {
                let config = ctx.service_config.read().await;
                config.get_bool(APPLICATION_LOGS_ENABLED, false)
            };
            let node_message = NodeBroadcastMessage::EventLoggingSet { enabled };
            let _ =
                publish_json_exchange(&ctx.broadcast_channel, NODE_BROADCAST_EXCHANGE, &node_message)
                    .await;
            let client_message = ClientBroadcastMessage::EventLoggingSet { enabled };
            let _ = publish_json_exchange(
                &ctx.broadcast_channel,
                CLIENT_BROADCAST_EXCHANGE,
                &client_message,
            )
            .await;
        }

        _ => unreachable!("non-registration message routed to handle_registration_signal"),
    }
}

