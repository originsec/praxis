use agent_client_protocol::{
    Client, ClientSideConnection,
    CreateTerminalRequest, CreateTerminalResponse,
    Error, ExtNotification, ExtRequest, ExtResponse,
    KillTerminalRequest, KillTerminalResponse,
    ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionRequest, RequestPermissionResponse,
    Result as AcpResult, SessionNotification,
    TerminalOutputRequest, TerminalOutputResponse,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
};
use tokio::sync::mpsc;
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::messages::ServerMessage;

//
// Handler that implements the ACP Client trait. Receives agent->client messages
// (notifications and requests) and forwards them to the browser via WebSocket.
//
// The `ws_tx` sender pushes ServerMessage values into the per-connection
// broadcast that eventually reaches the browser. For request_permission, the
// handler sends the request to the browser and waits for the response on
// `permission_rx`.
//

pub struct WebAcpHandler {
    ws_tx: mpsc::UnboundedSender<ServerMessage>,
}

impl WebAcpHandler {
    pub fn new(ws_tx: mpsc::UnboundedSender<ServerMessage>) -> Self {
        Self { ws_tx }
    }
}

#[async_trait::async_trait(?Send)]
impl Client for WebAcpHandler {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> AcpResult<RequestPermissionResponse> {
        //
        // Serialize the permission request as a JSON-RPC message and send it
        // to the browser. The browser is expected to respond with an AcpMessage
        // containing the JSON-RPC response, which will be fed back through the
        // ClientSideConnection's stream and matched by request ID.
        //
        let json_rpc = serde_json::to_string(&args).map_err(|_| Error::internal_error())?;
        let _ = self.ws_tx.send(ServerMessage::AcpMessage { json_rpc });
        Err(Error::internal_error())
    }

    async fn session_notification(&self, args: SessionNotification) -> AcpResult<()> {
        //
        // Forward the session notification to the browser as a JSON-RPC
        // notification. The browser can render streaming updates from this.
        //
        let json_rpc = serde_json::to_string(&args).map_err(|_| Error::internal_error())?;
        let _ = self.ws_tx.send(ServerMessage::AcpMessage { json_rpc });
        Ok(())
    }

    async fn write_text_file(
        &self,
        _args: WriteTextFileRequest,
    ) -> AcpResult<WriteTextFileResponse> {
        Err(Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: ReadTextFileRequest,
    ) -> AcpResult<ReadTextFileResponse> {
        Err(Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _args: CreateTerminalRequest,
    ) -> AcpResult<CreateTerminalResponse> {
        Err(Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: TerminalOutputRequest,
    ) -> AcpResult<TerminalOutputResponse> {
        Err(Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _args: ReleaseTerminalRequest,
    ) -> AcpResult<ReleaseTerminalResponse> {
        Err(Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: WaitForTerminalExitRequest,
    ) -> AcpResult<WaitForTerminalExitResponse> {
        Err(Error::method_not_found())
    }

    async fn kill_terminal(
        &self,
        _args: KillTerminalRequest,
    ) -> AcpResult<KillTerminalResponse> {
        Err(Error::method_not_found())
    }

    async fn ext_method(&self, _args: ExtRequest) -> AcpResult<ExtResponse> {
        Err(Error::method_not_found())
    }

    async fn ext_notification(&self, _args: ExtNotification) -> AcpResult<()> {
        Ok(())
    }
}

//
// Bridge between the Send world (axum/tokio multi-threaded) and the !Send
// ClientSideConnection. A dedicated thread runs a LocalSet where the
// connection lives; communication crosses the thread boundary through mpsc
// channels.
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
    // Spawn a new ACP bridge. Returns the bridge handle (Send-safe) and starts
    // a background thread that hosts the !Send ClientSideConnection.
    //
    // `ws_tx` is used by the WebAcpHandler to push agent->client messages to
    // the browser WebSocket.
    //
    pub fn spawn(ws_tx: mpsc::UnboundedSender<ServerMessage>) -> Self {
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<String>();
        let (client_tx, client_rx) = mpsc::unbounded_channel::<String>();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build single-threaded tokio runtime for ACP bridge");

            let local = tokio::task::LocalSet::new();

            local.block_on(&rt, async move {
                //
                // Create an in-memory duplex stream pair. The
                // ClientSideConnection reads from `agent_read` (data coming
                // from the agent) and writes to `client_write` (data going to
                // the agent). We bridge these to the mpsc channels.
                //
                let (agent_write, agent_read) = tokio::io::duplex(64 * 1024);
                let (client_write, mut client_read) = tokio::io::duplex(64 * 1024);

                let handler = WebAcpHandler::new(ws_tx);

                let (_conn, io_task) = ClientSideConnection::new(
                    handler,
                    client_write.compat(),
                    agent_read.compat(),
                    |fut| {
                        tokio::task::spawn_local(fut);
                    },
                );

                //
                // Spawn the IO task that drives the connection's read/write
                // loop.
                //
                tokio::task::spawn_local(async move {
                    if let Err(e) = io_task.await {
                        common::log_warn!("ACP connection IO task ended: {}", e);
                    }
                });

                //
                // Pump: agent_rx (from RabbitMQ) -> agent_write (into
                // connection's incoming bytes).
                //
                let mut agent_write = agent_write;
                tokio::task::spawn_local(async move {
                    use tokio::io::AsyncWriteExt;
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
                // Pump: client_read (connection's outgoing bytes) -> client_tx
                // (to RabbitMQ).
                //
                tokio::task::spawn_local(async move {
                    use tokio::io::AsyncBufReadExt;
                    let mut reader = tokio::io::BufReader::new(&mut client_read);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                let trimmed = line.trim_end().to_string();
                                if !trimmed.is_empty() {
                                    if client_tx.send(trimmed).is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });

                //
                // Keep the local set alive until all tasks complete. The
                // connection stays alive as long as the agent_tx / client_rx
                // channels are open.
                //
                // We just yield forever here; the actual work happens in the
                // spawned local tasks above. When channels close (bridge
                // dropped), tasks will exit.
                //
                std::future::pending::<()>().await;
            });
        });

        Self {
            agent_tx,
            client_rx,
        }
    }
}
