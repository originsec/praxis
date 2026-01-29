use super::DynamicAgent;

impl DynamicAgent {
    //
    // Perform fingerprinting for dynamic agent.
    // Dynamic agents are always "available" since they were created from a
    // discovered endpoint.
    //

    pub(super) fn do_fingerprint_impl(&self) -> bool {
        true
    }
}
