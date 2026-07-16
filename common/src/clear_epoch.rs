//
// Pure helpers for service-scoped clear generation.
//
// Live batches and clear responses carry (service_instance_id, generation).
// Instance rebinding is *control-plane only* (RegistrationAck after connect /
// ServiceOnline re-register). Data-plane events never change the remembered
// instance: a delayed batch from service A after rebinding to B must be
// rejected, not used to flip state backward (ABA).
//

/// Whether a data-plane event's instance is acceptable for the current client
/// identity. Empty instance (legacy / body-fetch) is accepted without rebind.
/// A non-empty mismatched instance is always rejected.
pub fn instance_matches_current(client_instance: Option<&str>, event_instance: &str) -> bool {
    if event_instance.is_empty() {
        return true;
    }
    match client_instance {
        None => false, // not yet bound — wait for RegistrationAck
        Some(cur) => cur == event_instance,
    }
}

/// Live batch filter: never rebinds. Requires instance equality (or empty
/// legacy stamp) and generation >= clear epoch.
pub fn accept_live_batch(
    client_instance: Option<&str>,
    client_epoch: u64,
    batch_instance: &str,
    batch_gen: u64,
) -> bool {
    if !instance_matches_current(client_instance, batch_instance) {
        return false;
    }
    batch_gen >= client_epoch
}

/// Apply a successful clear response for the *current* instance only.
/// Returns false if the clear belongs to a different instance (no mutation).
pub fn apply_clear_response(
    client_instance: Option<&str>,
    client_epoch: &mut u64,
    clear_instance: &str,
    clear_gen: u64,
) -> bool {
    if !instance_matches_current(client_instance, clear_instance) {
        //
        // Empty clear_instance is legacy (matches_current true); mismatched
        // non-empty is rejected.
        //
        return false;
    }
    if clear_gen > *client_epoch {
        *client_epoch = clear_gen;
    }
    true
}

///
/// Whether a RegistrationAck may rebind (or confirm) client instance identity.
///
/// - Nonce must match when the client set a non-empty expected nonce.
/// - When `expected_from_service_online` is non-empty, the ack instance must
///   match it even on first bind (ServiceOnline before initial register).
/// - First bind with no expected: any non-empty ack instance is accepted.
/// - Idempotent: ack equals current.
///
pub fn may_accept_registration_ack(
    current: Option<&str>,
    ack_instance: &str,
    expected_from_service_online: Option<&str>,
    expected_nonce: &str,
    ack_nonce: &str,
) -> bool {
    if ack_instance.is_empty() {
        return false;
    }
    if !expected_nonce.is_empty() && ack_nonce != expected_nonce {
        return false;
    }
    //
    // Expected instance is authoritative whenever announced.
    //
    if let Some(exp) = expected_from_service_online {
        if !exp.is_empty() && exp != ack_instance {
            return false;
        }
    }
    match current {
        None => true,
        Some(cur) if cur == ack_instance => true,
        Some(_) => match expected_from_service_online {
            Some(exp) if !exp.is_empty() && exp == ack_instance => true,
            _ => false,
        },
    }
}

///
/// How the client should treat a rejected RegistrationAck for an in-flight
/// attempt. Wrong-instance with matching nonce should retry (another service
/// consumer may answer); nonce mismatch is ignore/wait.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationRejectAction {
    /// Put pending back and wait for another ack (or worker re-publish).
    Retry,
    /// Permanent failure for this attempt.
    Fail,
}

/// Marker returned on oneshot so the registration worker re-publishes.
pub const REGISTRATION_RETRY_MARKER: &str = "__retry_wrong_instance__";

pub fn registration_reject_action(
    expected_nonce: &str,
    ack_nonce: &str,
    expected_instance: Option<&str>,
    ack_instance: &str,
) -> RegistrationRejectAction {
    if expected_nonce != ack_nonce {
        return RegistrationRejectAction::Retry; // ignore foreign attempt
    }
    if let Some(exp) = expected_instance {
        if !exp.is_empty() && exp != ack_instance {
            //
            // Right nonce, wrong service (shared queue) — retry until B acks.
            //
            return RegistrationRejectAction::Retry;
        }
    }
    RegistrationRejectAction::Fail
}

/// Whether a pending clear's expected instance accepts a TrafficCleared stamp.
pub fn clear_pending_accepts_response(
    expected_instance: &str,
    response_instance: &str,
) -> bool {
    if expected_instance.is_empty() || response_instance.is_empty() {
        return true;
    }
    expected_instance == response_instance
}

/// Authoritative rebind from a validated RegistrationAck. Caller must have
/// checked [`may_accept_registration_ack`]. Returns true when identity changed.
pub fn rebind_service_instance(
    client_instance: &mut Option<String>,
    client_epoch: &mut u64,
    new_instance: &str,
) -> bool {
    if new_instance.is_empty() {
        return false;
    }
    if client_instance.as_deref() == Some(new_instance) {
        return false;
    }
    *client_instance = Some(new_instance.to_string());
    *client_epoch = 0;
    true
}

#[cfg(test)]
mod tests {
    use super::{
        accept_live_batch, apply_clear_response, clear_pending_accepts_response,
        instance_matches_current, may_accept_registration_ack, rebind_service_instance,
        registration_reject_action, RegistrationRejectAction,
    };

    #[test]
    fn restart_via_rebind_accepts_new_gen0() {
        let mut inst = Some("svc-a".into());
        let mut epoch = 5u64;
        assert!(!accept_live_batch(inst.as_deref(), epoch, "svc-a", 3));
        //
        // Data-plane must NOT rebind: delayed or new-instance batches rejected
        // until RegistrationAck.
        //
        assert!(!accept_live_batch(inst.as_deref(), epoch, "svc-b", 0));
        assert_eq!(inst.as_deref(), Some("svc-a"));
        assert_eq!(epoch, 5);

        assert!(rebind_service_instance(&mut inst, &mut epoch, "svc-b"));
        assert_eq!(inst.as_deref(), Some("svc-b"));
        assert_eq!(epoch, 0);
        assert!(accept_live_batch(inst.as_deref(), epoch, "svc-b", 0));
        assert!(accept_live_batch(inst.as_deref(), epoch, "svc-b", 1));
    }

    #[test]
    fn delayed_old_instance_batch_rejected_after_rebind() {
        let mut inst = Some("svc-a".into());
        let mut epoch = 2u64;
        assert!(rebind_service_instance(&mut inst, &mut epoch, "svc-b"));
        assert_eq!(epoch, 0);
        // Delayed A after B: reject, stay on B.
        assert!(!accept_live_batch(inst.as_deref(), epoch, "svc-a", 99));
        assert_eq!(inst.as_deref(), Some("svc-b"));
        assert!(!apply_clear_response(inst.as_deref(), &mut epoch, "svc-a", 10));
        assert_eq!(epoch, 0);
        assert_eq!(inst.as_deref(), Some("svc-b"));
    }

    #[test]
    fn clear_response_advances_epoch_on_same_instance() {
        let inst: Option<String> = Some("svc-a".into());
        let mut epoch = 2u64;
        assert!(apply_clear_response(inst.as_deref(), &mut epoch, "svc-a", 5));
        assert_eq!(epoch, 5);
        assert!(!accept_live_batch(inst.as_deref(), epoch, "svc-a", 4));
        assert!(accept_live_batch(inst.as_deref(), epoch, "svc-a", 5));
    }

    #[test]
    fn empty_instance_falls_back_to_generation_only() {
        let inst: Option<String> = Some("svc-a".into());
        let epoch = 3u64;
        // Empty stamp is legacy: generation-only, does not rebind.
        assert!(!accept_live_batch(inst.as_deref(), epoch, "", 2));
        assert!(accept_live_batch(inst.as_deref(), epoch, "", 3));
        // Unbound client rejects non-empty data until RegistrationAck.
        assert!(!accept_live_batch(None, 0, "svc-a", 0));
        assert!(accept_live_batch(None, 0, "", 0));
    }

    #[test]
    fn rebind_idempotent_same_instance() {
        let mut inst = Some("svc-a".into());
        let mut epoch = 4u64;
        assert!(!rebind_service_instance(&mut inst, &mut epoch, "svc-a"));
        assert_eq!(epoch, 4);
        assert!(!rebind_service_instance(&mut inst, &mut epoch, ""));
        assert_eq!(inst.as_deref(), Some("svc-a"));
    }

    #[test]
    fn instance_matches_current_pure() {
        assert!(instance_matches_current(Some("a"), "a"));
        assert!(!instance_matches_current(Some("a"), "b"));
        assert!(instance_matches_current(Some("a"), ""));
        assert!(!instance_matches_current(None, "a"));
        assert!(instance_matches_current(None, ""));
    }

    #[test]
    fn delayed_old_registration_ack_rejected_after_rebind() {
        // A -> B via ServiceOnline expected B, then delayed A ack.
        assert!(may_accept_registration_ack(
            Some("svc-a"),
            "svc-b",
            Some("svc-b"),
            "n1",
            "n1"
        ));
        assert!(!may_accept_registration_ack(
            Some("svc-b"),
            "svc-a",
            Some("svc-b"),
            "n2",
            "n2"
        ));
        // Wrong nonce rejected.
        assert!(!may_accept_registration_ack(
            Some("svc-a"),
            "svc-b",
            Some("svc-b"),
            "n1",
            "n-wrong"
        ));
        // After rebind, delayed A without matching expected is rejected.
        assert!(!may_accept_registration_ack(
            Some("svc-b"),
            "svc-a",
            None,
            "",
            ""
        ));
        // First bind without expected accepts any non-empty instance.
        assert!(may_accept_registration_ack(None, "svc-a", None, "n0", "n0"));
        // First bind with expected B rejects A (ServiceOnline-before-register).
        assert!(!may_accept_registration_ack(
            None,
            "svc-a",
            Some("svc-b"),
            "n1",
            "n1"
        ));
        assert!(may_accept_registration_ack(
            None,
            "svc-b",
            Some("svc-b"),
            "n1",
            "n1"
        ));
        assert_eq!(
            registration_reject_action("n1", "n1", Some("svc-b"), "svc-a"),
            RegistrationRejectAction::Retry
        );
        assert_eq!(
            registration_reject_action("n1", "n-other", Some("svc-b"), "svc-b"),
            RegistrationRejectAction::Retry
        );
    }

    #[test]
    fn foreign_clear_response_rejected_for_pending() {
        assert!(clear_pending_accepts_response("svc-a", "svc-a"));
        assert!(!clear_pending_accepts_response("svc-a", "svc-b"));
        assert!(!clear_pending_accepts_response("svc-b", "svc-a"));
        // Legacy empty stamps do not hard-fail.
        assert!(clear_pending_accepts_response("", "svc-a"));
        assert!(clear_pending_accepts_response("svc-a", ""));
    }
}
