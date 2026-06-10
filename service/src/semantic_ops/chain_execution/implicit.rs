/// Check if a chain ID represents an implicit chain.
pub fn is_implicit_chain(chain_id: &str) -> bool {
    chain_id.starts_with("implicit_")
}
