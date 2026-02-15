//
// Semantic Parser - Service-provided AI parsing.
//

/// Request to the service's semantic parser
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SemanticParserRequest {
    /// Unique request ID for matching response
    pub request_id: String,
    /// Instructions for what to extract
    pub instruction: String,
    /// The text/data to parse
    pub text: String,
    /// JSON schema that the output must match (as a string)
    pub schema: String,
}

/// Response from the service's semantic parser
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SemanticParserResponse {
    /// Request ID for matching with the original request
    pub request_id: String,
    /// Whether parsing was successful
    pub success: bool,
    /// The parsed JSON (if successful)
    pub json: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
}

