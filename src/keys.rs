//! Contracts' handle on the vault (Ferrum docs/app-contract.md, "Using another app's operations"): what proves that a
//! call of Sign's operations is Contracts' (`sign.rs`). Kept inside this crate, never handed to another, never logged.

use ferrum_kernel::vault::Vault;
use ferrum_kernel::{Ctx, DocHook, Error, Kernel, Result};
use std::sync::{Arc, Mutex};

/// Each registration's handle, by the Contract hook it registered: one process may hold kernels of several registries
/// (a test's side by side), and each kernel accepts only the handle its own registry gave.
static HANDLES: Mutex<Vec<(usize, Vault)>> = Mutex::new(Vec::new());

fn address(hook: &Arc<dyn DocHook>) -> usize {
    Arc::as_ptr(hook) as *const () as usize
}

/// Keeps the handle Contracts' registration was given, beside the Contract hook it registered with it.
pub(crate) fn remember(hook: &Arc<dyn DocHook>, v: Vault) {
    HANDLES.lock().unwrap_or_else(|e| e.into_inner()).push((address(hook), v));
}

/// Contracts' own handle for the kernel `k` serving `ctx`'s workspace.
pub(crate) fn handle(k: &Kernel, ctx: &Ctx) -> Result<Vault> {
    let missing =
        || Error::Unavailable("Contracts was registered without its vault handle: register it as the server does, hooks.app(\"contracts\", …)".into());
    let hook = k.hooks.for_tenant("Contract", &ctx.schema.packages).into_iter().find(|h| h.name() == "contracts.contract").ok_or_else(missing)?;
    let at = address(&hook);
    let held = HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    held.iter().rev().find(|(a, _)| *a == at).map(|(_, v)| v.clone()).ok_or_else(missing)
}
