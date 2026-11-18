//! Contract lifecycle management (FERRUM-ECOSYSTEM-008): contracts of a type from approved clauses,
//! their body generated and kept by version, approval by policy, signature through Sign, activation,
//! renewal, notice, obligations and reminders.
//!
//! * `contract`: drafts from the clause library, the body made from the type's template and frozen
//!   at each submit, approval through the platform's policy on Approve It, terms kept once out of the
//!   draft; clauses approved by a Contract Approver; a copy signed elsewhere recorded.
//! * `sign`: an approved contract sent to Sign, to its signatories in order, through Sign's declared
//!   operations; followed to its signature, or back to approved.
//! * `term`: signed contracts active from their start, renewed or expired at their end, notice due
//!   before a renewal; obligations overdue when late; reminders, once each; Renew It.
//! * `reports`: by state, ending or renewing, obligations due, days from draft to signed.
//!
//! This app's own design; no code was copied (docs/PROVENANCE.md). Hooks are registered per entity,
//! so a workspace without the `contracts` package never runs them.

mod contract;
mod keys;
mod reports;
mod sign;
mod term;

use chrono::NaiveDate;
use ferrum_domain::reports::ReportSource;
use ferrum_kernel::{Ctx, Doc, DocHook, HookRegistry, Kernel, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

include!(concat!(env!("OUT_DIR"), "/packages.rs"));
include!(concat!(env!("OUT_DIR"), "/ui.rs"));

/// This app's manifest (`app.yaml`).
pub const MANIFEST: &str = include_str!("../app.yaml");

/// This app's metadata packages, embedded at build time.
pub fn packages() -> std::result::Result<Vec<ferrum_meta::Package>, ferrum_meta::MetaError> {
    ferrum_meta::catalog::load_embedded(EMBEDDED)
}

/// Registers this app's document hooks, inside its registration as app `contracts` (`hooks.app("contracts", …)`): it
/// keeps its vault handle, with which it calls Sign's operations.
pub fn register(hooks: &mut HookRegistry) {
    let contract: Arc<dyn DocHook> = Arc::new(contract::ContractHook);
    keys::remember(&contract, hooks.vault());
    hooks.register("Contract", contract).register("Clause", Arc::new(contract::ClauseHook)).register("Obligation", Arc::new(term::ObligationHook));
}

/// This app's reports.
pub fn reports() -> Vec<Arc<dyn ReportSource>> {
    vec![Arc::new(reports::ContractReports)]
}

/// Upkeep of one workspace: approvals and signatures followed, terms moved on, obligations overdue, reminders sent, as
/// of the workspace's own day.
pub async fn maintenance(k: &Kernel, tenant: Uuid) -> Result<()> {
    upkeep(k, tenant, None).await
}

/// [`maintenance`] as if the workspace's day were `today`.
pub async fn maintenance_on(k: &Kernel, tenant: Uuid, today: NaiveDate) -> Result<()> {
    upkeep(k, tenant, Some(today)).await
}

async fn upkeep(k: &Kernel, tenant: Uuid, on: Option<NaiveDate>) -> Result<()> {
    let ctx = Ctx::system(tenant, k.registry.schema(tenant).await?);
    let mut tx = k.db.tenant_tx(tenant).await?;
    contract::follow_approvals(k, &ctx, &mut tx).await?;
    sign::follow(k, &ctx, &mut tx).await?;
    term::upkeep(k, &ctx, &mut tx, on.unwrap_or_else(|| ctx.today())).await?;
    tx.commit().await?;
    Ok(())
}

/// A document from JSON.
pub(crate) fn obj(v: Value) -> Doc {
    v.as_object().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The connector packages link a contract to records of apps this repository does not build with: those apps'
    /// packages stand in here by the entity each link names (CRM's Opportunity, selling's SalesOrder, buying's
    /// PurchaseOrder, HR's Employee).
    #[test]
    fn the_packages_and_their_connectors_are_valid() {
        let stub = |name: &'static str, entity: &str| -> ferrum_meta::Package {
            let yaml = format!("package: {name}\nversion: 0.0.1\ndepends: [core]\n");
            let entities = format!("- name: {entity}\n  label: {entity}\n  kind: master\n  fields:\n    - {{name: title, type: data}}\n");
            let files: Vec<(&str, &str, &str)> =
                vec![(name, "package.yaml", Box::leak(yaml.into_boxed_str())), (name, "entities/e.yaml", Box::leak(entities.into_boxed_str()))];
            ferrum_meta::catalog::load_embedded(&files).expect("stub").remove(0)
        };
        let mut all = ferrum_domain::packages().expect("platform packages");
        all.extend(ferrum_app_sign::packages().expect("Sign's packages"));
        all.extend([stub("crm", "Opportunity"), stub("selling", "SalesOrder"), stub("buying", "PurchaseOrder"), stub("hr", "Employee")]);
        all.extend(packages().expect("app packages"));
        let catalog = ferrum_meta::Catalog::from_packages(all).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> =
            ["contracts", "contracts_parties", "contracts_projects", "contracts_crm", "contracts_selling", "contracts_buying", "contracts_hr"]
                .map(String::from)
                .to_vec();
        let schema = ferrum_meta::effective::build(&catalog, &catalog.closure(&names).unwrap(), &[], 1).unwrap_or_else(|e| panic!("{e}"));
        assert!(schema.warnings.is_empty(), "{:?}", schema.warnings);
        let contract = schema.entity("Contract").expect("Contract");
        for f in ["pty_customer", "pty_supplier", "pty_contact", "prj_project", "crm_opportunity", "sel_sales_order", "buy_purchase_order", "hr_employee"] {
            assert!(contract.field(f).is_some(), "{f}");
        }
        assert!(schema.entity("SalesOrder").and_then(|e| e.field("sel_contract")).is_some(), "an order names its contract");
        assert!(schema.entity("PurchaseOrder").and_then(|e| e.field("buy_contract")).is_some(), "an order names its contract");
        let on = ferrum_meta::DocumentAction::Action("approve".into());
        assert!(contract.document_approvals.contains_key(&on), "the default approval policy comes with the package");
        let alone = ferrum_meta::effective::build(&catalog, &catalog.closure(&["contracts".to_string()]).unwrap(), &[], 1).unwrap();
        assert!(alone.warnings.is_empty() && alone.entity("Contract").unwrap().field("pty_customer").is_none(), "without the others, the typed name only");
    }

    #[test]
    fn the_interface_is_a_release_the_platform_takes() {
        let m = ferrum_apps::AppManifest::parse(MANIFEST).expect("app.yaml");
        assert_eq!(m.routes(), vec![("contracts".to_string(), vec![])]);
        let files = UI.iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
        let ui = ferrum_apps::ui::prepare("contracts", files).expect("ui/dist is an interface release");
        assert!(UI.iter().any(|(p, _)| *p == "index.html") && !UI.iter().any(|(p, _)| p.starts_with('.')), "{}", ui.digest);
    }
}
