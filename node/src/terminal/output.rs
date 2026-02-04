pub struct TerminalOutputEvent {
    pub terminal_id: String,
    pub client_id: String,
    pub data: Option<Vec<u8>>,
    pub closed: bool,
}
