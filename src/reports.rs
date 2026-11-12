//! Contracts reports: contracts by state, type or owner with their value; contracts ending or reaching their
//! notice date in a period; obligations due; and the days from draft to signed. Every query carries the caller's
//! record scope on Contract (a confidential contract counts only for who may read it).

use async_trait::async_trait;
use chrono::NaiveDate;
use ferrum_domain::reports::{Params, ReportCompany, ReportSource, col, param_def, period, read_tx};
use ferrum_domain::util::money;
use ferrum_kernel::{Action, Ctx, Error, Kernel, Result, Tx, row_scope};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;
use uuid::Uuid;

pub struct ContractReports;

const GROUPS: [(&str, &str); 4] = [
    ("State", "a.status"),
    ("Type", "coalesce(t.type_name, a.contract_type, 'None')"),
    ("Owner", "coalesce(a.owner, 'None')"),
    ("Category", "coalesce(a.contract_type, 'None')"),
];

fn span(ctx: &Ctx, p: &Params<'_>) -> Result<(NaiveDate, NaiveDate)> {
    let co = ReportCompany { id: Uuid::nil(), name: String::new(), currency: String::new(), fy_month: ctx.fy_start_month, precision: 2 };
    period(p, &co)
}

/// `select <columns> from` the caller's visible contracts `a` (with their type `t`) `where` the tenant is `$1`.
fn contracts(ctx: &Ctx, columns: &str, join: &str) -> Result<QueryBuilder<Postgres>> {
    let ent = ctx.schema.entity("Contract").ok_or_else(|| Error::NotFound("Contract".into()))?;
    let scope = row_scope(ctx, ent, Action::Read)?;
    let mut qb = QueryBuilder::new(format!(
        "select {columns} from e_contract a left join e_contract_type t on t.tenant_id = a.tenant_id and t.id = a.kind {join} where a.tenant_id = "
    ));
    qb.push_bind(ctx.tenant);
    scope.push_sql(&mut qb, "a");
    Ok(qb)
}

fn dec(v: Option<Decimal>) -> Value {
    v.map_or(Value::Null, money)
}

async fn by_state(ctx: &Ctx, p: &Params<'_>, tx: &mut Tx) -> Result<Value> {
    let want = p.str("group_by").unwrap_or("State");
    let (label, expr) = GROUPS
        .iter()
        .copied()
        .find(|(l, _)| l.eq_ignore_ascii_case(want))
        .ok_or_else(|| Error::field("group_by", format!("use one of: {}", GROUPS.map(|g| g.0).join(", "))))?;
    let mut qb = contracts(ctx, &format!("{expr}, count(*), sum(coalesce(a.value, 0))"), "")?;
    qb.push(" group by 1 order by 3 desc nulls last, 1");
    let data: Vec<(String, i64, Option<Decimal>)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let (n, v) = data.iter().fold((0, Decimal::ZERO), |(n, v), r| (n + r.1, v + r.2.unwrap_or_default()));
    Ok(json!({
        "columns": [col("group", label, "data"), col("contracts", "Contracts", "int"), col("value", "Value", "currency")],
        "rows": data.into_iter().map(|(g, n, v)| json!({"group": g, "contracts": n, "value": dec(v)})).collect::<Vec<_>>(),
        "totals": {"contracts": n, "value": money(v)},
        "meta": {"group_by": label},
    }))
}

type EndingRow = (Option<String>, String, String, NaiveDate, Option<NaiveDate>, bool, String, Option<Decimal>);

async fn ending(ctx: &Ctx, p: &Params<'_>, tx: &mut Tx) -> Result<Value> {
    let (from, to) = span(ctx, p)?;
    let notice = "(a.ends_on - coalesce(a.notice_days, 30)::int)";
    let mut qb = contracts(
        ctx,
        &format!(
            "a.number, a.title, a.counterparty_name, a.ends_on, case when coalesce(a.auto_renew, false) then {notice} end, coalesce(a.auto_renew, false), a.status, a.value"
        ),
        "",
    )?;
    qb.push(" and a.status in ('Signed', 'Active') and (a.ends_on between ").push_bind(from).push(" and ").push_bind(to);
    qb.push(format!(" or (coalesce(a.auto_renew, false) and {notice} between ")).push_bind(from).push(" and ").push_bind(to).push(")) order by a.ends_on");
    let data: Vec<EndingRow> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let total = data.iter().map(|r| r.7.unwrap_or_default()).sum::<Decimal>();
    let rows: Vec<Value> = data
        .into_iter()
        .map(|(number, title, party, ends, notice, renews, status, value)| {
            json!({"number": number, "title": title, "counterparty": party, "ends_on": ends, "notice_by": notice,
                   "renews": if renews { "Renews by itself" } else { "Ends" }, "status": status, "value": dec(value)})
        })
        .collect();
    Ok(json!({
        "columns": [col("number", "Number", "data"), col("title", "Title", "data"), col("counterparty", "Counterparty", "data"),
            col("ends_on", "Ends On", "date"), col("notice_by", "Notice By", "date"), col("renews", "Renewal", "data"),
            col("status", "Status", "data"), col("value", "Value", "currency")],
        "rows": rows,
        "totals": {"value": money(total)},
        "meta": {"from_date": from, "to_date": to},
    }))
}

type ObligationRow = (String, Option<String>, Option<String>, Option<String>, NaiveDate, bool, Option<String>);

async fn obligations(ctx: &Ctx, p: &Params<'_>, tx: &mut Tx) -> Result<Value> {
    let (_, to) = span(ctx, p)?;
    let mut qb = contracts(
        ctx,
        "o.description, o.kind, o.owner, a.title, o.due_on, coalesce(o.overdue, false), a.number",
        "join e_obligation o on o.tenant_id = a.tenant_id and o.contract = a.id",
    )?;
    qb.push(" and o.status = 'Open' and o.due_on <= ").push_bind(to).push(" order by o.due_on, a.title");
    let data: Vec<ObligationRow> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let late = data.iter().filter(|r| r.5).count();
    let rows: Vec<Value> = data
        .into_iter()
        .map(|(what, kind, owner, title, due, overdue, number)| {
            json!({"obligation": what, "kind": kind, "owner": owner, "contract": title, "number": number, "due_on": due, "overdue": overdue})
        })
        .collect();
    Ok(json!({
        "columns": [col("obligation", "Obligation", "data"), col("kind", "Kind", "data"), col("owner", "Owner", "data"), col("contract", "Contract", "data"),
            col("number", "Number", "data"), col("due_on", "Due On", "date"), col("overdue", "Overdue", "check")],
        "rows": rows,
        "totals": {"overdue": late},
        "meta": {"to_date": to},
    }))
}

type CycleRow = (String, i64, Option<Decimal>, Option<Decimal>, Option<i32>, Option<i32>);

async fn cycle(ctx: &Ctx, p: &Params<'_>, tx: &mut Tx) -> Result<Value> {
    let (from, to) = span(ctx, p)?;
    let days = "(a.signed_on - (a.created_at at time zone 'UTC')::date)";
    let columns =
        format!("count(*), round(avg({days})::numeric, 1), round((percentile_cont(0.5) within group (order by {days}))::numeric, 1), min({days}), max({days})");
    let filter = |qb: &mut QueryBuilder<Postgres>| {
        qb.push(" and a.signed_on between ").push_bind(from).push(" and ").push_bind(to).push(" and not coalesce(a.signed_elsewhere, false)");
    };
    let mut grouped = contracts(ctx, &format!("to_char(a.signed_on, 'YYYY-MM'), {columns}"), "")?;
    filter(&mut grouped);
    grouped.push(" group by 1 order by 1");
    let data: Vec<CycleRow> = grouped.build_query_as().fetch_all(&mut **tx).await?;
    let mut all = contracts(ctx, &format!("'All'::text, {columns}"), "")?;
    filter(&mut all);
    let total: CycleRow = all.build_query_as().fetch_one(&mut **tx).await?;
    let row = |(month, n, avg, median, min, max): CycleRow| json!({"month": month, "signed": n, "average_days": dec(avg), "median_days": dec(median), "fastest_days": min, "slowest_days": max});
    let mut totals = row(total);
    if let Value::Object(m) = &mut totals {
        m.remove("month");
    }
    Ok(json!({
        "columns": [col("month", "Month", "data"), col("signed", "Signed", "int"), col("average_days", "Average (days)", "decimal"),
            col("median_days", "Median (days)", "decimal"), col("fastest_days", "Fastest (days)", "int"), col("slowest_days", "Slowest (days)", "int")],
        "rows": data.into_iter().map(row).collect::<Vec<_>>(),
        "totals": totals,
        "meta": {"from_date": from, "to_date": to},
    }))
}

#[async_trait]
impl ReportSource for ContractReports {
    fn catalog(&self) -> Vec<Value> {
        let entry = |name: &str, label: &str, entity: &str, params: Vec<Value>| json!({"name": name, "label": label, "module": "contracts", "entity": entity, "params": params});
        vec![
            entry(
                "contracts_by_state",
                "Contracts by State",
                "Contract",
                vec![json!({"name": "group_by", "label": "Group By", "type": "select", "options": GROUPS.map(|g| g.0), "default": "State"})],
            ),
            entry("contracts_ending", "Contracts Ending or Renewing", "Contract", vec![param_def("from_date"), param_def("to_date")]),
            entry("contract_obligations_due", "Obligations Due", "Obligation", vec![param_def("to_date")]),
            entry("contract_cycle_time", "Days from Draft to Signed", "Contract", vec![param_def("from_date"), param_def("to_date")]),
        ]
    }

    async fn run(&self, k: &Kernel, ctx: &Ctx, name: &str, params: &HashMap<String, String>) -> Result<Value> {
        let p = Params { raw: params };
        let mut tx = read_tx(k, ctx).await?;
        let out = match name {
            "contracts_by_state" => by_state(ctx, &p, &mut tx).await,
            "contracts_ending" => ending(ctx, &p, &mut tx).await,
            "contract_obligations_due" => obligations(ctx, &p, &mut tx).await,
            "contract_cycle_time" => cycle(ctx, &p, &mut tx).await,
            _ => Err(Error::NotFound(format!("report {name}"))),
        };
        tx.rollback().await?;
        out
    }
}
