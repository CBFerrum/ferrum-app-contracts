//! Contracts and clauses. A contract is drafted from approved clauses, their wording kept as it was
//! when drafted. Submitted, one worth less than the threshold is approved; one worth more waits for a
//! Contract Approver who does not own it, who approves it or sends it back saying why. Once out of
//! its draft, a contract keeps its terms. Only a Contract Approver approves a clause for use.

use crate::obj;
use async_trait::async_trait;
use chrono::Utc;
use ferrum_domain::util::{d, elevated, id, rows, s};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Result};
use rust_decimal::Decimal;
use serde_json::{Value, json};

pub(crate) fn approver(ctx: &Ctx) -> bool {
    ctx.is_admin || ctx.roles.iter().any(|r| r == "Contract Approver")
}

/// What a contract agrees: the party, the value, the term, its renewal and its clauses.
fn terms(doc: &Doc) -> Vec<Option<String>> {
    let mut t: Vec<Option<String>> =
        ["counterparty_name", "counterparty_email", "starts_on", "ends_on", "contract_type"].iter().map(|k| s(doc, k).map(str::to_string)).collect();
    t.push(Some(d(doc, "value").normalize().to_string()));
    t.push(Some(doc.get("auto_renew").and_then(Value::as_bool).unwrap_or(false).to_string()));
    t.push(doc.get("renewal_months").and_then(Value::as_i64).map(|v| v.to_string()));
    t.extend(rows(doc, "clauses").iter().map(|c| s(c, "clause").map(str::to_string)));
    t
}

/// What an approval is bound to: the contract's terms, each clause's wording as drafted, and its PDF's
/// stored SHA-256 (FEO-038). The same contract gives the same digest; any change gives another.
pub(crate) async fn approved_digest(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<String> {
    let pdf: Option<String> = match s(doc, "document").and_then(|u| u.rsplit('/').next()).and_then(|f| f.parse::<uuid::Uuid>().ok()) {
        Some(file) => sqlx::query_scalar("select sha256 from file_object where tenant_id = $1 and id = $2")
            .bind(hc.ctx.tenant)
            .bind(file)
            .fetch_optional(&mut **hc.tx)
            .await?
            .flatten(),
        None => None,
    };
    let clauses: Vec<Value> = rows(doc, "clauses").iter().map(|c| json!([s(c, "clause"), s(c, "text")])).collect();
    Ok(ferrum_kernel::receipt::digest("contract approval", None, &json!({"terms": terms(doc), "clauses": clauses, "pdf": pdf})))
}

pub struct ContractHook;

#[async_trait]
impl DocHook for ContractHook {
    fn name(&self) -> &'static str {
        "contracts.contract"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["owner", "clauses"]
    }

    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "owner").is_none() && !hc.ctx.email.is_empty() {
            doc.insert("owner".into(), json!(hc.ctx.email.to_lowercase()));
        }
        if !matches!(s(doc, "status"), Some("Draft") | None) {
            return Ok(());
        }
        // Each clause's wording as it is now.
        let mut clauses = rows(doc, "clauses");
        for c in &mut clauses {
            let text: Option<String> = sqlx::query_scalar("select text from e_clause where tenant_id = $1 and id = $2")
                .bind(hc.ctx.tenant)
                .bind(id(c, "clause"))
                .fetch_optional(&mut **hc.tx)
                .await?
                .flatten();
            c.insert("text".into(), json!(text));
        }
        doc.insert("clauses".into(), Value::Array(clauses.into_iter().map(Value::Object).collect()));
        Ok(())
    }

    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if s(doc, "ends_on") <= s(doc, "starts_on") {
            return Err(Error::field("ends_on", "is after the contract starts"));
        }
        if d(doc, "value") < Decimal::ZERO {
            return Err(Error::field("value", "is zero or more"));
        }
        if matches!(s(doc, "status"), Some("Draft") | None) {
            for c in rows(doc, "clauses") {
                let row: Option<(String, Option<bool>)> = sqlx::query_as("select clause_name, approved from e_clause where tenant_id = $1 and id = $2")
                    .bind(hc.ctx.tenant)
                    .bind(id(&c, "clause"))
                    .fetch_optional(&mut **hc.tx)
                    .await?;
                if let Some((name, approved)) = row
                    && approved != Some(true)
                {
                    return Err(Error::field("clauses", format!("{name} is not approved for use")));
                }
            }
        }
        Ok(())
    }

    async fn validate_change(&self, hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        if hc.ctx.is_system {
            return Ok(());
        }
        if let Some(o) = old
            && !matches!(s(o, "status"), Some("Draft") | None)
            && (terms(o) != terms(doc) || s(o, "document") != s(doc, "document"))
        {
            return Err(Error::invalid("a contract out of its draft keeps its terms"));
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        let contract = id(doc, "id").ok_or_else(|| Error::internal("a contract without an id"))?;
        let system = elevated(hc.ctx);
        let note = |k: &str| params.get(k).and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
        let values = match name {
            "submit_contract" => {
                let threshold: Option<Decimal> = sqlx::query_scalar("select approval_threshold from e_contract_settings where tenant_id = $1 limit 1")
                    .bind(hc.ctx.tenant)
                    .fetch_optional(&mut **hc.tx)
                    .await?
                    .flatten();
                if d(doc, "value") < threshold.unwrap_or(Decimal::ZERO) {
                    // Nobody compared its PDF with its clauses: the approval says so by naming no one.
                    json!({"status": "Approved", "decision_note": "Below the approval threshold", "approved_digest": approved_digest(hc, doc).await?})
                } else {
                    json!({"status": "In Review"})
                }
            }
            "approve_contract" | "reject_contract" => {
                if !approver(hc.ctx) {
                    return Err(Error::Forbidden("a Contract Approver decides a contract".into()));
                }
                if s(doc, "owner").is_some_and(|o| o.eq_ignore_ascii_case(&hc.ctx.email)) {
                    return Err(Error::Forbidden("a contract is approved by someone other than its owner".into()));
                }
                if name == "approve_contract" {
                    // The PDF is what gets signed: its approver says it says what the clauses say (FEO-038).
                    let checked = s(doc, "document").is_some();
                    if checked && params.get("pdf_matches").and_then(Value::as_bool) != Some(true) {
                        return Err(Error::field("pdf_matches", "read the PDF against the clauses, and say it says what they say"));
                    }
                    json!({"status": "Approved", "approved_by": hc.ctx.email, "decision_note": note("note"),
                           "approved_digest": approved_digest(hc, doc).await?, "pdf_checked_by": if checked { json!(hc.ctx.email) } else { Value::Null }})
                } else {
                    json!({"status": "Draft", "decision_note": note("note").ok_or_else(|| Error::field("note", "sending it back says why"))?})
                }
            }
            "send_for_signature" => {
                // What goes for signature is what was approved, wording and PDF (FEO-038); one approved before
                // approvals were bound has nothing to compare.
                if let Some(approved) = s(doc, "approved_digest")
                    && approved != approved_digest(hc, doc).await?
                {
                    return Err(Error::invalid("its wording or its PDF is not what was approved: send it back and approve it again"));
                }
                let request = crate::sign::send(hc.kernel, &system, hc.tx, doc).await?;
                json!({"status": "Out for Signature", "sign_request": request.to_string()})
            }
            "terminate_contract" => {
                let why = note("reason").ok_or_else(|| Error::field("reason", "a termination says why"))?;
                json!({"status": "Terminated", "terminated_on": Utc::now().date_naive().to_string(), "termination_reason": why, "notice_due": false})
            }
            _ => return Ok(None),
        };
        let status = values["status"].clone();
        hc.kernel.set_values_tx(&system, hc.tx, "Contract", contract, obj(values)).await?;
        Ok(Some(json!({"status": status})))
    }
}

pub struct ClauseHook;

#[async_trait]
impl DocHook for ClauseHook {
    fn name(&self) -> &'static str {
        "contracts.clause"
    }

    async fn validate_change(&self, hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        let was = old.and_then(|o| o.get("approved")).and_then(Value::as_bool).unwrap_or(false);
        let now = doc.get("approved").and_then(Value::as_bool).unwrap_or(false);
        if was != now && !approver(hc.ctx) {
            return Err(Error::Forbidden("a Contract Approver approves a clause for use".into()));
        }
        if was && now && old.is_some_and(|o| s(o, "text") != s(doc, "text")) && !approver(hc.ctx) {
            return Err(Error::Forbidden("an approved clause's wording changes only with a Contract Approver".into()));
        }
        Ok(())
    }
}
