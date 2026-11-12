//! Signature through Sign. An approved contract with its PDF goes to Sign as a request, to its signatories in
//! their order (equal orders sign at the same time); a contract that names none goes to the counterparty, then the
//! organisation's signatory. Each signatory's box is on the document's last page, the signature page its type's
//! template ends with: two columns of blocks, one per signatory in the order listed. Upkeep follows the request:
//! completed, the contract is signed with the sealed PDF; declined, expired or voided, it is approved again,
//! saying so.

use crate::obj;
use ferrum_domain::util::{id, rows, s};
use ferrum_kernel::{Ctx, Doc, Error, Kernel, Result, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// Signature blocks a signature page holds: two columns of six.
pub(crate) const PER_PAGE: usize = 12;

/// Where the `n`th signatory's box (from 0) sits on the signature page, in points from the page's top left, as the
/// packaged templates lay their blocks out.
pub(crate) fn place(n: usize) -> (i64, i64) {
    let x = if n.is_multiple_of(2) { 72 } else { 320 };
    (x, 120 + (n / 2) as i64 * 110 + 30)
}

/// Sends `contract` for signature and returns the Sign request's id.
pub(crate) async fn send(k: &Kernel, ctx: &Ctx, tx: &mut Tx, contract: &Doc) -> Result<Uuid> {
    let document = s(contract, "document").ok_or_else(|| {
        Error::field("document", "it left its draft without the PDF to sign, which is part of its terms: draft it again with the PDF attached")
    })?;
    // Sign signs a PDF. Where the server has no PDF service a typed contract's body is kept as a page (HTML), and
    // whatever the content type says, a file that is not a PDF is refused here, before a request is made.
    if let Some(file) = crate::contract::file_of(contract, "document") {
        let (_, _, bytes) = k.read_file(ctx, file).await?;
        if !bytes.starts_with(b"%PDF-") {
            return Err(Error::invalid(
                "its body is a page, not a PDF: this server has no PDF service (FERRUM_PDF_URL), and Sign signs PDFs only. It stays approved with its page as the document: set the service up and send it again, or sign it elsewhere and record the signed copy",
            ));
        }
    }
    let listed = rows(contract, "signatories");
    let signers: Vec<Value> = if listed.is_empty() {
        let counterparty = s(contract, "counterparty_email").ok_or_else(|| Error::field("counterparty_email", "is where the request goes"))?;
        let signatory: Option<(Option<String>, Option<String>)> =
            sqlx::query_as("select signatory_name, signatory_email from e_contract_settings where tenant_id = $1 limit 1")
                .bind(ctx.tenant)
                .fetch_optional(&mut **tx)
                .await?;
        let Some((Some(name), Some(email))) = signatory else {
            return Err(Error::invalid("name the organisation's signatory in Contract Settings first, or list the contract's signatories"));
        };
        vec![
            json!({"role": "Counterparty", "name": s(contract, "counterparty_name"), "email": counterparty, "signing_order": 1}),
            json!({"role": "Organisation", "name": name, "email": email, "signing_order": 2}),
        ]
    } else {
        // Sign tells signers apart by role: a role used twice is numbered.
        let mut seen = std::collections::HashMap::<String, usize>::new();
        listed
            .iter()
            .map(|r| {
                let role = s(r, "role").unwrap_or("Signatory").to_string();
                let n = seen.entry(role.clone()).and_modify(|n| *n += 1).or_insert(1);
                let role = if *n > 1 { format!("{role} {n}") } else { role };
                json!({"role": role, "name": s(r, "name"), "email": s(r, "email"), "signing_order": r.get("signing_order").and_then(Value::as_i64).unwrap_or(1)})
            })
            .collect()
    };
    if signers.len() > PER_PAGE {
        return Err(Error::field("signatories", format!("at most {PER_PAGE} fit the signature page")));
    }
    let mode = if signers.iter().all(|r| r["signing_order"] == signers[0]["signing_order"]) { "Parallel" } else { "Sequential" };
    // An attached PDF of a contract with no type and no signatories keeps the two boxes it always had.
    let legacy = listed.is_empty() && s(contract, "kind").is_none();
    let fields = |page: i64| -> Value {
        signers
            .iter()
            .enumerate()
            .map(|(n, r)| {
                let (x, y) = if legacy { (72, 600 + 80 * n as i64) } else { place(n) };
                json!({"role": r["role"], "field_type": "Signature", "label": format!("{} signature", r["role"].as_str().unwrap_or_default()), "required": true,
                       "page": page, "x": x, "y": y, "width": 200, "height": 50})
            })
            .collect()
    };
    let draft = k
        .insert_tx(
            ctx,
            tx,
            "SignRequest",
            obj(json!({
                "title": s(contract, "title"), "document": document, "signing_mode": mode,
                "message": match s(contract, "approved_digest") {
                    Some(d) => format!("Please sign {}, approved as {}", s(contract, "title").unwrap_or("the contract"), &d[..12.min(d.len())]),
                    None => format!("Please sign {}", s(contract, "title").unwrap_or("the contract")),
                },
                "reference_entity": "Contract", "reference_id": s(contract, "id"),
                "signers": signers,
                "fields": fields(1),
            })),
        )
        .await?;
    let request = id(&draft, "id").ok_or_else(|| Error::internal("a sign request without an id"))?;
    // The signatures go on the last page, once Sign has measured the document.
    let pages = draft.get("page_count").and_then(Value::as_i64).unwrap_or(1);
    if pages > 1 {
        k.update_tx(ctx, tx, "SignRequest", request, obj(json!({"fields": fields(pages)})), None).await?;
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
            Some("Completed") => {
                // The sealed PDF is Sign's, kept with its audit and read only by who may read that. The
                // contract keeps a copy of its own, which whoever reads the contract may open.
                let signed = match sealed.as_deref().and_then(|u| u.rsplit('/').next()).and_then(|f| f.parse::<Uuid>().ok()) {
                    Some(file) => k.copy_file(ctx, file, Some(("Contract", contract))).await?.get("url").and_then(Value::as_str).map(str::to_string),
                    None => sealed,
                };
                json!({"status": "Signed", "signed_document": signed, "signed_on": completed.map(|c| c.date_naive().to_string())})
            }
            Some(st @ ("Declined" | "Expired" | "Voided")) => {
                json!({"status": "Approved", "decision_note": format!("The signature request was {}: send it again", st.to_lowercase())})
            }
            _ => continue,
        };
        k.set_values_tx(ctx, tx, "Contract", contract, obj(values)).await?;
    }
    Ok(())
}
