//
// Pure intercept enable lifecycle transitions (unit-tested). Production
// enable/cancel/reset paths call these so cancel is not only "drop the future".
//

/// Lifecycle of node intercept enable/disable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterceptLifecycle {
    #[default]
    Disabled,
    /// Privileged setup in progress; cleanup required even if not yet Enabled.
    Enabling,
    Enabled,
    /// Prior enable failed/cancelled and rollback did not fully clean up.
    /// A new enable is forbidden until force_cleanup succeeds.
    CleanupRequired,
}

/// Whether reset/shutdown/force_cleanup must run (not only is_enabled).
pub fn needs_cleanup(
    lifecycle: InterceptLifecycle,
    has_vpn_or_proxy_resources: bool,
    has_recovery_state: bool,
) -> bool {
    matches!(
        lifecycle,
        InterceptLifecycle::Enabling
            | InterceptLifecycle::Enabled
            | InterceptLifecycle::CleanupRequired
    ) || has_vpn_or_proxy_resources
        || has_recovery_state
}

/// Whether a new enable may start.
pub fn can_begin_enable(lifecycle: InterceptLifecycle) -> Result<(), &'static str> {
    match lifecycle {
        InterceptLifecycle::Disabled => Ok(()),
        InterceptLifecycle::Enabling => Err("intercept enable already in progress"),
        InterceptLifecycle::Enabled => Err("intercept already enabled"),
        InterceptLifecycle::CleanupRequired => Err(
            "intercept cleanup required before re-enable (run disable/reset or force cleanup)",
        ),
    }
}

/// Transition when enable begins (only from Disabled).
pub fn begin_enable(lifecycle: InterceptLifecycle) -> Result<InterceptLifecycle, &'static str> {
    can_begin_enable(lifecycle)?;
    Ok(InterceptLifecycle::Enabling)
}

/// Transition when enable completes successfully.
pub fn finish_enable(lifecycle: InterceptLifecycle) -> Result<InterceptLifecycle, &'static str> {
    match lifecycle {
        InterceptLifecycle::Enabling => Ok(InterceptLifecycle::Enabled),
        InterceptLifecycle::Disabled => Err("finish_enable from Disabled"),
        InterceptLifecycle::Enabled => Err("finish_enable from Enabled"),
        InterceptLifecycle::CleanupRequired => Err("finish_enable from CleanupRequired"),
    }
}

/// After successful disable/rollback with recovery state removed.
pub fn finish_clean(_lifecycle: InterceptLifecycle) -> InterceptLifecycle {
    InterceptLifecycle::Disabled
}

/// After failed or incomplete rollback; enable must be blocked until cleanup.
#[allow(dead_code)] // used by unit tests and available for call sites
pub fn mark_cleanup_required(_lifecycle: InterceptLifecycle) -> InterceptLifecycle {
    InterceptLifecycle::CleanupRequired
}

/// Map rollback Result to lifecycle: Ok → Disabled, Err → CleanupRequired.
pub fn after_rollback(rollback_ok: bool) -> InterceptLifecycle {
    if rollback_ok {
        InterceptLifecycle::Disabled
    } else {
        InterceptLifecycle::CleanupRequired
    }
}

/// Whether enable should abort at a phase boundary.
pub fn should_abort_enable(cancel_requested: bool) -> bool {
    cancel_requested
}

///
/// Whether VPN adapter/TUN managers may be torn down after a bounded join
/// attempt. Only true when the engine task is confirmed stopped (joined or
/// aborted-and-joined). Timeout without confirmed stop must retain ownership.
///
pub fn may_teardown_vpn_after_engine_join(engine_confirmed_stopped: bool) -> bool {
    engine_confirmed_stopped
}

///
/// Whether force_cleanup may run disk-based `cleanup_stale_state` / Drop may
/// run `cleanup_vpn_sync`. False while a packet-engine JoinHandle is still
/// owned (termination unconfirmed).
///
pub fn may_run_sync_vpn_or_stale_cleanup(packet_engine_task_owned: bool) -> bool {
    !packet_engine_task_owned
}

///
/// Whether Reset may discard the manager and re-register as clean after a
/// force_cleanup attempt. False when cleanup was incomplete.
///
pub fn may_reset_reregister_after_force_cleanup(force_cleanup_ok: bool) -> bool {
    force_cleanup_ok
}

/// Cleanup-required for status: lifecycle CleanupRequired, or Disabled with
/// retained recovery ownership (disk) after incomplete process rebuild.
pub fn status_shows_cleanup_required(
    lifecycle: InterceptLifecycle,
    has_recovery_state: bool,
) -> bool {
    matches!(lifecycle, InterceptLifecycle::CleanupRequired)
        || (matches!(lifecycle, InterceptLifecycle::Disabled) && has_recovery_state)
}

/// Status/UI: incomplete cleanup is reported independently of `is_enabled`.
pub fn status_cleanup_required(lifecycle: InterceptLifecycle) -> bool {
    matches!(lifecycle, InterceptLifecycle::CleanupRequired)
}

///
/// Whether Enable may short-circuit as "already enabled".
///
/// Lifecycle is authoritative: `CleanupRequired` with a stale
/// `is_enabled == true` must never report success.
/// Returns `Ok(true)` = return Enabled without re-running setup,
/// `Ok(false)` = proceed with enable, `Err` = reject enable.
///
pub fn enable_short_circuit(
    lifecycle: InterceptLifecycle,
    is_enabled: bool,
) -> Result<bool, &'static str> {
    match lifecycle {
        InterceptLifecycle::CleanupRequired => Err(
            "intercept cleanup required before re-enable (run disable/reset or force cleanup)",
        ),
        InterceptLifecycle::Enabling => Err("intercept enable already in progress"),
        InterceptLifecycle::Enabled if is_enabled => Ok(true),
        InterceptLifecycle::Enabled => {
            //
            // Inconsistent: lifecycle Enabled but flag false — treat as
            // needing cleanup rather than blind re-enable.
            //
            Err("intercept lifecycle Enabled without is_enabled; cleanup required")
        }
        InterceptLifecycle::Disabled if is_enabled => {
            //
            // Stale flag after failed partial teardown — not "already enabled".
            //
            Err(
                "intercept cleanup required before re-enable (stale enabled flag with incomplete cleanup)",
            )
        }
        InterceptLifecycle::Disabled => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_enable_path() {
        assert_eq!(
            begin_enable(InterceptLifecycle::Disabled).unwrap(),
            InterceptLifecycle::Enabling
        );
        assert!(begin_enable(InterceptLifecycle::Enabling).is_err());
        assert!(begin_enable(InterceptLifecycle::Enabled).is_err());
        assert!(begin_enable(InterceptLifecycle::CleanupRequired).is_err());
        assert_eq!(
            finish_enable(InterceptLifecycle::Enabling).unwrap(),
            InterceptLifecycle::Enabled
        );
        assert_eq!(
            finish_clean(InterceptLifecycle::Enabled),
            InterceptLifecycle::Disabled
        );
    }

    #[test]
    fn failed_rollback_blocks_reenable() {
        let life = mark_cleanup_required(InterceptLifecycle::Enabling);
        assert_eq!(life, InterceptLifecycle::CleanupRequired);
        assert!(can_begin_enable(life).is_err());
        assert!(needs_cleanup(life, false, false));
        assert_eq!(after_rollback(false), InterceptLifecycle::CleanupRequired);
        assert_eq!(after_rollback(true), InterceptLifecycle::Disabled);
        assert!(can_begin_enable(after_rollback(true)).is_ok());
    }

    #[test]
    fn needs_cleanup_while_enabling_or_with_resources() {
        assert!(!needs_cleanup(
            InterceptLifecycle::Disabled,
            false,
            false
        ));
        assert!(needs_cleanup(
            InterceptLifecycle::Enabling,
            false,
            false
        ));
        assert!(needs_cleanup(
            InterceptLifecycle::Disabled,
            true,
            false
        ));
        assert!(needs_cleanup(
            InterceptLifecycle::Disabled,
            false,
            true
        ));
        assert!(needs_cleanup(
            InterceptLifecycle::Enabled,
            false,
            false
        ));
        assert!(needs_cleanup(
            InterceptLifecycle::CleanupRequired,
            false,
            false
        ));
    }

    #[test]
    fn abort_on_cancel_flag() {
        assert!(!should_abort_enable(false));
        assert!(should_abort_enable(true));
    }

    #[test]
    fn cleanup_required_blocks_enable_even_when_is_enabled() {
        //
        // Post-bind failure + failed rollback: is_enabled may still be true.
        //
        assert!(enable_short_circuit(InterceptLifecycle::CleanupRequired, true).is_err());
        assert!(enable_short_circuit(InterceptLifecycle::CleanupRequired, false).is_err());
        assert_eq!(
            enable_short_circuit(InterceptLifecycle::Enabled, true).unwrap(),
            true
        );
        assert_eq!(
            enable_short_circuit(InterceptLifecycle::Disabled, false).unwrap(),
            false
        );
        assert!(enable_short_circuit(InterceptLifecycle::Disabled, true).is_err());
        assert!(status_cleanup_required(InterceptLifecycle::CleanupRequired));
        assert!(!status_cleanup_required(InterceptLifecycle::Enabled));
        assert!(!status_cleanup_required(InterceptLifecycle::Disabled));
    }

    #[test]
    fn vpn_teardown_requires_confirmed_engine_stop() {
        assert!(may_teardown_vpn_after_engine_join(true));
        assert!(!may_teardown_vpn_after_engine_join(false));
        assert!(!may_run_sync_vpn_or_stale_cleanup(true));
        assert!(may_run_sync_vpn_or_stale_cleanup(false));
        assert!(!may_reset_reregister_after_force_cleanup(false));
        assert!(may_reset_reregister_after_force_cleanup(true));
        assert!(status_shows_cleanup_required(
            InterceptLifecycle::CleanupRequired,
            false
        ));
        assert!(status_shows_cleanup_required(
            InterceptLifecycle::Disabled,
            true
        ));
        assert!(!status_shows_cleanup_required(
            InterceptLifecycle::Disabled,
            false
        ));
    }
}
