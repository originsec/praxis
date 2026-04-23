use agent_client_protocol as acp;
use acp::schema::{
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SessionNotification,
};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::messages::ServerMessage;

//
// Bridge between the Send world (axum/tokio multi-threaded) and the ACP
// client connection. The connection is built via `Client.builder().
// connect_with(...)` and driven on the ambient tokio runtime; incoming
// bytes from the agent (RabbitMQ) are piped in over a DuplexStream, and
// outgoing bytes flow back out over another.
//

pub struct AcpBridge {
    /// Feed raw JSON-RPC bytes from the agent (RabbitMQ) into the connection.
    pub agent_tx: mpsc::UnboundedSender<String>,

    /// Receive raw JSON-RPC bytes produced by the connection (to send to
    /// RabbitMQ / the agent).
    pub client_rx: mpsc::UnboundedReceiver<String>,
}

impl AcpBridge {
    //
    // Spawn a new ACP bridge. Returns the bridge handle (Send-safe) and
    // starts a background task that hosts the ACP client connection.
    //
    // `ws_tx` is used by handlers to push agent->client messages to the
    // browser WebSocket.
    //

    pub fn spawn(ws_tx: mpsc::UnboundedSender<ServerMessage>) -> Self {
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<String>();
        let (client_tx, client_rx) = mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            //
            // DuplexStream pair bridging the ACP connection's byte I/O to
            // the RabbitMQ mpsc channels above.
            //

            let (agent_write, agent_read) = tokio::io::duplex(64 * 1024);
            let (client_write, mut client_read) = tokio::io::duplex(64 * 1024);

            //
            // Pump: agent_rx -> agent_write (into connection's incoming).
            //

            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                let mut agent_write = agent_write;
                while let Some(line) = agent_rx.recv().await {
                    if agent_write.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    if agent_write.write_all(b"\n").await.is_err() {
                        break;
                    }
                    if agent_write.flush().await.is_err() {
                        break;
                    }
                }
            });

            //
            // Pump: client_read (outgoing) -> client_tx (to RabbitMQ).
            //

            let client_tx_pump = client_tx.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut reader = tokio::io::BufReader::new(&mut client_read);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim_end().to_string();
                            if !trimmed.is_empty() && client_tx_pump.send(trimmed).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });

            let transport = acp::ByteStreams::new(client_write.compat_write(), agent_read.compat());

            let ws_tx_notif = ws_tx.clone();

            let result = acp::Client
                .builder()
                .name("praxis-web")
                .on_receive_notification(
                    move |notif: SessionNotification, _cx| {
                        let ws_tx = ws_tx_notif.clone();
                        async move {
                            //
                            // Forward the session notification to the browser
                            // as a JSON-RPC frame.
                            //

                            let json_rpc = serde_json::to_string(&notif).map_err(|_| {
                                acp::util::internal_error("serialize SessionNotification")
                            })?;
                            let _ = ws_tx.send(ServerMessage::AcpMessage { json_rpc });
                            Ok(())
                        }
                    },
                    acp::on_receive_notification!(),
                )
                .on_receive_request(
                    move |args: RequestPermissionRequest,
                          responder: acp::Responder<RequestPermissionResponse>,
                          _cx: acp::ConnectionTo<acp::Agent>| {
                        let ws_tx = ws_tx.clone();
                        async move {
                            //
                            // Serialize the permission request and push it to
                            // the browser, then cancel so the agent doesn't
                            // block. The 0.10 handler also returned a
                            // terminal response here — behaviour preserved.
                            //

                            let json_rpc = serde_json::to_string(&args).map_err(|_| {
                                acp::util::internal_error(
                                    "serialize RequestPermissionRequest",
                                )
                            })?;
                            let _ = ws_tx.send(ServerMessage::AcpMessage { json_rpc });
                            responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Cancelled,
                            ))
                        }
                    },
                    acp::on_receive_request!(),
                )
                .connect_with(transport, async |_cx| {
                    //
                    // Keep the connection alive until the bridge is dropped.
                    // All traffic is handled via the notification/request
                    // callbacks above; no outbound requests are initiated
                    // from the web side.
                    //

                    std::future::pending::<()>().await;
                    Ok(())
                })
                .await;

            if let Err(e) = result {
                common::log_warn!("Web ACP connection ended: {}", e);
            }
        });

        Self {
            agent_tx,
            client_rx,
        }
    }
}
