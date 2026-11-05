use crate::control::{Settings, contain, create_configuration, update_configuration};
use cumulus_core::effects::material::PeqCall;
use cumulus_core::effects::parametric::{
    ABI_VERSION, MAX_OPERATIONS, ParametricPlan, PeqOperation,
};

#[unsafe(no_mangle)]
pub extern "C" fn cumulus_peq_bypass_v1(id: u32) -> i32 {
    contain(-1, || {
        let Ok(plan) = ParametricPlan::from_calls(&[PeqCall::On { value: false }]) else {
            return -2;
        };
        update_configuration(id, Settings::Parametric(plan))
    })
}

/// Restore a checked material on the existing PEQ control. No pointer is retained.
///
/// # Safety
/// `operations` points to `count` readable operations for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_peq_restore_v1(
    id: u32,
    operations: *const PeqOperation,
    count: u32,
) -> i32 {
    contain(-1, || {
        if operations.is_null() || count == 0 || count as usize > MAX_OPERATIONS {
            return -2;
        }
        // SAFETY: caller supplies readable storage for the bounded count.
        let operations = unsafe { std::slice::from_raw_parts(operations, count as usize) };
        let Ok(plan) = ParametricPlan::from_operations(operations) else {
            return -2;
        };
        if !plan.enabled() {
            return -2;
        }
        update_configuration(id, Settings::Parametric(plan))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn cumulus_peq_version() -> u32 {
    ABI_VERSION
}

/// # Safety
/// `operations` points to `count` readable operations; `out` is a writable
/// u32. The plan is copied and no pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_peq_create_v1(
    operations: *const PeqOperation,
    count: u32,
    out: *mut u32,
) -> i32 {
    contain(-1, || {
        if operations.is_null() || count == 0 || count as usize > MAX_OPERATIONS {
            return -2;
        }
        // SAFETY: caller supplies readable storage for the bounded count.
        let operations = unsafe { std::slice::from_raw_parts(operations, count as usize) };
        let Ok(plan) = ParametricPlan::from_operations(operations) else {
            return -2;
        };
        // SAFETY: forwarded output contract is unchanged.
        unsafe { create_configuration(Settings::Parametric(plan), out) }
    })
}
