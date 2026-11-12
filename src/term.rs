//! A contract's term. A signed contract is active from its start. An active one past its end renews
//! by itself for its renewal months when it says so, and expires otherwise; inside its notice
//! period before a renewal, notice is due. Obligations past their day are overdue; done, they keep
//! the day. Their owners are reminded before an obligation is due, before the last day to give notice
//! and before a contract that does not renew ends, once each, with the package's message templates.
//! Renew It makes the next term: on the same contract as a new version, or as a new contract linked to it,
//! as its type says.
//!
//! Each step selects its rows `for update` and writes what it did in the same transaction, so two upkeeps
//! at once never move a contract or send a reminder twice: the second waits on the row, then finds it done.

use crate::obj;
use async_trait::async_trait;
use chrono::{Months, NaiveDate, Utc};
use ferrum_domain::util::{elevated, id, rows, s};
use ferrum_kernel::messages::{Message, Recipient};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Kernel, Result, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// One workspace's contracts and obligations as of `today`.
pub(crate) async fn upkeep(k: &Kernel, ctx: &Ctx, tx: &mut Tx, today: NaiveDate) -> Result<()> {
    let starting: Vec<Uuid> =
        sqlx::query_scalar("select id from e_contract where tenant_id = $1 and status = 'Signed' and starts_on <= $2 order by id for update")
            .bind(ctx.tenant)
            .bind(today)
            .fetch_all(&mut **tx)
            .await?;
    for c in starting {
        k.set_values_tx(ctx, tx, "Contract", c, obj(json!({"status": "Active"}))).await?;
    }
    let active: Vec<(Uuid, NaiveDate, Option<bool>, Option<i64>, Option<i64>, Option<i64>)> = sqlx::query_as(
        "select id, ends_on, auto_renew, renewal_months, notice_days, renewals from e_contract where tenant_id = $1 and status = 'Active' order by id for update",
    )
    .bind(ctx.tenant)
    .fetch_all(&mut **tx)
    .await?;
    for (c, mut ends, renew, months, notice, renewals) in active {
        let renew = renew.unwrap_or(false);
        let mut values = serde_json::Map::new();
        if ends < today {
            if !renew {
                values.insert("status".into(), json!("Expired"));
                values.insert("notice_due".into(), json!(false));
                k.set_values_tx(ctx, tx, "Contract", c, values).await?;
                continue;
            }
            // Renewed for as many terms as it takes to reach today.
            let mut count = renewals.unwrap_or(0);
            while ends < today {
                ends = ends.checked_add_months(Months::new(months.unwrap_or(12).max(1) as u32)).unwrap_or(ends);
                count += 1;
            }
            values.insert("ends_on".into(), json!(ends.to_string()));
            values.insert("renewals".into(), json!(count));
        }
        let due = renew && today >= ends - chrono::Duration::days(notice.unwrap_or(30).max(0));
        values.insert("notice_due".into(), json!(due));
        k.set_values_tx(ctx, tx, "Contract", c, values).await?;
    }
    let late: Vec<Uuid> = sqlx::query_scalar(
        "select id from e_obligation where tenant_id = $1 and status = 'Open' and due_on < $2 and not coalesce(overdue, false) order by id for update",
    )
    .bind(ctx.tenant)
    .bind(today)
    .fetch_all(&mut **tx)
    .await?;
    for o in late {
        k.set_values_tx(ctx, tx, "Obligation", o, obj(json!({"overdue": true}))).await?;
    }
    remind(k, ctx, tx, today).await
}

/// Reminders, each once: an open obligation `reminder_days` before it is due (to its owner and its contract's),
/// an active contract that renews by itself before its last day to give notice, and one that does not before it
/// ends (to its owner). The date reminded for is kept, so a new due date or a new term is reminded again.
async fn remind(k: &Kernel, ctx: &Ctx, tx: &mut Tx, today: NaiveDate) -> Result<()> {
    let days: i64 = sqlx::query_scalar::<_, Option<i64>>("select reminder_days::bigint from e_contract_settings where tenant_id = $1 limit 1")
        .bind(ctx.tenant)
        .fetch_optional(&mut **tx)
        .await?
        .flatten()
        .unwrap_or(14);
    if days <= 0 {
        return Ok(());
    }
    let horizon = today + chrono::Duration::days(days);
    let obligations: Vec<(Uuid, NaiveDate, Option<String>, Option<String>)> = sqlx::query_as(
        "select o.id, o.due_on, o.owner, c.owner from e_obligation o left join e_contract c on c.tenant_id = o.tenant_id and c.id = o.contract \
         where o.tenant_id = $1 and o.status = 'Open' and o.due_on <= $2 and o.reminded_for is distinct from o.due_on order by o.id for update of o",
    )
    .bind(ctx.tenant)
    .bind(horizon)
    .fetch_all(&mut **tx)
    .await?;
    for (o, due, owner, contract_owner) in obligations {
        send(k, ctx, tx, "contracts.obligation_due", "Obligation", o, [owner, contract_owner]).await?;
        k.set_values_tx(ctx, tx, "Obligation", o, obj(json!({"reminded_for": due.to_string()}))).await?;
    }
    // Before the last day to give notice (renewing by itself), or before the end (not renewing).
    let contracts: Vec<(Uuid, NaiveDate, bool, Option<String>)> = sqlx::query_as(
        "select id, ends_on, coalesce(auto_renew, false), owner from e_contract where tenant_id = $1 and status = 'Active' and ( \
           (coalesce(auto_renew, false) and ends_on - coalesce(notice_days, 30)::int <= $2 and notice_reminded_for is distinct from ends_on) \
           or (not coalesce(auto_renew, false) and ends_on <= $2 and end_reminded_for is distinct from ends_on)) order by id for update",
    )
    .bind(ctx.tenant)
    .bind(horizon)
    .fetch_all(&mut **tx)
    .await?;
    for (c, ends, renews, owner) in contracts {
        let (template, marker) = if renews { ("contracts.renewal_notice", "notice_reminded_for") } else { ("contracts.ending", "end_reminded_for") };
        send(k, ctx, tx, template, "Contract", c, [owner, None]).await?;
        k.set_values_tx(ctx, tx, "Contract", c, obj(json!({marker: ends.to_string()}))).await?;
    }
    Ok(())
}

async fn send(k: &Kernel, ctx: &Ctx, tx: &mut Tx, template: &str, entity: &str, record: Uuid, to: [Option<String>; 2]) -> Result<()> {
    let mut people: Vec<String> = to.into_iter().flatten().map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()).collect();
    people.dedup();
    if people.is_empty() {
        return Ok(());
    }
    let m = Message {
        template: template.into(),
        entity: Some(entity.into()),
        record: Some(record),
        to: people.iter().map(|e| Recipient::address(e)).collect(),
        ..Default::default()
    };
    ferrum_kernel::messages::send(k, tx, ctx, &m).await?;
    Ok(())
}

/// Renew It: the next term, as the contract's type says (a new version when it has none).
/// * New Version: the same contract, its end moved on by its renewal months from its end, active again if it had
///   expired, its body made again and kept as the next version.
/// * New Contract: a draft starting the day after it ends, with its terms, parties, signatories and clauses, linked
///   both ways; it goes through approval and signature as any contract.
pub(crate) async fn renew(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<Value> {
    let contract = id(doc, "id").ok_or_else(|| Error::internal("a contract without an id"))?;
    if s(doc, "renewed_by").is_some() {
        return Err(Error::invalid("it was renewed already"));
    }
    let system = elevated(hc.ctx);
    let how: Option<String> = match id(doc, "kind") {
        Some(kind) => sqlx::query_scalar("select renewal from e_contract_type where tenant_id = $1 and id = $2")
            .bind(hc.ctx.tenant)
            .bind(kind)
            .fetch_optional(&mut **hc.tx)
            .await?
            .flatten(),
        None => None,
    };
    let ends: NaiveDate = s(doc, "ends_on").and_then(|d| d.parse().ok()).ok_or_else(|| Error::field("ends_on", "is needed to renew"))?;
    let months = Months::new(doc.get("renewal_months").and_then(Value::as_i64).unwrap_or(12).max(1) as u32);
    let next_end = ends.checked_add_months(months).ok_or_else(|| Error::field("renewal_months", "is too far"))?;
    if how.as_deref() == Some("New Contract") {
        let starts = ends + chrono::Duration::days(1);
        let lines = |table: &str, keep: &[&str]| -> Vec<Value> {
            rows(doc, table).iter().map(|r| Value::Object(keep.iter().filter_map(|k| r.get(*k).map(|v| (k.to_string(), v.clone()))).collect())).collect()
        };
        let mut next = obj(json!({
            "title": s(doc, "title"), "kind": s(doc, "kind"), "contract_type": s(doc, "contract_type"),
            "counterparty_name": s(doc, "counterparty_name"), "counterparty_email": s(doc, "counterparty_email"),
            "department": s(doc, "department"), "confidential": doc.get("confidential"), "value": doc.get("value"),
            "starts_on": starts.to_string(), "ends_on": starts.checked_add_months(months).unwrap_or(next_end).to_string(),
            "auto_renew": doc.get("auto_renew"), "renewal_months": doc.get("renewal_months"), "notice_days": doc.get("notice_days"),
            "signatories": lines("signatories", &["name", "email", "role", "signing_order"]),
            "readers": lines("readers", &["email"]), "clauses": lines("clauses", &["clause"]),
        }));
        // The links a connector package adds (a customer, a deal) go with it.
        for (k, v) in doc.iter().filter(|(k, _)| ["pty_", "prj_", "crm_", "sel_", "buy_", "hr_"].iter().any(|p| k.starts_with(p))) {
            next.insert(k.clone(), v.clone());
        }
        let made = hc.kernel.insert_tx(hc.ctx, hc.tx, "Contract", next).await?;
        let new_id = id(&made, "id").ok_or_else(|| Error::internal("a contract without an id"))?;
        hc.kernel.set_values_tx(&system, hc.tx, "Contract", new_id, obj(json!({"previous": contract.to_string()}))).await?;
        hc.kernel.set_values_tx(&system, hc.tx, "Contract", contract, obj(json!({"renewed_by": new_id.to_string()}))).await?;
        return Ok(json!({"status": s(doc, "status"), "entity": "Contract", "id": new_id}));
    }
    let mut values = obj(json!({
        "ends_on": next_end.to_string(), "renewals": doc.get("renewals").and_then(Value::as_i64).unwrap_or(0) + 1,
        "status": "Active", "notice_due": false,
    }));
    hc.kernel.set_values_tx(&system, hc.tx, "Contract", contract, values.clone()).await?;
    let mut renewed = doc.clone();
    renewed.extend(values.clone());
    if let Some(body) = crate::contract::freeze(hc.kernel, hc.tx, &system, &renewed, &hc.ctx.email, &format!("Renewed to {next_end}")).await? {
        hc.kernel.set_values_tx(&system, hc.tx, "Contract", contract, body.clone()).await?;
        values.extend(body);
    }
    Ok(json!({"status": "Active", "ends_on": values["ends_on"]}))
}

pub struct ObligationHook;

#[async_trait]
impl DocHook for ObligationHook {
    fn name(&self) -> &'static str {
        "contracts.obligation"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["done_on", "overdue"]
    }

    async fn before_validate(&self, _hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "status") == Some("Done") {
            if doc.get("done_on").is_none_or(Value::is_null) {
                doc.insert("done_on".into(), json!(Utc::now().date_naive().to_string()));
            }
            doc.insert("overdue".into(), json!(false));
        } else {
            doc.insert("done_on".into(), Value::Null);
        }
        Ok(())
    }
}
