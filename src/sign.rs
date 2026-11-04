//! Signature through Sign. An approved contract with its PDF and the counterparty's address goes to
//! Sign as a request: the counterparty signs first, then the organisation's signatory, each with a
//! signature field on the document's last page. Upkeep follows the request: completed, the contract is signed
//! with the sealed PDF; declined, expired or voided, it is approved again, saying so.

use crate::obj;
use ferrum_domain::util::{id, s};
use ferrum_kernel::{Ctx, Doc, Error, Kernel, Result, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// Sends `contract` for signature and returns the Sign request's id.
pub(crate) async fn send(k: &Kernel, ctx: &Ctx, tx: &mut Tx, contract: &Doc) -> Result<Uuid> {
    let document = s(contract, "document").ok_or_else(|| Error::field("document", "attach the PDF to sign first"))?;
    let counterparty = s(contract, "counterparty_email").ok_or_else(|| Error::field("counterparty_email", "is where the request goes"))?;
    let signatory: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("select signatory_name, signatory_email from e_contract_settings where tenant_id = $1 limit 1")
            .bind(ctx.tenant)
            .fetch_optional(&mut **tx)
            .await?;
    let Some((Some(name), Some(email))) = signatory else {
        return Err(Error::invalid("name the organisation's signatory in Contract Settings first"));
    };
    let field = |role: &str, y: i64, page: i64| json!({"role": role, "field_type": "Signature", "label": format!("{role} signature"), "required": true, "page": page, "x": 72, "y": y, "width": 200, "height": 50});
    let draft = k
        .insert_tx(
            ctx,
            tx,
            "SignRequest",
            obj(json!({
                "title": s(contract, "title"), "document": document, "signing_mode": "Sequential",
                "message": format!("Please sign {}", s(contract, "title").unwrap_or("the contract")),
                "reference_entity": "Contract", "reference_id": s(contract, "id"),
                "signers": [
                    {"role": "Counterparty", "name": s(contract, "counterparty_name"), "email": counterparty, "signing_order": 1},
                    {"role": "Organisation", "name": name, "email": email, "signing_order": 2}],
                "fields": [field("Counterparty", 600, 1), field("Organisation", 680, 1)],
            })),
        )
        .await?;
    let request = id(&draft, "id").ok_or_else(|| Error::internal("a sign request without an id"))?;
    // The signatures go on the last page, once Sign has measured the document.
    let pages = draft.get("page_count").and_then(Value::as_i64).unwrap_or(1);
    if pages > 1 {
        let fields = json!([field("Counterparty", 600, pages), field("Organisation", 680, pages)]);
        k.update_tx(ctx, tx, "SignRequest", request, obj(json!({"fields": fields})), None).await?;
    }
    k.run_action_tx(ctx, tx, "SignRequest", request, "send", &json!({})).await?;
    Ok(request)
}

/// Contracts out for signature, moved on as their Sign requests have.
pub(crate) async fn follow(k: &Kernel, ctx: &Ctx, tx: &mut Tx) -> Result<()> {
    let out: Vec<(Uuid, Uuid)> =
        sqlx::query_as("select id, sign_request from e_contract where tenant_id = $1 and status = 'Out for Signature' and sign_request is not null")
            .bind(ctx.tenant)
            .fetch_all(&mut **tx)
            .await?;
    for (contract, request) in out {
        let row: Option<(Option<String>, Option<String>, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as("select status, sealed_document, completed_at from e_sign_request where tenant_id = $1 and id = $2")
                .bind(ctx.tenant)
                .bind(request)
                .fetch_optional(&mut **tx)
                .await?;
        let Some((status, sealed, completed)) = row else { continue };
        let values: Value = match status.as_deref() {
            Some("Completed") => json!({"status": "Signed", "signed_document": sealed, "signed_on": completed.map(|c| c.date_naive().to_string())}),
            Some(st @ ("Declined" | "Expired" | "Voided")) => {
                json!({"status": "Approved", "decision_note": format!("The signature request was {}: send it again", st.to_lowercase())})
            }
            _ => continue,
        };
        k.set_values_tx(ctx, tx, "Contract", contract, obj(values)).await?;
    }
    Ok(())
}
