//! A year of contracts (FERRUM-ECOSYSTEM-008) against a real PostgreSQL
//! (`FERRUM_CONTRACTS_TEST_DATABASE_URL`, default `postgres://ferrum:ferrum@localhost:5432/ferrum_contracts_test`;
//! reset first), with Sign installed and a stand-in PDF service: contracts drafted from approved clauses only,
//! their wording kept and merge fields filled; a contract type's body frozen as a version at each submit;
//! approved through the package's approval policy (at once below the threshold, by another approver above
//! it, sent back with a reason) or the single approver when a workspace has no policy; their terms kept
//! once out of the draft; sent to Sign to their signatories in order and followed to their signature or
//! back; a copy signed elsewhere recorded; active from their start, renewed or expired at their end,
//! notice due before a renewal; reminders sent once, even by two upkeeps at once; renewed as a new
//! version or a new contract; terminated saying why; obligations overdue and done; reports; confidential
//! contracts read by three people as their roles say; and who may do what.

use chrono::{Duration, Utc};
use ferrum_kernel::secret::SecretKey;
use ferrum_kernel::tenant::NewTenant;
use ferrum_kernel::{Ctx, Db, DbConfig, Doc, Error, HookRegistry, Kernel, KernelSettings};
use ferrum_meta::Catalog;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// A PDF service standing in for Gotenberg: every page it is sent comes back as a two-page PDF.
async fn fake_pdf_renderer() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = sock.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
                let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
            let body = pdf("Body", 2);
            let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&body).await;
        }
    });
    format!("http://{addr}")
}

fn obj(v: Value) -> Doc {
    v.as_object().cloned().unwrap()
}

fn uid(v: &Value) -> Uuid {
    v.as_str().unwrap_or_else(|| panic!("not an id: {v}")).parse().unwrap()
}

/// An in-memory object store standing in for S3.
#[derive(Default)]
struct MemFiles(std::sync::Mutex<HashMap<String, Vec<u8>>>);

#[async_trait::async_trait]
impl ferrum_kernel::files::FileStore for MemFiles {
    async fn put(&self, key: &str, data: &[u8]) -> ferrum_kernel::Result<()> {
        self.0.lock().unwrap().insert(key.to_string(), data.to_vec());
        Ok(())
    }
    async fn get(&self, key: &str) -> ferrum_kernel::Result<Vec<u8>> {
        self.0.lock().unwrap().get(key).cloned().ok_or_else(|| Error::NotFound("object".into()))
    }
    async fn delete(&self, key: &str) -> ferrum_kernel::Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// A PDF of `pages` letter-size pages.
fn pdf(label: &str, pages: i64) -> Vec<u8> {
    use lopdf::{Dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = vec![];
    for i in 0..pages {
        let content = doc.add_object(Stream::new(Dictionary::new(), format!("BT 72 720 Td ({label}, page {}) Tj ET", i + 1).into_bytes()));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", pages_id);
        page.set("Contents", content);
        page.set("MediaBox", vec![Object::Integer(0), Object::Integer(0), Object::Integer(612), Object::Integer(792)]);
        kids.push(Object::Reference(doc.add_object(page)));
    }
    let mut tree = Dictionary::new();
    tree.set("Type", Object::Name(b"Pages".to_vec()));
    tree.set("Count", pages);
    tree.set("Kids", kids);
    doc.objects.insert(pages_id, Object::Dictionary(tree));
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", pages_id);
    let catalog = doc.add_object(catalog);
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

struct T {
    k: Arc<Kernel>,
    tenant: Uuid,
    admin: Ctx,
}

impl T {
    async fn insert(&self, ctx: &Ctx, entity: &str, v: Value) -> Doc {
        self.k.insert(ctx, entity, obj(v.clone())).await.unwrap_or_else(|e| panic!("insert {entity} {v}: {e:?}"))
    }
    async fn get(&self, entity: &str, id: Uuid) -> Doc {
        self.k.get(&self.admin, entity, id, false).await.unwrap_or_else(|e| panic!("get {entity}: {e:?}"))
    }
    async fn user(&self, email: &str, roles: &[&str]) -> Ctx {
        let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
        let id = self.k.create_user(&self.admin, email, email, Some("Correct-Horse-9"), &roles, false).await.expect("user");
        let s = self.k.login("legal", email, "Correct-Horse-9").await.expect("login");
        self.k.load_ctx(self.tenant, id, Some(s.token_version), "it".into()).await.expect("ctx")
    }
    async fn act(&self, ctx: &Ctx, id: Uuid, action: &str, params: Value) -> Result<Value, Error> {
        self.k.run_action(ctx, "Contract", id, action, params).await
    }
    async fn set(&self, ctx: &Ctx, entity: &str, id: Uuid, v: Value) -> Result<Doc, Error> {
        self.k.update(ctx, entity, id, obj(v), None).await
    }
    async fn status(&self, id: Uuid) -> String {
        self.get("Contract", id).await["status"].as_str().unwrap().to_string()
    }
    /// What Sign records once a request ends, as its own signing would: completed, its sealed PDF,
    /// kept with Sign's own records.
    async fn sign_ends(&self, request: Uuid, status: &str) {
        let ctx = Ctx::system(self.tenant, self.k.registry.schema(self.tenant).await.unwrap());
        let mut values = json!({"status": status});
        if status == "Completed" {
            let sealed = self.k.store_file(&ctx, Some(("SignRequest", request)), "sealed.pdf", "application/pdf", &pdf("Sealed", 3)).await.unwrap();
            values = json!({"status": status, "sealed_document": sealed["url"], "completed_at": Utc::now().to_rfc3339()});
        }
        let mut tx = self.k.db.tenant_tx(self.tenant).await.unwrap();
        self.k.set_values_tx(&ctx, &mut tx, "SignRequest", request, obj(values)).await.unwrap();
        tx.commit().await.unwrap();
    }
}

fn refused<V: std::fmt::Debug>(r: Result<V, Error>, kind: &str, says: &str) {
    let msg = format!("{r:?}");
    let ok = matches!(
        (&r, kind),
        (Err(Error::Invalid(_) | Error::Validation(_)), "invalid") | (Err(Error::Forbidden(_)), "forbidden") | (Err(Error::NotFound(_)), "not found")
    );
    assert!(ok && msg.contains(says), "expected {kind} saying {says:?}, got {msg}");
}

#[test]
fn a_year_of_contracts() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build().unwrap();
    rt.block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let url = std::env::var("FERRUM_CONTRACTS_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://ferrum:ferrum@localhost:5432/ferrum_contracts_test".into());
    let db = Db::connect(DbConfig { url, max_connections: 16, ..Default::default() }).await.expect("connect");
    sqlx::raw_sql("drop schema if exists public cascade; create schema public;").execute(&db.pool).await.expect("reset");
    let mut packages = ferrum_domain::packages().expect("platform packages");
    packages.extend(ferrum_app_sign::packages().expect("Sign's packages"));
    // Its connectors to apps not built here (CRM, ERP's selling and buying, HR) wait for those apps.
    let have: Vec<String> = packages.iter().map(|p| p.name().to_string()).collect();
    packages.extend(
        ferrum_app_contracts::packages().expect("app packages").into_iter().filter(|p| p.manifest.depends.iter().all(|d| have.contains(d) || d == "contracts")),
    );
    let catalog = Catalog::from_packages(packages).expect("catalog");
    let mut hooks = HookRegistry::default();
    ferrum_domain::register(&mut hooks);
    ferrum_app_sign::register(&mut hooks);
    ferrum_app_contracts::register(&mut hooks);
    let settings = KernelSettings { secret_key: Some(SecretKey::new([7u8; 32])), pdf_url: Some(fake_pdf_renderer().await), ..Default::default() };
    let k = Kernel::boot(db, catalog, hooks, settings, None).await.expect("boot");
    k.set_file_store(Arc::new(MemFiles::default()));
    ferrum_domain::maintenance(&k).await.expect("partitions");
    let tenant = k
        .create_tenant(NewTenant {
            slug: "legal".into(),
            name: "Northwind Legal".into(),
            admin_email: "admin@northwind.test".into(),
            admin_password: "Correct-Horse-9".into(),
            admin_name: None,
            packages: vec!["contracts".into(), "contracts_parties".into()],
            settings: Some(json!({"timezone": "UTC"})),
        })
        .await
        .expect("tenant");
    let session = k.login("legal", "admin@northwind.test", "Correct-Horse-9").await.expect("login");
    let admin = k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx");
    let t = T { k: k.clone(), tenant, admin };
    let cm = t.user("cm@northwind.test", &["Contract Manager"]).await;
    let ca = t.user("ca@northwind.test", &["Contract Approver"]).await;
    let both = t.user("lead@northwind.test", &["Contract Manager", "Contract Approver"]).await;
    let today = Utc::now().date_naive();
    t.insert(&cm, "ContractSettings", json!({"approval_threshold": "10000", "signatory_name": "Nora Chief", "signatory_email": "nora@northwind.test"})).await;
    t.insert(&t.admin, "SignSettings", json!({"sender_name": "Northwind"})).await;

    // ───────────── clauses ─────────────
    let pay = uid(&t
        .insert(&cm, "Clause", json!({"clause_name": "Payment in 30 days", "category": "Payment", "text": "Invoices are paid within 30 days."}))
        .await["id"]);
    let liab = uid(&t
        .insert(&cm, "Clause", json!({"clause_name": "Liability cap", "category": "Liability", "text": "Liability is capped at the fees paid."}))
        .await["id"]);
    refused(t.set(&cm, "Clause", pay, json!({"approved": true})).await, "forbidden", "a Contract Approver approves a clause");
    t.set(&ca, "Clause", pay, json!({"approved": true})).await.unwrap();
    refused(t.set(&cm, "Clause", pay, json!({"text": "Invoices are paid whenever."})).await, "forbidden", "changes only with a Contract Approver");

    // ───────────── drafting and approval ─────────────
    let contract = |title: &str, value: &str, from: String, to: String, clauses: Vec<Uuid>| {
        json!({"title": title, "contract_type": "Service", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "value": value,
               "starts_on": from, "ends_on": to, "clauses": clauses.iter().map(|c| json!({"clause": c.to_string()})).collect::<Vec<_>>()})
    };
    let (start, end) = (today.to_string(), (today + Duration::days(365)).to_string());
    refused(
        t.k.insert(&cm, "Contract", obj(contract("Unapproved", "5000", start.clone(), end.clone(), vec![liab]))).await,
        "invalid",
        "Liability cap is not approved for use",
    );
    refused(t.k.insert(&cm, "Contract", obj(contract("Backwards", "5000", end.clone(), start.clone(), vec![]))).await, "invalid", "after the contract starts");
    let small = t.insert(&cm, "Contract", contract("Support plan", "5000", start.clone(), end.clone(), vec![pay])).await;
    assert_eq!((small["owner"].as_str(), small["clauses"][0]["text"].as_str()), (Some("cm@northwind.test"), Some("Invoices are paid within 30 days.")));
    let small = uid(&small["id"]);
    t.act(&cm, small, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(small).await, "Approved", "below the threshold");
    refused(t.set(&cm, "Contract", small, json!({"value": "4000"})).await, "invalid", "keeps its terms");
    refused(t.set(&cm, "Contract", small, json!({"clauses": [{"clause": pay.to_string()}, {"clause": liab.to_string()}]})).await, "invalid", "keeps its terms");
    t.set(&ca, "Clause", pay, json!({"text": "Invoices are paid within 45 days."})).await.unwrap();
    t.set(&cm, "Contract", small, json!({"title": "Support plan 2026"})).await.expect("its title is not its terms");
    assert_eq!(t.get("Contract", small).await["clauses"][0]["text"], "Invoices are paid within 30 days.", "the wording as it was drafted");
    let big = uid(&t.insert(&both, "Contract", contract("Platform licence", "250000", start.clone(), end.clone(), vec![pay])).await["id"]);
    // At the threshold or more, the package's policy asks a Contract Approver other than whoever submitted it.
    let asked = t.act(&both, big, "submit_contract", Value::Null).await.unwrap();
    assert!(asked["approval"]["steps"][0]["label"].as_str().is_some(), "asked: {asked}");
    assert_eq!(t.status(big).await, "In Review");
    refused(t.act(&ca, big, "approve_contract", Value::Null).await, "invalid", "decide it under Approvals");
    refused(t.k.decide_approval(&both, "Contract", big, true, None).await, "forbidden", "");
    t.k.decide_approval(&ca, "Contract", big, false, Some("Cap the liability")).await.expect("sent back");
    ferrum_app_contracts::maintenance_on(&k, today).await.unwrap();
    assert_eq!(
        (t.status(big).await, t.get("Contract", big).await["decision_note"].as_str().map(str::to_string)),
        ("Draft".to_string(), Some("Cap the liability".to_string()))
    );
    t.set(&ca, "Clause", liab, json!({"approved": true})).await.unwrap();
    t.set(&both, "Contract", big, json!({"clauses": [{"clause": pay.to_string()}, {"clause": liab.to_string()}]})).await.expect("a draft changes");
    t.act(&both, big, "submit_contract", Value::Null).await.unwrap();
    t.k.decide_approval(&ca, "Contract", big, true, None).await.expect("approved by another");
    let approved = t.get("Contract", big).await;
    assert_eq!((approved["status"].as_str(), approved["approved_by"].as_str()), (Some("Approved"), Some("ca@northwind.test")));
    assert!(approved["approved_digest"].as_str().is_some_and(|d| d.len() == 64), "bound to what was approved: {approved:?}");
    assert!(approved["pdf_checked_by"].is_null(), "it has no PDF to read");

    // ───────────── signature through Sign ─────────────
    refused(t.act(&cm, small, "send_for_signature", Value::Null).await, "invalid", "is not available");
    let file = k.store_file(&cm, None, "support.pdf", "application/pdf", &pdf("Support plan", 3)).await.unwrap();
    let path = file["url"].as_str().unwrap().to_string();
    // The document is part of the terms: attached before it leaves its draft.
    refused(t.set(&cm, "Contract", small, json!({"document": path})).await, "invalid", "keeps its terms");
    let signed_one = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Support plan, signed", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "value": "5000",
                                        "starts_on": start, "ends_on": end, "document": path}),
        )
        .await["id"]);
    t.act(&cm, signed_one, "submit_contract", Value::Null).await.unwrap();
    // An approval is bound to the wording and the PDF it was given (FEO-038): one over the threshold is
    // approved by someone who read the PDF against the clauses, and goes for signature only as approved.
    let bound = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Licence with its PDF", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "value": "250000",
                   "starts_on": start, "ends_on": end, "document": path, "clauses": [{"clause": pay.to_string()}]}),
        )
        .await["id"]);
    t.act(&cm, bound, "submit_contract", Value::Null).await.unwrap();
    t.k.decide_approval(&ca, "Contract", bound, true, Some("The PDF says what the clauses say")).await.expect("approved, its PDF read");
    assert_eq!(t.get("Contract", bound).await["pdf_checked_by"], "ca@northwind.test");
    // Past the guards that keep its terms, straight into its tables: its wording or its PDF changed.
    let behind = |sql: &'static str, value: String| {
        let k = k.clone();
        async move {
            let mut tx = k.db.tenant_tx(tenant).await.unwrap();
            sqlx::query(sql).bind(tenant).bind(value).bind(bound).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
        }
    };
    let reword = |text: String| behind("update e_contract__clauses set text = $2 where tenant_id = $1 and parent_id = $3", text);
    let attach = |url: String| behind("update e_contract set document = $2 where tenant_id = $1 and id = $3", url);
    let drafted = t.get("Contract", bound).await["clauses"][0]["text"].as_str().unwrap().to_string();
    reword("Invoices are paid within 90 days.".into()).await;
    refused(t.act(&cm, bound, "send_for_signature", Value::Null).await, "invalid", "not what was approved");
    reword(drafted).await;
    let other = k.store_file(&cm, None, "licence.pdf", "application/pdf", &pdf("Licence, another", 2)).await.unwrap();
    attach(other["url"].as_str().unwrap().to_string()).await;
    refused(t.act(&cm, bound, "send_for_signature", Value::Null).await, "invalid", "not what was approved");
    attach(path.clone()).await;
    t.act(&cm, signed_one, "send_for_signature", Value::Null).await.expect("sent");
    let c = t.get("Contract", signed_one).await;
    assert_eq!(c["status"], "Out for Signature");
    let request = t.get("SignRequest", uid(&c["sign_request"])).await;
    assert_eq!(
        (request["status"].as_str(), request["reference_entity"].as_str(), request["reference_id"].as_str()),
        (Some("Sent"), Some("Contract"), c["id"].as_str())
    );
    let digest = c["approved_digest"].as_str().expect("approved below the threshold, bound all the same");
    assert_eq!(request["message"], format!("Please sign Support plan, signed, approved as {}", &digest[..12]), "the request names what was approved");
    let signers: Vec<(String, i64)> =
        request["signers"].as_array().unwrap().iter().map(|s| (s["email"].as_str().unwrap().to_string(), s["signing_order"].as_i64().unwrap())).collect();
    assert_eq!(signers, [("legal@globex.test".to_string(), 1), ("nora@northwind.test".to_string(), 2)], "the counterparty first, then the organisation");
    assert!(request["fields"].as_array().unwrap().iter().all(|f| f["page"] == 3), "signatures on the last page");
    ferrum_app_contracts::maintenance_on(&k, today).await.unwrap();
    assert_eq!(t.status(signed_one).await, "Out for Signature", "waiting while Sign waits");
    t.sign_ends(uid(&c["sign_request"]), "Completed").await;
    ferrum_app_contracts::maintenance_on(&k, today).await.unwrap();
    let c = t.get("Contract", signed_one).await;
    assert_eq!(c["status"], "Active", "signed, and active from its start");
    // The contract keeps a copy of the sealed PDF of its own: an approver, who may not read Sign's
    // records, opens the contract's and not Sign's.
    let file = |v: &Value| -> Uuid { v.as_str().and_then(|u| u.rsplit('/').next()).unwrap_or_else(|| panic!("not a file: {v}")).parse().unwrap() };
    let sealed = file(&t.get("SignRequest", uid(&c["sign_request"])).await["sealed_document"]);
    let copy = file(&c["signed_document"]);
    assert_ne!(copy, sealed, "a copy of its own");
    let system = Ctx::system(t.tenant, k.registry.schema(t.tenant).await.unwrap());
    assert_eq!(k.read_file(&ca, copy).await.expect("the approver opens the signed contract").2, k.read_file(&system, sealed).await.unwrap().2);
    refused(k.read_file(&ca, sealed).await, "forbidden", "signrequest");
    // A declined request puts the contract back to approved, saying so.
    let file2 = k.store_file(&cm, None, "nda.pdf", "application/pdf", &pdf("NDA", 1)).await.unwrap();
    let nda = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "NDA", "contract_type": "Non-Disclosure", "counterparty_name": "Initech", "counterparty_email": "ceo@initech.test",
                                        "starts_on": start, "ends_on": end, "document": file2["url"]}),
        )
        .await["id"]);
    t.act(&cm, nda, "submit_contract", Value::Null).await.unwrap();
    t.act(&cm, nda, "send_for_signature", Value::Null).await.unwrap();
    t.sign_ends(uid(&t.get("Contract", nda).await["sign_request"]), "Declined").await;
    ferrum_app_contracts::maintenance_on(&k, today).await.unwrap();
    let back = t.get("Contract", nda).await;
    assert_eq!((back["status"].as_str(), back["decision_note"].as_str()), (Some("Approved"), Some("The signature request was declined: send it again")));

    // ───────────── the term ─────────────
    let future = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Next year", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "document": path,
                                        "starts_on": (today + Duration::days(30)).to_string(), "ends_on": (today + Duration::days(400)).to_string()}),
        )
        .await["id"]);
    t.act(&cm, future, "submit_contract", Value::Null).await.unwrap();
    t.act(&cm, future, "send_for_signature", Value::Null).await.unwrap();
    t.sign_ends(uid(&t.get("Contract", future).await["sign_request"]), "Completed").await;
    ferrum_app_contracts::maintenance_on(&k, today).await.unwrap();
    assert_eq!(t.status(future).await, "Signed", "signed, not yet started");
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(30)).await.unwrap();
    assert_eq!(t.status(future).await, "Active");
    // Renewing by itself: notice falls due before the end, then a new term.
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_contract set auto_renew = true, renewal_months = 12, notice_days = 30 where tenant_id = $1 and id = $2")
        .bind(tenant)
        .bind(signed_one)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(300)).await.unwrap();
    assert_eq!(t.get("Contract", signed_one).await["notice_due"], false, "65 days before its end");
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(340)).await.unwrap();
    assert_eq!(t.get("Contract", signed_one).await["notice_due"], true, "25 days before its end");
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(370)).await.unwrap();
    let renewed = t.get("Contract", signed_one).await;
    let next_end = (today + Duration::days(365)).checked_add_months(chrono::Months::new(12)).unwrap().to_string();
    assert_eq!((renewed["status"].as_str(), renewed["renewals"].as_i64(), renewed["ends_on"].as_str()), (Some("Active"), Some(1), Some(next_end.as_str())));
    assert_eq!(renewed["notice_due"], false, "a new term, and its notice far off");
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(450)).await.unwrap();
    assert_eq!(t.status(future).await, "Expired", "no renewal: it ends");
    refused(t.act(&cm, signed_one, "terminate_contract", Value::Null).await, "invalid", "says why");
    t.act(&cm, signed_one, "terminate_contract", json!({"reason": "Globex was acquired"})).await.unwrap();
    assert_eq!(t.status(signed_one).await, "Terminated");

    // ───────────── obligations ─────────────
    let ob = |what: &str, due: String| json!({"contract": signed_one.to_string(), "description": what, "owner": "cm@northwind.test", "due_on": due});
    let o1 = uid(&t.insert(&cm, "Obligation", ob("Quarterly service review", (today + Duration::days(90)).to_string())).await["id"]);
    let o2 = uid(&t.insert(&cm, "Obligation", ob("Annual security report", (today + Duration::days(200)).to_string())).await["id"]);
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(100)).await.unwrap();
    assert_eq!((t.get("Obligation", o1).await["overdue"].as_bool(), t.get("Obligation", o2).await["overdue"].as_bool()), (Some(true), Some(false)));
    t.set(&cm, "Obligation", o1, json!({"status": "Done"})).await.unwrap();
    let done = t.get("Obligation", o1).await;
    assert_eq!((done["done_on"].as_str(), done["overdue"].as_bool()), (Some(today.to_string().as_str()), Some(false)));

    // ───────────── a contract type's body, frozen by version (DC-57, DC-58) ─────────────
    let lq = |f: ferrum_kernel::query::Filter| ferrum_kernel::query::ListQuery { filters: vec![f], ..Default::default() };
    let services = t.k.list(&t.admin, "ContractType", &lq(ferrum_kernel::query::Filter::eq("type_name", "Services Agreement"))).await.unwrap().rows;
    assert_eq!(services.len(), 1, "the generic types come with the package");
    let merged = uid(&t
        .insert(
            &cm,
            "Clause",
            json!({"clause_name": "Notice address", "category": "Other", "text": "Notices to {{ doc.counterparty_name }} go to {{ doc.counterparty_email }}."}),
        )
        .await["id"]);
    let short = uid(&t.insert(&cm, "Clause", json!({"clause_name": "Short cap", "category": "Liability", "text": "Capped at fees."})).await["id"]);
    t.set(&ca, "Clause", merged, json!({"approved": true})).await.unwrap();
    t.set(&ca, "Clause", short, json!({"approved": true})).await.unwrap();
    t.set(&ca, "Clause", liab, json!({"fallback": short.to_string()})).await.unwrap();
    let kind = uid(&services[0]["id"]);
    t.set(&cm, "ContractType", kind, json!({"clauses": [{"clause": merged.to_string()}, {"clause": liab.to_string(), "mandatory": true}]})).await.unwrap();
    let typed = t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Managed services", "kind": kind.to_string(), "counterparty_name": "Globex", "counterparty_email": "legal@globex.test",
                   "value": "5000", "starts_on": start, "ends_on": end, "pty_customer": null}),
        )
        .await;
    let typed_id = uid(&typed["id"]);
    let texts: Vec<&str> = typed["clauses"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect();
    assert_eq!(texts, ["Notices to Globex go to legal@globex.test.", "Liability is capped at the fees paid."], "its type's clauses, merge fields filled");
    // A mandatory clause, or its fallback, is there before it is submitted.
    t.set(&cm, "Contract", typed_id, json!({"clauses": [{"clause": merged.to_string()}]})).await.unwrap();
    refused(t.act(&cm, typed_id, "submit_contract", Value::Null).await, "invalid", "its type needs Liability cap");
    t.set(&cm, "Contract", typed_id, json!({"clauses": [{"clause": merged.to_string()}, {"clause": short.to_string()}]})).await.unwrap();
    t.act(&cm, typed_id, "submit_contract", Value::Null).await.expect("its fallback will do");
    let frozen = t.get("Contract", typed_id).await;
    assert_eq!(frozen["status"], "Approved", "below the threshold, at once");
    let v1 = frozen["versions"].as_array().unwrap().clone();
    assert_eq!((v1.len(), v1[0]["version_no"].as_i64(), v1[0]["made_by"].as_str()), (1, Some(1), Some("cm@northwind.test")));
    assert_eq!(frozen["document"], v1[0]["file"], "the body frozen is what goes for signature");
    let stored = k.read_file(&t.admin, uid(&json!(frozen["document"].as_str().unwrap().rsplit('/').next().unwrap()))).await.unwrap();
    assert!(stored.2.starts_with(b"%PDF"), "a PDF from the service");
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    let kept: String = sqlx::query_scalar("select sha256 from file_object where tenant_id = $1 and id = $2")
        .bind(tenant)
        .bind(uid(&json!(frozen["document"].as_str().unwrap().rsplit('/').next().unwrap())))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(frozen["document_sha256"].as_str(), Some(kept.as_str()), "its digest kept");
    assert!(frozen["body_text"].as_str().is_some_and(|w| w.contains("Notices to Globex go to legal@globex.test.") && w.contains("Managed services")));
    let found = t.k.list(&cm, "Contract", &ferrum_kernel::query::ListQuery { search: Some("Notices to Globex".into()), ..Default::default() }).await.unwrap();
    assert!(found.rows.iter().any(|r| r["id"] == typed["id"]), "found by the words of its body");
    // A clause changed in the library does not change a frozen contract.
    t.set(&ca, "Clause", merged, json!({"text": "Notices go anywhere."})).await.unwrap();
    assert_eq!(t.get("Contract", typed_id).await["clauses"][0]["text"], "Notices to Globex go to legal@globex.test.");

    // ───────────── parties as records, signatories in order (DC-65, DC-67) ─────────────
    let customer = uid(&t.insert(&t.admin, "Customer", json!({"customer_name": "Globex"})).await["id"]);
    let party = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Globex master agreement", "kind": kind.to_string(), "counterparty_name": "Globex", "pty_customer": customer.to_string(),
                   "starts_on": start, "ends_on": end, "clauses": [{"clause": merged.to_string()}, {"clause": liab.to_string()}],
                   "signatories": [
                       {"name": "Gina Globex", "email": "gina@globex.test", "role": "Counterparty", "signing_order": 1},
                       {"name": "Gus Globex", "email": "gus@globex.test", "role": "Counterparty", "signing_order": 1},
                       {"name": "Nora Chief", "email": "nora@northwind.test", "role": "Organisation", "signing_order": 2}]}),
        )
        .await["id"]);
    let theirs = t.k.list(&cm, "Contract", &lq(ferrum_kernel::query::Filter::eq("pty_customer", customer.to_string()))).await.unwrap().rows;
    assert_eq!(theirs.len(), 1, "contracts for this customer, by the link");
    t.act(&cm, party, "submit_contract", Value::Null).await.unwrap();
    t.act(&cm, party, "send_for_signature", Value::Null).await.expect("sent to its signatories");
    let request = t.get("SignRequest", uid(&t.get("Contract", party).await["sign_request"])).await;
    let signers: Vec<(String, String, i64)> = request["signers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["role"].as_str().unwrap().to_string(), s["email"].as_str().unwrap().to_string(), s["signing_order"].as_i64().unwrap()))
        .collect();
    assert_eq!(
        signers,
        [
            ("Counterparty".to_string(), "gina@globex.test".to_string(), 1),
            ("Counterparty 2".to_string(), "gus@globex.test".to_string(), 1),
            ("Organisation".to_string(), "nora@northwind.test".to_string(), 2)
        ]
    );
    let num = |v: &Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap() as i64;
    let boxes: Vec<(i64, i64, i64)> = request["fields"].as_array().unwrap().iter().map(|f| (num(&f["page"]), num(&f["x"]), num(&f["y"]))).collect();
    assert_eq!(boxes, [(2, 72, 150), (2, 320, 150), (2, 72, 260)], "on the signature page, the body's last, one block each");
    assert_eq!(request["signing_mode"], "Sequential");

    // ───────────── signed elsewhere (DC-66), and an import's way in (DC-70) ─────────────
    let paper = k.store_file(&both, None, "signed-on-paper.pdf", "application/pdf", &pdf("Signed on paper", 2)).await.unwrap();
    let wet = uid(&t.insert(&cm, "Contract", json!({"title": "Wet ink", "counterparty_name": "Initech", "starts_on": start, "ends_on": end})).await["id"]);
    let record = json!({"signed_file": paper["url"], "signed_by": "Bill Lumbergh for Initech; Nora Chief for us", "signed_on": today.to_string()});
    refused(t.act(&cm, wet, "record_signed_copy", record.clone()).await, "forbidden", "Contract Approver or Legal");
    t.act(&both, wet, "record_signed_copy", record.clone()).await.expect("recorded by one who may approve");
    let wet_doc = t.get("Contract", wet).await;
    assert_eq!((wet_doc["status"].as_str(), wet_doc["signed_elsewhere"].as_bool()), (Some("Signed"), Some(true)));
    assert!(wet_doc["sign_request"].is_null() && wet_doc["signed_document"].as_str().is_some(), "no Sign request: its copy");

    // ───────────── reminders, once each, even from two upkeeps at once (DC-74) ─────────────
    let soon = uid(&t
        .insert(
            &cm,
            "Obligation",
            json!({"contract": wet.to_string(), "description": "Deliver the audit report", "kind": "Milestone",
                                              "owner": "ops@northwind.test", "due_on": (today + Duration::days(10)).to_string()}),
        )
        .await["id"]);
    let sends = |record: Uuid, template: &'static str| {
        let k = k.clone();
        async move {
            let mut tx = k.db.tenant_tx(tenant).await.unwrap();
            let n: i64 = sqlx::query_scalar("select count(*) from message_send where tenant_id = $1 and record_id = $2 and template = $3")
                .bind(tenant)
                .bind(record)
                .bind(template)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            n
        }
    };
    let (a, b) = tokio::join!(ferrum_app_contracts::maintenance_on(&k, today), ferrum_app_contracts::maintenance_on(&k, today));
    a.unwrap();
    b.unwrap();
    assert_eq!(sends(soon, "contracts.obligation_due").await, 2, "its owner and the contract's, once each");
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(1)).await.unwrap();
    assert_eq!(sends(soon, "contracts.obligation_due").await, 2, "not again");
    // The contract signed on paper does not renew: its owner hears before it ends, once.
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(355)).await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(356)).await.unwrap();
    assert_eq!(sends(wet, "contracts.ending").await, 1);

    // ───────────── renewal as a new version, or as a new contract (DC-75) ─────────────
    t.set(&cm, "ContractType", kind, json!({"renewal": "New Version"})).await.unwrap();
    let wet_end = t.get("Contract", wet).await["ends_on"].as_str().unwrap().to_string();
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(400)).await.unwrap();
    assert_eq!(t.status(wet).await, "Expired", "it did not renew by itself");
    ferrum_app_contracts::maintenance_on(&k, today + Duration::days(401)).await.unwrap();
    assert_eq!(t.status(wet).await, "Expired", "expired once, and stays");
    t.act(&cm, wet, "renew_contract", Value::Null).await.expect("renewed");
    let again = t.get("Contract", wet).await;
    let next_end = wet_end.parse::<chrono::NaiveDate>().unwrap().checked_add_months(chrono::Months::new(12)).unwrap().to_string();
    assert_eq!((again["status"].as_str(), again["ends_on"].as_str(), again["renewals"].as_i64()), (Some("Active"), Some(next_end.as_str()), Some(1)));
    // A typed one renews as a new version of its body.
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_contract set status = 'Active' where tenant_id = $1 and id = $2").bind(tenant).bind(typed_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    t.act(&cm, typed_id, "renew_contract", Value::Null).await.unwrap();
    let v = t.get("Contract", typed_id).await;
    assert_eq!(v["versions"].as_array().unwrap().len(), 2, "its next term, its next version");
    assert_ne!(v["versions"][0]["file"], v["versions"][1]["file"], "each kept");
    t.set(&cm, "ContractType", kind, json!({"renewal": "New Contract"})).await.unwrap();
    let made = t.act(&cm, typed_id, "renew_contract", Value::Null).await.expect("a new contract");
    let next = t.get("Contract", uid(&made["id"])).await;
    assert_eq!((next["status"].as_str(), next["previous"].as_str()), (Some("Draft"), v["id"].as_str()));
    assert_eq!(t.get("Contract", typed_id).await["renewed_by"], made["id"]);
    refused(t.act(&cm, typed_id, "renew_contract", Value::Null).await, "invalid", "is not available");

    // ───────────── reports (DC-77) ─────────────
    let reports = ferrum_app_contracts::reports();
    let run = |name: &'static str, params: &[(&str, String)]| {
        let (k, cm, reports) = (k.clone(), cm.clone(), reports.clone());
        let params: HashMap<String, String> = params.iter().map(|(a, b)| (a.to_string(), b.clone())).collect();
        async move { reports[0].run(&k, &cm, name, &params).await.unwrap_or_else(|e| panic!("{name}: {e:?}")) }
    };
    let states = run("contracts_by_state", &[]).await;
    assert!(states["rows"].as_array().unwrap().iter().any(|r| r["group"] == "Active"), "{states}");
    let types = run("contracts_by_state", &[("group_by", "Type".into())]).await;
    assert!(types["rows"].as_array().unwrap().iter().any(|r| r["group"] == "Services Agreement"), "{types}");
    let ending = run("contracts_ending", &[("from_date", today.to_string()), ("to_date", (today + Duration::days(800)).to_string())]).await;
    assert!(ending["rows"].as_array().unwrap().iter().any(|r| r["title"] == "Wet ink"), "{ending}");
    let due = run("contract_obligations_due", &[("to_date", (today + Duration::days(30)).to_string())]).await;
    assert!(due["rows"].as_array().unwrap().iter().any(|r| r["obligation"] == "Deliver the audit report"), "{due}");
    let cycle = run("contract_cycle_time", &[("from_date", today.to_string()), ("to_date", today.to_string())]).await;
    assert_eq!(cycle["totals"]["signed"], 2, "signed through Sign today, not on paper: {cycle}");

    // ───────────── a confidential contract, read as three people (DC-71) ─────────────
    let cm2 = t.user("cm2@northwind.test", &["Contract Manager"]).await;
    let legal = t.user("counsel@northwind.test", &["Legal"]).await;
    let secret = uid(&t
        .insert(
            &cm2,
            "Contract",
            json!({"title": "Acquisition", "counterparty_name": "Initech", "confidential": true, "starts_on": start, "ends_on": end,
                                          "readers": [{"email": "lead@northwind.test"}]}),
        )
        .await["id"]);
    refused(t.k.get(&cm, "Contract", secret, false).await, "not found", "");
    assert!(t.k.get(&both, "Contract", secret, false).await.is_ok(), "a reader it lists");
    assert!(t.k.get(&legal, "Contract", secret, false).await.is_ok(), "Legal");
    assert!(t.k.get(&cm2, "Contract", secret, false).await.is_ok(), "its owner");
    let listed = t.k.list(&cm, "Contract", &ferrum_kernel::query::ListQuery::default()).await.unwrap();
    assert!(!listed.rows.iter().any(|r| r["id"] == json!(secret.to_string())), "nor in a list");

    // ───────────── who may do what ─────────────
    refused(t.k.insert(&ca, "Contract", obj(contract("Mine", "1", start.clone(), end.clone(), vec![]))).await, "forbidden", "");
    refused(t.k.insert(&ca, "Obligation", obj(ob("Mine", today.to_string()))).await, "forbidden", "");

    // ───────────── a workspace with no approval policy: the single approver it had before ─────────────
    let none: ferrum_meta::OverlayOp = serde_json::from_value(json!({"op": "set_workflow", "workflow": {
        "name": "Contract Steps", "entity": "Contract", "initial_state": "Open",
        "states": [{"name": "Open"}, {"name": "Filed"}], "transitions": [{"action": "File", "from": "Open", "to": "Filed"}]}}))
    .unwrap();
    k.add_customization(&t.admin, "Contract", none, None).await.expect("a workflow without Approve");
    let cm = t.user("cm3@northwind.test", &["Contract Manager"]).await;
    let ca = t.user("ca3@northwind.test", &["Contract Approver"]).await;
    let old_way = uid(&t.insert(&cm, "Contract", contract("Old way", "250000", start.clone(), end.clone(), vec![pay])).await["id"]);
    t.act(&cm, old_way, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(old_way).await, "In Review");
    refused(t.act(&cm, old_way, "approve_contract", Value::Null).await, "forbidden", "a Contract Approver decides");
    t.act(&ca, old_way, "approve_contract", Value::Null).await.expect("the single approver");
    assert_eq!(
        (t.status(old_way).await, t.get("Contract", old_way).await["approved_by"].as_str().map(str::to_string)),
        ("Approved".to_string(), Some("ca3@northwind.test".to_string()))
    );
}
