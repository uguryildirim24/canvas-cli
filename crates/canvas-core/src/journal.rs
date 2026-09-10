//! Journal recovery extension point for the M2-a implementation.

/// M0-c hook: M2-a will recover journals whose owner is absent here.
/// No journal state is changed until that protocol is implemented.
pub fn recover_owner_absent(_store: &crate::store::Store) -> Vec<String> {
    Vec::new()
}
