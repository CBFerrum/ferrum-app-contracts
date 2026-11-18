//! Contracts and clauses. A contract is drafted from approved clauses, their wording kept as it was
//! when drafted, merge fields filled from the contract. One with a type has a body made from the type's
//! document template, frozen as a file each time it is submitted (each kept, as a version). Submitted, it goes
//! through the approval policy on its hidden action Approve It (package.yaml, `approvals`): the platform decides who
//! approves; this file binds what was approved (its digest) and follows the request's end. Once out of its draft, a
//! contract keeps its terms. Only a Contract Approver approves a clause for use.

use crate::obj;
use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use ferrum_domain::util::{d, elevated, id, rows, s};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Kernel, Result, Tx};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use uuid::Uuid;

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

/// What a contract out of its draft keeps: its terms, its clauses' own wording, and its notice days and signatories, which
/// its body prints. Apart
/// from `terms` so that the digest of a contract approved before them stays the same.
fn kept(doc: &Doc) -> Vec<Option<String>> {
    let mut t = terms(doc);
    t.push(doc.get("notice_days").and_then(Value::as_i64).map(|v| v.to_string()));
    t.extend(rows(doc, "clauses").iter().map(|c| s(c, "own_wording").map(str::trim).filter(|w| !w.is_empty()).map(str::to_string)));
    for r in rows(doc, "signatories") {
        t.extend(["name", "email", "role"].map(|k| s(&r, k).map(str::to_string)));
        t.push(r.get("signing_order").and_then(Value::as_i64).map(|v| v.to_string()));
    }
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
/// contract, never replaced (each version stays), its SHA-256, who and when, and its words. The words are taken only when
/// `stored` (the record in the database is what `doc` holds): the page is rendered from the stored record, so at a
/// renewal, whose new end is not stored yet, they would be the old body's (a renewal's version keeps no words). The
/// values to write on the contract, or none. A contract with no template whose attached PDF is not its last version's
/// gets a version of that PDF, with no words (they are inside the PDF).
pub(crate) async fn freeze(k: &Kernel, tx: &mut Tx, ctx: &Ctx, doc: &Doc, by: &str, reason: &str, stored: bool) -> Result<Option<Doc>> {
    let contract = id(doc, "id").ok_or_else(|| Error::internal("a contract without an id"))?;
    const KEPT: [&str; 8] = ["id", "version_no", "file", "sha256", "made_by", "made_at", "reason", "text"];
    let mut versions: Vec<Doc> = rows(doc, "versions").into_iter().map(|r| r.into_iter().filter(|(k, _)| KEPT.contains(&k.as_str())).collect()).collect();
    let version = versions.iter().filter_map(|v| v.get("version_no").and_then(Value::as_i64)).max().unwrap_or(0) + 1;
    let at = Utc::now().to_rfc3339();
    let Some(template) = template_of(tx, ctx, doc).await? else {
        let Some(file) = file_of(doc, "document") else { return Ok(None) };
        let sha: Option<String> = sqlx::query_scalar("select sha256 from file_object where tenant_id = $1 and id = $2")
            .bind(ctx.tenant)
            .bind(file)
            .fetch_optional(&mut **tx)
            .await?
            .flatten();
        if sha.is_some() && versions.last().and_then(|v| s(v, "sha256")) == sha.as_deref() {
            return Ok(None);
        }
        versions.push(obj(json!({"version_no": version, "file": s(doc, "document"), "sha256": sha, "made_by": by, "made_at": at, "reason": reason})));
        k.set_child_rows_tx(ctx, tx, "Contract", "versions", contract, versions).await?;
        return Ok(None);
    };
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
    let mut row = json!({"version_no": version, "file": made.url, "sha256": made.sha256, "made_by": by, "made_at": at, "reason": reason});
    let mut values = json!({"document": made.url, "document_sha256": made.sha256});
    if stored {
        // The words of the body: the page as HTML, whether the file kept is a PDF or the page itself.
        let words =
            k.render_document(ctx, &template, contract, None, false).await.map(|page| text_of(&String::from_utf8_lossy(&page.bytes))).unwrap_or_default();
        row["text"] = json!(words);
        values["body_text"] = json!(words);
    }
    versions.push(obj(row));
    k.set_child_rows_tx(ctx, tx, "Contract", "versions", contract, versions).await?;
    Ok(Some(obj(values)))
}

/// One change between two versions' words: `equal`, `delete` or `insert`, with its words.
pub(crate) type Change = (&'static str, String);

/// What changed from `a` to `b`, word by word, runs of one kind merged. A diff that takes longer than two seconds
/// stops there and gives what it found as coarser changes.
pub(crate) fn changes(a: &str, b: &str) -> Vec<Change> {
    let diff = similar::TextDiff::configure().timeout(std::time::Duration::from_secs(2)).diff_words(a, b);
    let mut out: Vec<Change> = vec![];
    for change in diff.iter_all_changes() {
        let op = match change.tag() {
            similar::ChangeTag::Equal => "equal",
            similar::ChangeTag::Delete => "delete",
            similar::ChangeTag::Insert => "insert",
        };
        match out.last_mut() {
            Some((last, text)) if *last == op => text.push_str(change.value()),
            _ => out.push((op, change.value().to_string())),
        }
    }
    out
}

/// Every character that came from the contract, safe in a page.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// The redline of `changes` between versions `from` and `to`: a page with no words of its own, what was removed struck
/// through, what was added underlined.
pub(crate) fn redline(title: &str, number: &str, from: i64, to: i64, changes: &[Change]) -> String {
    let mut out = format!(
        "<!doctype html><meta charset=utf-8><title>{} v{from} – v{to}</title><h1>{}</h1><p>v{from} → v{to}</p><p style=\"white-space:pre-wrap\">",
        esc(number),
        esc(title)
    );
    for (op, text) in changes {
        match *op {
            "delete" => out.push_str(&format!("<del>{}</del>", esc(text))),
            "insert" => out.push_str(&format!("<ins>{}</ins>", esc(text))),
            _ => out.push_str(&esc(text)),
        }
    }
    out.push_str("</p>");
    out
}

/// Compare Versions: the words removed and added between versions `from` and `to`, and the redline as a file.
fn compare(doc: &Doc, params: &Value) -> Result<Value> {
    let versions = rows(doc, "versions");
    let pick = |field: &str| -> Result<(i64, Option<String>)> {
        let n = params.get(field).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())));
        let found = n.and_then(|n| versions.iter().find(|v| v.get("version_no").and_then(Value::as_i64) == Some(n)));
        match (n, found) {
            (Some(n), Some(v)) => Ok((n, s(v, "text").filter(|t| !t.is_empty()).map(str::to_string))),
            _ => Err(Error::field(field, "is one of this contract's versions")),
        }
    };
    let ((from, a), (to, b)) = (pick("from")?, pick("to")?);
    if from == to {
        return Err(Error::field("to", "is one of this contract's versions, another than From Version"));
    }
    let words =
        |n: i64, t: Option<String>| t.ok_or_else(|| Error::invalid(format!("version {n} has no words kept: its body was an attached PDF or a renewal")));
    let (a, b) = (words(from, a)?, words(to, b)?);
    let changes = changes(&a, &b);
    let count = |op: &str| changes.iter().filter(|(o, _)| *o == op).map(|(_, t)| t.split_whitespace().count()).sum::<usize>();
    let number = s(doc, "number").unwrap_or_default();
    Ok(json!({
        "from": from, "to": to, "removed": count("delete"), "added": count("insert"),
        "changes": changes.iter().map(|(op, text)| json!({"op": op, "text": text})).collect::<Vec<_>>(),
        "file": {"name": format!("{number} v{from}-v{to}.html"), "type": "text/html", "content": redline(s(doc, "title").unwrap_or_default(), number, from, to, &changes)},
    }))
}

/// A contract type's playbook: each clause it lists with that clause's fallback and what own wording and the fallback
/// cost, and what any other clause costs.
pub(crate) struct Playbook {
    listed: Vec<(Uuid, Option<Uuid>, i64, i64)>,
    unlisted: i64,
}

/// The playbook of the contract's type; none without a type.
async fn playbook(tx: &mut Tx, ctx: &Ctx, doc: &Doc) -> Result<Option<Playbook>> {
    let Some(kind) = id(doc, "kind") else { return Ok(None) };
    let unlisted: Option<i64> = sqlx::query_scalar("select coalesce(unlisted_points, 1)::bigint from e_contract_type where tenant_id = $1 and id = $2")
        .bind(ctx.tenant)
        .bind(kind)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(unlisted) = unlisted else { return Ok(None) };
    let listed = sqlx::query_as(
        "select m.clause, c.fallback, coalesce(m.deviation_points, 1)::bigint, coalesce(m.fallback_points, 0)::bigint from e_contract_type__clauses m \
         left join e_clause c on c.tenant_id = m.tenant_id and c.id = m.clause where m.tenant_id = $1 and m.parent_id = $2 order by m.idx",
    )
    .bind(ctx.tenant)
    .bind(kind)
    .fetch_all(&mut **tx)
    .await?;
    Ok(Some(Playbook { listed, unlisted }))
}

/// How a contract's `clause` departs from its type's playbook, and what that costs: its own wording costs the listed
/// clause's points, a fallback its fallback points, a clause the type does not list the type's unlisted points.
pub(crate) fn deviation(playbook: Option<&Playbook>, clause: Option<Uuid>, own: bool) -> (&'static str, i64) {
    let Some(p) = playbook else { return ("None", 0) };
    if let Some(row) = p.listed.iter().find(|r| Some(r.0) == clause) {
        return if own { ("Own Wording", row.2) } else { ("None", 0) };
    }
    if let Some(row) = p.listed.iter().find(|r| r.1.is_some() && r.1 == clause) {
        return if own { ("Own Wording", row.2) } else { ("Fallback", row.3) };
    }
    ("Not In Type", p.unlisted)
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

/// Contracts in review whose approval ended without approving them (rejected, recalled, cancelled, or approved but
/// refused when done): back to their draft, saying why. An approval that completes approves the contract itself.
pub(crate) async fn follow_approvals(k: &Kernel, ctx: &Ctx, tx: &mut Tx) -> Result<()> {
    let ended: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "select c.id, r.status, coalesce((select d.comment from approval_decision d where d.tenant_id = r.tenant_id and d.request_id = r.id \
           and d.outcome = 'reject' order by d.at desc limit 1), r.failure) \
         from e_contract c join lateral (select * from approval_request r where r.tenant_id = c.tenant_id and r.entity = 'Contract' \
           and r.record_id = c.id order by r.created_at desc limit 1) r on true \
         where c.tenant_id = $1 and c.status = 'In Review' and r.status in ('rejected', 'recalled', 'cancelled', 'failed') order by c.id for update of c",
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
        &["owner", "clauses", "deviation_score"]
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
        // Each clause's wording as it is now, or the wording agreed for this contract, its merge fields filled from the
        // contract; the clause's version; and how far it departs from its type's playbook.
        let playbook = playbook(hc.tx, hc.ctx, doc).await?;
        let mut score = 0;
        for c in &mut clauses {
            let clause = id(c, "clause");
            let row: Option<(Option<String>, Option<i64>)> =
                sqlx::query_as("select text, wording_version::bigint from e_clause where tenant_id = $1 and id = $2")
                    .bind(hc.ctx.tenant)
                    .bind(clause)
                    .fetch_optional(&mut **hc.tx)
                    .await?;
            let (text, version) = row.unwrap_or_default();
            let own = s(c, "own_wording").map(str::trim).filter(|w| !w.is_empty()).map(str::to_string);
            let (deviation, points) = deviation(playbook.as_ref(), clause, own.is_some());
            score += points;
            c.insert("text".into(), json!(own.or(text).map(|t| fill(&t, doc))));
            c.insert("clause_version".into(), json!(version));
            c.insert("deviation".into(), json!(deviation));
            c.insert("points".into(), json!(points));
        }
        doc.insert("clauses".into(), Value::Array(clauses.into_iter().map(Value::Object).collect()));
        doc.insert("deviation_score".into(), json!(score));
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
            && (kept(o) != kept(doc) || s(o, "document") != s(doc, "document") || s(o, "kind") != s(doc, "kind"))
        {
            return Err(Error::invalid(match s(o, "status") {
                Some("Approved") => "a contract out of its draft keeps its terms: withdraw its approval to change them",
                _ => "a contract out of its draft keeps its terms",
            }));
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
                if let Some(body) = freeze(hc.kernel, hc.tx, &system, doc, &hc.ctx.email, reason, true).await? {
                    values.extend(body);
                }
                values.insert("status".into(), json!("In Review"));
                values.insert("decision_note".into(), Value::Null);
                values.insert("submitted_by".into(), json!(hc.ctx.email));
                hc.kernel.set_values_tx(&system, hc.tx, "Contract", contract, values).await?;
                // The policy asks whoever it names, or lets it through at once: Approve It then approves it here.
                let out = hc.kernel.run_action_tx(hc.ctx, hc.tx, "Contract", contract, "approve", &json!({})).await?;
                if out.get("_approval").is_none() {
                    hc.kernel.set_values_tx(&system, hc.tx, "Contract", contract, obj(json!({"decision_note": "No approval step applied"}))).await?;
                }
                let status: Option<String> = sqlx::query_scalar("select status from e_contract where tenant_id = $1 and id = $2")
                    .bind(hc.ctx.tenant)
                    .bind(contract)
                    .fetch_one(&mut **hc.tx)
                    .await?;
                return Ok(Some(json!({"status": status, "approval": out.get("_approval")})));
            }
            // The approval policy is satisfied, or asked nobody: the contract approved, bound to what it was given.
            "approve" => json!({"status": "Approved", "approved_digest": approved_digest(hc.tx, hc.ctx, doc).await?, "decision_note": null}),
            // Back to its draft, bound to nothing: changed, it is approved again only through Submit It.
            "withdraw_approval" => {
                let why = note("reason").ok_or_else(|| Error::field("reason", "a withdrawal says why"))?;
                json!({"status": "Draft", "approved_digest": null, "decision_note": format!("Approval withdrawn by {}: {why}", hc.ctx.email)})
            }
            "send_for_signature" => {
                // What goes for signature is what was approved, wording and PDF (FEO-038); one approved before
                // approvals were bound has nothing to compare.
                if let Some(approved) = s(doc, "approved_digest")
                    && approved != approved_digest(hc.tx, hc.ctx, doc).await?
                {
                    return Err(Error::invalid("its wording or its PDF is not what was approved: withdraw its approval and submit it again"));
                }
                let request = crate::sign::send(hc.kernel, hc.ctx, &system, hc.tx, doc).await?;
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
            "compare_versions" => return compare(doc, params).map(Some),
            "terminate_contract" => {
                let why = note("reason").ok_or_else(|| Error::field("reason", "a termination says why"))?;
                json!({"status": "Terminated", "terminated_on": hc.ctx.today().to_string(), "termination_reason": why, "notice_due": false})
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

    fn fills(&self) -> &'static [&'static str] {
        &["wording_version", "versions"]
    }

    /// Each new wording is kept as its next version, with who and when; the same wording saved again is not one. A
    /// clause from before versions first keeps what it said as version 1.
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let stored: Option<Option<String>> = match id(doc, "id") {
            Some(c) => {
                sqlx::query_scalar("select text from e_clause where tenant_id = $1 and id = $2")
                    .bind(hc.ctx.tenant)
                    .bind(c)
                    .fetch_optional(&mut **hc.tx)
                    .await?
            }
            None => None,
        };
        if stored.as_ref().is_some_and(|was| was.as_deref() == s(doc, "text")) {
            return Ok(());
        }
        let mut versions: Vec<Doc> = if stored.is_some() { rows(doc, "versions") } else { vec![] };
        if let Some(was) = stored
            && versions.is_empty()
        {
            versions.push(obj(json!({"id": Uuid::now_v7().to_string(), "version_no": 1, "text": was})));
        }
        let next = versions.iter().filter_map(|v| v.get("version_no").and_then(Value::as_i64)).max().unwrap_or(0) + 1;
        let by = Some(hc.ctx.email.as_str()).filter(|e| !e.is_empty());
        versions.push(obj(
            json!({"id": Uuid::now_v7().to_string(), "version_no": next, "text": s(doc, "text"), "changed_by": by, "changed_at": Utc::now().to_rfc3339()}),
        ));
        doc.insert("wording_version".into(), json!(next));
        doc.insert("versions".into(), Value::Array(versions.into_iter().map(Value::Object).collect()));
        Ok(())
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
    fn deviation_follows_the_playbook() {
        let (std, fallback, other) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let p = Playbook { listed: vec![(std, Some(fallback), 3, 2)], unlisted: 1 };
        assert_eq!(deviation(Some(&p), Some(std), false), ("None", 0));
        assert_eq!(deviation(Some(&p), Some(std), true), ("Own Wording", 3));
        assert_eq!(deviation(Some(&p), Some(fallback), false), ("Fallback", 2));
        assert_eq!(deviation(Some(&p), Some(fallback), true), ("Own Wording", 3));
        assert_eq!(deviation(Some(&p), Some(other), true), ("Not In Type", 1));
        assert_eq!(deviation(None, Some(other), true), ("None", 0));
    }

    #[test]
    fn changes_merge_runs_of_one_kind() {
        let c = changes("paid within 30 days net", "paid within 45 days net");
        let want: Vec<Change> = vec![("equal", "paid within ".into()), ("delete", "30".into()), ("insert", "45".into()), ("equal", " days net".into())];
        assert_eq!(c, want);
    }

    #[test]
    fn the_redline_escapes_what_the_contract_says() {
        let page = redline("A <b>", "CTR-1", 1, 2, &[("equal", "x & ".into()), ("delete", "<script>".into()), ("insert", "'\"".into())]);
        assert!(!page.contains("<script") && !page.contains("<b>"), "{page}");
        assert!(page.contains("<del>&lt;script&gt;</del>") && page.contains("<ins>&#39;&quot;</ins>") && page.contains("x &amp; "), "{page}");
    }

    #[test]
    fn a_page_reads_as_its_words() {
        let html = "<style>p{color:red}</style><h1>NDA</h1><p>Between A &amp; B,</p><p>for   two years.</p>";
        assert_eq!(text_of(html), "NDA Between A & B, for two years.");
    }
}
