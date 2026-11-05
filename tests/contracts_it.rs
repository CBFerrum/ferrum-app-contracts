//! A year of contracts (FERRUM-ECOSYSTEM-008) against a real PostgreSQL
//! (`FERRUM_CONTRACTS_TEST_DATABASE_URL`, default `postgres://ferrum:ferrum@localhost:5432/ferrum_contracts_test`;
//! reset first), with Sign installed: contracts drafted from approved clauses only, their wording
//! kept; approved below the threshold, or by an approver who does not own them; their terms kept
//! once out of the draft; sent to Sign, the counterparty first, and followed to their signature or
//! back; active from their start, renewed or expired at their end, notice due before a renewal;
//! terminated saying why; obligations overdue and done; and who may do what.

use chrono::{Duration, Utc};
use ferrum_kernel::secret::SecretKey;
use ferrum_kernel::tenant::NewTenant;
use ferrum_kernel::{Ctx, Db, DbConfig, Doc, Error, HookRegistry, Kernel, KernelSettings};
use ferrum_meta::Catalog;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

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
        self.k.insert(ctx, entity, obj(v)).await.unwrap_or_else(|e| panic!("insert {entity}: {e:?}"))
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
    packages.extend(ferrum_app_contracts::packages().expect("app packages"));
    let catalog = Catalog::from_packages(packages).expect("catalog");
    let mut hooks = HookRegistry::default();
    ferrum_domain::register(&mut hooks);
    ferrum_app_sign::register(&mut hooks);
    ferrum_app_contracts::register(&mut hooks);
    let settings = KernelSettings { secret_key: Some(SecretKey::new([7u8; 32])), ..Default::default() };
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
            packages: vec!["contracts".into()],
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
    t.act(&both, big, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(big).await, "In Review");
    refused(t.act(&cm, big, "approve_contract", Value::Null).await, "forbidden", "a Contract Approver decides");
    refused(t.act(&both, big, "approve_contract", Value::Null).await, "forbidden", "someone other than its owner");
    refused(t.act(&ca, big, "reject_contract", Value::Null).await, "invalid", "says why");
    t.act(&ca, big, "reject_contract", json!({"note": "Cap the liability"})).await.unwrap();
    assert_eq!(
        (t.status(big).await, t.get("Contract", big).await["decision_note"].as_str().map(str::to_string)),
        ("Draft".to_string(), Some("Cap the liability".to_string()))
    );
    t.set(&ca, "Clause", liab, json!({"approved": true})).await.unwrap();
    t.set(&both, "Contract", big, json!({"clauses": [{"clause": pay.to_string()}, {"clause": liab.to_string()}]})).await.expect("a draft changes");
    t.act(&both, big, "submit_contract", Value::Null).await.unwrap();
    t.act(&ca, big, "approve_contract", Value::Null).await.expect("approved by another");
    assert_eq!(t.get("Contract", big).await["approved_by"], "ca@northwind.test");

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
    t.act(&cm, signed_one, "send_for_signature", Value::Null).await.expect("sent");
    let c = t.get("Contract", signed_one).await;
    assert_eq!(c["status"], "Out for Signature");
    let request = t.get("SignRequest", uid(&c["sign_request"])).await;
    assert_eq!(
        (request["status"].as_str(), request["reference_entity"].as_str(), request["reference_id"].as_str()),
        (Some("Sent"), Some("Contract"), c["id"].as_str())
    );
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

    // ───────────── who may do what ─────────────
    refused(t.k.insert(&ca, "Contract", obj(contract("Mine", "1", start.clone(), end.clone(), vec![]))).await, "forbidden", "");
    refused(t.k.insert(&ca, "Obligation", obj(ob("Mine", today.to_string()))).await, "forbidden", "");
}
