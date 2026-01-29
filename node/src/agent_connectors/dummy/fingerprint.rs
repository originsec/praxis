use super::DummyAgent;

impl DummyAgent {
    //
    // Perform fingerprinting for dummy agent.
    // Dummy agent is always "available" for testing purposes.
    //

    pub(super) async fn do_fingerprint_impl(&self) -> bool {
        true
    }
}
