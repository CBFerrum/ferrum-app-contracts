//! Contracts and clauses. A contract is drafted from approved clauses, their wording kept as it was
//! when drafted, merge fields filled from the contract. One with a type has a body made from the type's
//! document template, frozen as a file each time it is submitted (each kept, as a version). Submitted, it
//! goes through the workspace's approval policy (the package's Contract Approval workflow); without one, one
//! worth less than the threshold is approved and one worth more waits for a Contract Approver who does not own
//! it. Once out of its draft, a contract keeps its terms. Only a Contract Approver approves a clause for use.

use crate::obj;
use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use ferrum_domain::util::{d, elevated, id, rows, s};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Kernel, Result, Tx};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use uuid::Uuid;

/// The transition Submit It takes when the contract's workflow has it (the package's, or a workspace's own).
pub(crate) const APPROVE: &str = "Approve";

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
pub(crate) async fn approved_digest(tx: &mut Tx, ctx: &Ctx, doc: &Doc) -> Result<String> {
    let pdf: Option<String> = match file_of(doc, "document") {
        Some(file) => sqlx::query_scalar("select sha256 from file_object where tenant_id = $1 and id = $2")
            .bind(ctx.tenant)
            .bind(file)
            .fetch_optional(&mut **tx)
            .await?
            .flatten(),
        None => None,
    };
    let clauses: Vec<Value> = rows(doc, "clauses").iter().map(|c| json!([s(c, "clause"), s(c, "text")])).collect();
    Ok(ferrum_kernel::receipt::digest("contract approval", None, &json!({"terms": terms(doc), "clauses": clauses, "pdf": pdf})))
}

/// The stored file an attach field names (`/files/{id}`).
pub(crate) fn file_of(doc: &Doc, field: &str) -> Option<Uuid> {
    s(doc, field).and_then(|u| u.rsplit('/').next()).and_then(|f| f.parse().ok())
}

/// `{{ doc.<field> }}` in a clause's wording, filled with the contract's value as text; anything else between
/// braces stays as written.
// ponytail: plain substitution, no filters; the kernel's MiniJinja renderer for a text is not public (print::message).
pub(crate) fn fill(text: &str, doc: &Doc) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else { break };
        out.push_str(&rest[..start]);
        let inner = rest[start + 2..start + len].trim();
        match inner.strip_prefix("doc.").filter(|f| f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
            Some(field) => match doc.get(field) {
                Some(Value::String(v)) => out.push_str(v),
                Some(Value::Null) | None => {}
                Some(v) => out.push_str(&v.to_string()),
            },
            None => out.push_str(&rest[start..start + len + 2]),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

/// The text of a page, for search: what is between its tags, its style and script left out, spaces collapsed.
pub(crate) fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        out.push(' ');
        let tag = rest[open + 1..].trim_start().to_ascii_lowercase();
        let skip = ["style", "script"].into_iter().find(|t| tag.starts_with(t));
        let close = match skip {
            Some(t) => rest[open..].to_ascii_lowercase().find(&format!("</{t}")).and_then(|i| rest[open + i..].find('>').map(|j| open + i + j)),
            None => rest[open..].find('>').map(|j| open + j),
        };
        let Some(close) = close else { break };
        rest = &rest[close + 1..];
    }
    if !rest.contains('<') {
        out.push_str(rest);
    }
    let out = out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The document template the contract's type names, if it has a type that names one.
async fn template_of(tx: &mut Tx, ctx: &Ctx, doc: &Doc) -> Result<Option<String>> {
    let Some(kind) = id(doc, "kind") else { return Ok(None) };
    Ok(sqlx::query_scalar("select template from e_contract_type where tenant_id = $1 and id = $2")
        .bind(ctx.tenant)
        .bind(kind)
        .fetch_optional(&mut **tx)
        .await?
        .flatten()
        .filter(|t: &String| !t.trim().is_empty()))
}

/// Makes the contract's body from its type's template and keeps it as its next version: the file attached to the
/// contract, never replaced (each version stays), its SHA-256, who and when, and its words for search. The values
/// to write on the contract, or none when it has no type with a template.
pub(crate) async fn freeze(k: &Kernel, tx: &mut Tx, ctx: &Ctx, doc: &Doc, by: &str, reason: &str) -> Result<Option<Doc>> {
    let Some(template) = template_of(tx, ctx, doc).await? else { return Ok(None) };
    let contract = id(doc, "id").ok_or_else(|| Error::internal("a contract without an id"))?;
    const KEPT: [&str; 7] = ["id", "version_no", "file", "sha256", "made_by", "made_at", "reason"];
    let mut versions: Vec<Doc> = rows(doc, "versions").into_iter().map(|r| r.into_iter().filter(|(k, _)| KEPT.contains(&k.as_str())).collect()).collect();
    let version = versions.iter().filter_map(|v| v.get("version_no").and_then(Value::as_i64)).max().unwrap_or(0) + 1;
    let title = s(doc, "title").unwrap_or("Contract");
    let g = ferrum_kernel::documents::Generate {
        template: template.clone(),
        record: contract,
        name: Some(format!("{title} v{version}")),
        language: None,
        replace: false,
        values: Default::default(),
    };
    let made = ferrum_kernel::documents::generate(k, tx, ctx, &g).await?;
    // The words of the body for search: the page as HTML, whether the file kept is a PDF or the page itself.
    let words = k.render_document(ctx, &template, contract, None, false).await.map(|page| text_of(&String::from_utf8_lossy(&page.bytes))).unwrap_or_default();
    versions.push(obj(
        json!({"version_no": version, "file": made.url, "sha256": made.sha256, "made_by": by, "made_at": Utc::now().to_rfc3339(), "reason": reason}),
    ));
    k.set_child_rows_tx(ctx, tx, "Contract", "versions", contract, versions).await?;
    Ok(Some(obj(json!({"document": made.url, "document_sha256": made.sha256, "body_text": words}))))
}

/// Whether the contract's workflow (the package's Contract Approval, or a workspace's own) has the Approve
/// transition from the state the contract is in.
fn has_approve(ctx: &Ctx, doc: &Doc) -> bool {
    let Some(wf) = ctx.schema.entity("Contract").and_then(|e| e.workflow.as_ref()) else { return false };
    let state = s(doc, "workflow_state").unwrap_or(&wf.def.initial_state);
    wf.def.transitions.iter().any(|t| t.action == APPROVE && t.from == state)
}

/// A platform approval waiting on the contract.
async fn pending_approval(tx: &mut Tx, ctx: &Ctx, contract: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i32>("select 1 from approval_request where tenant_id = $1 and entity = 'Contract' and record_id = $2 and status = 'pending'")
        .bind(ctx.tenant)
        .bind(contract)
        .fetch_optional(&mut **tx)
        .await?
        .is_some())
}

/// The clauses its type makes mandatory that it does not include, neither the clause nor its fallback.
async fn missing_mandatory(tx: &mut Tx, ctx: &Ctx, doc: &Doc) -> Result<Vec<String>> {
    let Some(kind) = id(doc, "kind") else { return Ok(vec![]) };
    let have: Vec<Uuid> = rows(doc, "clauses").iter().filter_map(|c| id(c, "clause")).collect();
    let missing: Vec<String> = sqlx::query_scalar(
        "select c.clause_name from e_contract_type__clauses m join e_clause c on c.tenant_id = m.tenant_id and c.id = m.clause \
         where m.tenant_id = $1 and m.parent_id = $2 and coalesce(m.mandatory, false) \
         and not (m.clause = any($3) or (c.fallback is not null and c.fallback = any($3))) order by m.idx",
    )
    .bind(ctx.tenant)
    .bind(kind)
    .bind(&have)
    .fetch_all(&mut **tx)
    .await?;
    Ok(missing)
}

/// Who approved the contract in its latest approval request (the last to approve), if anyone decided.
async fn decided_by(tx: &mut Tx, ctx: &Ctx, contract: Uuid) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "select lower(u.email) from approval_request r join approval_decision d on d.tenant_id = r.tenant_id and d.request_id = r.id \
         join app_user u on u.tenant_id = d.tenant_id and u.id = d.decider \
         where r.tenant_id = $1 and r.entity = 'Contract' and r.record_id = $2 and d.outcome = 'approve' and not d.void \
         order by r.created_at desc, d.at desc limit 1",
    )
    .bind(ctx.tenant)
    .bind(contract)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Contracts in review whose approval ended without approving them (rejected, recalled, cancelled): back to their
/// draft, saying why. An approval that completes moves the contract itself (the transition's effect).
pub(crate) async fn follow_approvals(k: &Kernel, ctx: &Ctx, tx: &mut Tx) -> Result<()> {
    let ended: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "select c.id, r.status, (select d.comment from approval_decision d where d.tenant_id = r.tenant_id and d.request_id = r.id \
           and d.outcome = 'reject' order by d.at desc limit 1) \
         from e_contract c join lateral (select * from approval_request r where r.tenant_id = c.tenant_id and r.entity = 'Contract' \
           and r.record_id = c.id order by r.created_at desc limit 1) r on true \
         where c.tenant_id = $1 and c.status = 'In Review' and r.status in ('rejected', 'recalled', 'cancelled') order by c.id for update of c",
    )
    .bind(ctx.tenant)
    .fetch_all(&mut **tx)
    .await?;
    for (contract, status, comment) in ended {
        let note = comment.unwrap_or_else(|| format!("The approval was {status}"));
        k.set_values_tx(ctx, tx, "Contract", contract, obj(json!({"status": "Draft", "decision_note": note}))).await?;
    }
    Ok(())
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
        let mut clauses = rows(doc, "clauses");
        // A new contract of a type starts with the type's clauses, in order.
        let new = match id(doc, "id") {
            Some(c) => sqlx::query_scalar::<_, i32>("select 1 from e_contract where tenant_id = $1 and id = $2")
                .bind(hc.ctx.tenant)
                .bind(c)
                .fetch_optional(&mut **hc.tx)
                .await?
                .is_none(),
            None => true,
        };
        if clauses.is_empty()
            && new
            && let Some(kind) = id(doc, "kind")
        {
            let listed: Vec<Uuid> = sqlx::query_scalar("select clause from e_contract_type__clauses where tenant_id = $1 and parent_id = $2 order by idx")
                .bind(hc.ctx.tenant)
                .bind(kind)
                .fetch_all(&mut **hc.tx)
                .await?;
            clauses = listed.into_iter().map(|c| obj(json!({"id": Uuid::now_v7().to_string(), "clause": c.to_string()}))).collect();
        }
        // Each clause's wording as it is now, its merge fields filled from the contract.
        for c in &mut clauses {
            let text: Option<String> = sqlx::query_scalar("select text from e_clause where tenant_id = $1 and id = $2")
                .bind(hc.ctx.tenant)
                .bind(id(c, "clause"))
                .fetch_optional(&mut **hc.tx)
                .await?
                .flatten();
            c.insert("text".into(), json!(text.map(|t| fill(&t, doc))));
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
            && (terms(o) != terms(doc) || s(o, "document") != s(doc, "document") || s(o, "kind") != s(doc, "kind"))
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
                let missing = missing_mandatory(hc.tx, hc.ctx, doc).await?;
                if !missing.is_empty() {
                    return Err(Error::field("clauses", format!("its type needs {}", missing.join(", "))));
                }
                // Its body, frozen as it is now: what is approved and signed.
                let mut values = Doc::new();
                let first = rows(doc, "versions").is_empty();
                let reason = if first { "Submitted" } else { "Submitted again" };
                let mut current = doc.clone();
                if let Some(body) = freeze(hc.kernel, hc.tx, &system, doc, &hc.ctx.email, reason).await? {
                    current.extend(body.clone());
                    values.extend(body);
                }
                if has_approve(hc.ctx, doc) {
                    values.insert("status".into(), json!("In Review"));
                    values.insert("decision_note".into(), Value::Null);
                    hc.kernel.set_values_tx(&system, hc.tx, "Contract", contract, values).await?;
                    // The policy asks whoever it names, or lets it through at once; the transition's effect approves it.
                    let out = hc.kernel.transition_tx(hc.ctx, hc.tx, "Contract", contract, APPROVE, None).await?;
                    let status: Option<String> = sqlx::query_scalar("select status from e_contract where tenant_id = $1 and id = $2")
                        .bind(hc.ctx.tenant)
                        .bind(contract)
                        .fetch_one(&mut **hc.tx)
                        .await?;
                    return Ok(Some(json!({"status": status, "approval": out.get("_approval")})));
                }
                // No approval policy: the threshold the app had before.
                let threshold: Option<Decimal> = sqlx::query_scalar("select approval_threshold from e_contract_settings where tenant_id = $1 limit 1")
                    .bind(hc.ctx.tenant)
                    .fetch_optional(&mut **hc.tx)
                    .await?
                    .flatten();
                if d(doc, "value") < threshold.unwrap_or(Decimal::ZERO) {
                    // Nobody compared its PDF with its clauses: the approval says so by naming no one.
                    values.insert("status".into(), json!("Approved"));
                    values.insert("decision_note".into(), json!("Below the approval threshold"));
                    values.insert("approved_digest".into(), json!(approved_digest(hc.tx, hc.ctx, &current).await?));
                } else {
                    values.insert("status".into(), json!("In Review"));
                }
                Value::Object(values)
            }
            "approval_granted" => {
                // The approval policy is satisfied (or asked nobody): bound to what it was given.
                if s(doc, "status") != Some("In Review") {
                    return Ok(Some(json!({"status": s(doc, "status")})));
                }
                let by = decided_by(hc.tx, hc.ctx, contract).await?;
                let checked = s(doc, "document").is_some() && by.is_some();
                json!({"status": "Approved", "approved_by": by, "approved_digest": approved_digest(hc.tx, hc.ctx, doc).await?,
                       "decision_note": if by.is_none() { json!("No approval step applied") } else { Value::Null },
                       "pdf_checked_by": if checked { json!(by) } else { Value::Null }})
            }
            "approve_contract" | "reject_contract" => {
                if pending_approval(hc.tx, hc.ctx, contract).await? {
                    return Err(Error::invalid("it waits on its approval policy: decide it under Approvals"));
                }
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
                           "approved_digest": approved_digest(hc.tx, hc.ctx, doc).await?, "pdf_checked_by": if checked { json!(hc.ctx.email) } else { Value::Null }})
                } else {
                    json!({"status": "Draft", "decision_note": note("note").ok_or_else(|| Error::field("note", "sending it back says why"))?})
                }
            }
            "send_for_signature" => {
                // What goes for signature is what was approved, wording and PDF (FEO-038); one approved before
                // approvals were bound has nothing to compare.
                if let Some(approved) = s(doc, "approved_digest")
                    && approved != approved_digest(hc.tx, hc.ctx, doc).await?
                {
                    return Err(Error::invalid("its wording or its PDF is not what was approved: send it back and approve it again"));
                }
                let request = crate::sign::send(hc.kernel, &system, hc.tx, doc).await?;
                json!({"status": "Out for Signature", "sign_request": request.to_string()})
            }
            "record_signed_copy" => {
                // Recording one never approved is for those who may approve: an import of contracts signed before.
                if s(doc, "status") == Some("Draft") && !approver(hc.ctx) && !hc.ctx.roles.iter().any(|r| r == "Legal") {
                    return Err(Error::Forbidden("a contract signed elsewhere is recorded from its draft by a Contract Approver or Legal".into()));
                }
                let file = note("signed_file")
                    .and_then(|p| p.rsplit('/').next().and_then(|f| f.parse::<Uuid>().ok()))
                    .ok_or_else(|| Error::field("signed_file", "is the uploaded signed copy's /files/<id> path"))?;
                let by = note("signed_by").ok_or_else(|| Error::field("signed_by", "says who signed it"))?;
                let on = note("signed_on")
                    .and_then(|d| d.get(..10).and_then(|d| d.parse::<NaiveDate>().ok()))
                    .ok_or_else(|| Error::field("signed_on", "is the day it was signed"))?;
                // A copy of the contract's own, read by whoever reads the contract; the caller must be able to read it.
                // ponytail: copy_file commits on its own; a refused action after it leaves a copy on the contract.
                let copy = hc.kernel.copy_file(hc.ctx, file, Some(("Contract", contract))).await?;
                json!({"status": "Signed", "signed_document": copy.get("url"), "signed_on": on.to_string(), "signed_elsewhere": true,
                       "signed_elsewhere_by": by, "decision_note": format!("Signed elsewhere, recorded by {}", hc.ctx.email)})
            }
            "renew_contract" => return crate::term::renew(hc, doc).await.map(Some),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_fields_fill_from_the_contract() {
        let doc = obj(json!({"counterparty_name": "Globex", "value": 5000, "notice_days": null}));
        assert_eq!(fill("Paid by {{ doc.counterparty_name }} ({{doc.value}}).", &doc), "Paid by Globex (5000).");
        assert_eq!(fill("{{ doc.notice_days }} days, {{ other }} and {{ doc.x-y }}", &doc), " days, {{ other }} and {{ doc.x-y }}");
        assert_eq!(fill("unclosed {{ doc.value", &doc), "unclosed {{ doc.value");
    }

    #[test]
    fn a_page_reads_as_its_words() {
        let html = "<style>p{color:red}</style><h1>NDA</h1><p>Between A &amp; B,</p><p>for   two years.</p>";
        assert_eq!(text_of(html), "NDA Between A & B, for two years.");
    }
}
