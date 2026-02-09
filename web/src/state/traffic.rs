use super::AppState;

impl AppState {
    //
    // --- Traffic search tracking ---.
    //

    /// Add a pending traffic search request ID
    pub async fn add_pending_traffic_search(&self, request_id: String) {
        let mut pending = self.pending_traffic_searches.write().await;
        pending.insert(request_id);
    }

    /// Store a traffic search response
    pub async fn store_traffic_search_response(
        &self,
        entries: Vec<common::InterceptedTrafficEntry>,
        total_count: usize,
    ) {
        //
        // Store for any pending request (since we don't have request tracking
        // in the protocol)
        // This is a simplification - we store the latest response.
        //
        let pending: Vec<String> = {
            let pending = self.pending_traffic_searches.read().await;
            pending.iter().cloned().collect()
        };
        if !pending.is_empty() {
            let mut responses = self.traffic_search_responses.write().await;
            //
            // Store for all pending requests (they'll each get a copy).
            //
            for request_id in pending {
                responses.insert(request_id, (entries.clone(), total_count));
            }
        }
    }

    /// Take a traffic search response
    pub async fn take_traffic_search_response(
        &self,
        request_id: &str,
    ) -> Option<(Vec<common::InterceptedTrafficEntry>, usize)> {
        let mut responses = self.traffic_search_responses.write().await;
        if let Some(result) = responses.remove(request_id) {
            let mut pending = self.pending_traffic_searches.write().await;
            pending.remove(request_id);
            Some(result)
        } else {
            None
        }
    }
}
