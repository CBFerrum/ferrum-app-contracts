//! A contract's term. A signed contract is active from its start. An active one past its end renews
//! by itself for its renewal months when it says so, and expires otherwise; inside its notice
//! period before a renewal, notice is due. Obligations past their day are overdue; done, they keep
//! the day.

use crate::obj;
use async_trait::async_trait;
use chrono::{Months, NaiveDate, Utc};
use ferrum_domain::util::s;
use ferrum_kernel::{Ctx, Doc, DocHook, HookCtx, Kernel, Result, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// One workspace's contracts and obligations as of `today`.
pub(crate) async fn upkeep(k: &Kernel, ctx: &Ctx, tx: &mut Tx, today: NaiveDate) -> Result<()> {
    let starting: Vec<Uuid> = sqlx::query_scalar("select id from e_contract where tenant_id = $1 and status = 'Signed' and starts_on <= $2")
        .bind(ctx.tenant)
        .bind(today)
        .fetch_all(&mut **tx)
        .await?;
    for c in starting {
        k.set_values_tx(ctx, tx, "Contract", c, obj(json!({"status": "Active"}))).await?;
    }
    let active: Vec<(Uuid, NaiveDate, Option<bool>, Option<i64>, Option<i64>, Option<i64>)> =
        sqlx::query_as("select id, ends_on, auto_renew, renewal_months, notice_days, renewals from e_contract where tenant_id = $1 and status = 'Active'")
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
    let late: Vec<Uuid> =
        sqlx::query_scalar("select id from e_obligation where tenant_id = $1 and status = 'Open' and due_on < $2 and not coalesce(overdue, false)")
            .bind(ctx.tenant)
            .bind(today)
            .fetch_all(&mut **tx)
            .await?;
    for o in late {
        k.set_values_tx(ctx, tx, "Obligation", o, obj(json!({"overdue": true}))).await?;
    }
    Ok(())
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
