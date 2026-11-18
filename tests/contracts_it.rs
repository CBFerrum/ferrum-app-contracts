//! A year of contracts (FERRUM-ECOSYSTEM-008) against a real PostgreSQL
//! (`FERRUM_CONTRACTS_TEST_DATABASE_URL`, default `postgres://ferrum:ferrum@localhost:5432/ferrum_contracts_test`;
//! reset first), with Sign installed and a stand-in PDF service: contracts drafted from approved clauses only,
//! their wording kept and merge fields filled; a contract type's body frozen as a version at each submit;
//! approved through the package's approval policy (at once below the threshold, by another approver above
//! it, sent back with a reason), the contract held as it is meanwhile; their terms kept once out of the draft; sent to Sign to their signatories in order and followed to their signature or
//! back; a copy signed elsewhere recorded; active from their start, renewed or expired at their end,
//! notice due before a renewal; reminders sent once, even by two upkeeps at once; renewed as a new
//! version or a new contract; terminated saying why; obligations overdue and done; reports; confidential
//! contracts read by three people as their roles say; and who may do what.

use chrono::{Datelike, Duration, Utc};
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

/// What opens the test's vault: the database password of `url`, with Argon2id kept cheap for tests.
fn opener(url: &str) -> ferrum_kernel::vault::Opener {
    ferrum_kernel::vault::Opener {
        database_url: url.to_string(),
        file: "/nonexistent/root.key".into(),
        kdf: ferrum_kernel::vault::Kdf { m_kib: 64, t: 1, p: 1 },
        ..Default::default()
    }
}

#[test]
fn a_year_of_contracts() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build().unwrap();
    rt.block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let url = std::env::var("FERRUM_CONTRACTS_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://ferrum:ferrum@localhost:5432/ferrum_contracts_test".into());
    let db = Db::connect(DbConfig { url: url.clone(), max_connections: 16, ..Default::default() }).await.expect("connect");
    sqlx::raw_sql("drop schema if exists vault cascade; drop schema if exists public cascade; create schema public;").execute(&db.pool).await.expect("reset");
    // The vault, opened as a server opens it, before the kernel boots.
    ferrum_kernel::vault::open(&db, opener(&url), false).await.expect("the vault");
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
    let sign_packages: Vec<String> = ferrum_app_sign::packages().unwrap().iter().map(|p| p.name().to_string()).collect();
    hooks.app("sign", &sign_packages, ferrum_app_sign::register);
    hooks.own("sign", &sign_packages);
    // As the server registers it: its own vault handle, which proves its calls of Sign's operations are its.
    let names: Vec<String> = ferrum_app_contracts::packages().unwrap().iter().map(|p| p.name().to_string()).collect();
    hooks.app("contracts", &names, ferrum_app_contracts::register);
    hooks.own("contracts", &names);
    let settings = KernelSettings { pdf_url: Some(fake_pdf_renderer().await), ..Default::default() };
    let k = Kernel::boot(db, catalog, hooks, settings.clone(), None).await.expect("boot");
    let files = Arc::new(MemFiles::default());
    k.set_file_store(files.clone());
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
    // A clause's wording is kept by version, with who changed it and when.
    let worded = t.insert(&cm, "Clause", json!({"clause_name": "Governing words", "text": "Words in force."})).await;
    let first = (worded.get("wording_version").and_then(Value::as_i64), worded.get("versions").and_then(|v| v[0]["changed_by"].as_str()));
    assert_eq!(first, (Some(1), Some("cm@northwind.test")), "a clause's wording is kept by version: {worded:?}");
    let worded = uid(&worded["id"]);
    t.set(&ca, "Clause", worded, json!({"text": "Words in force, as amended."})).await.unwrap();
    t.set(&ca, "Clause", worded, json!({"text": "Words in force, as amended."})).await.unwrap();
    let w = t.get("Clause", worded).await;
    let kept: Vec<(i64, String)> =
        w["versions"].as_array().unwrap().iter().map(|v| (v["version_no"].as_i64().unwrap(), v["text"].as_str().unwrap().to_string())).collect();
    assert_eq!(w["wording_version"].as_i64(), Some(2), "saved with the same words, still 2");
    assert_eq!(kept, [(1, "Words in force.".to_string()), (2, "Words in force, as amended.".to_string())]);
    assert_eq!(w["versions"][1]["changed_by"], "ca@northwind.test");
    // A clause from before versions keeps what it said.
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("delete from e_clause__versions where tenant_id = $1 and parent_id = $2").bind(tenant).bind(worded).execute(&mut *tx).await.unwrap();
    sqlx::query("update e_clause set wording_version = null where tenant_id = $1 and id = $2").bind(tenant).bind(worded).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    t.set(&ca, "Clause", worded, json!({"text": "Words in force, once more."})).await.unwrap();
    let w = t.get("Clause", worded).await;
    let kept: Vec<(i64, String, Value)> = w["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["version_no"].as_i64().unwrap(), v["text"].as_str().unwrap().to_string(), v["changed_by"].clone()))
        .collect();
    assert_eq!(kept, [(1, "Words in force, as amended.".to_string(), Value::Null), (2, "Words in force, once more.".to_string(), json!("ca@northwind.test"))]);

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
    refused(t.act(&ca, small, "submit_contract", Value::Null).await, "invalid", "a Contract Manager or Legal asks");
    t.act(&cm, small, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(small).await, "Approved", "below the threshold");
    refused(t.set(&cm, "Contract", small, json!({"value": "4000"})).await, "invalid", "keeps its terms");
    refused(t.set(&cm, "Contract", small, json!({"clauses": [{"clause": pay.to_string()}, {"clause": liab.to_string()}]})).await, "invalid", "keeps its terms");
    // Its notice days and its signatories are printed in its body: kept as its terms are, until its approval is withdrawn.
    refused(t.set(&cm, "Contract", small, json!({"notice_days": 5})).await, "invalid", "keeps its terms");
    let signer = json!({"signatories": [{"name": "Gina Globex", "email": "gina@globex.test", "role": "Counterparty", "signing_order": 1}]});
    refused(t.set(&cm, "Contract", small, signer.clone()).await, "invalid", "keeps its terms");
    let nobody = t.user("nobody@northwind.test", &[]).await;
    refused(t.act(&nobody, small, "withdraw_approval", json!({"reason": "Wrong signatory"})).await, "forbidden", "");
    refused(t.act(&cm, small, "withdraw_approval", Value::Null).await, "invalid", "says why");
    t.act(&cm, small, "withdraw_approval", json!({"reason": "Wrong signatory"})).await.expect("withdrawn");
    let withdrawn = t.get("Contract", small).await;
    assert_eq!(
        (withdrawn["status"].as_str(), withdrawn["approved_digest"].as_str(), withdrawn["decision_note"].as_str()),
        (Some("Draft"), None, Some("Approval withdrawn by cm@northwind.test: Wrong signatory"))
    );
    refused(t.act(&cm, small, "withdraw_approval", json!({"reason": "Again"})).await, "invalid", "is not available");
    t.set(&cm, "Contract", small, signer).await.expect("a draft's signatories change");
    t.act(&cm, small, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(small).await, "Approved", "approved again, through Submit It");
    t.set(&ca, "Clause", pay, json!({"text": "Invoices are paid within 45 days."})).await.unwrap();
    t.set(&cm, "Contract", small, json!({"title": "Support plan 2026"})).await.expect("its title is not its terms");
    assert_eq!(t.get("Contract", small).await["clauses"][0]["text"], "Invoices are paid within 30 days.", "the wording as it was drafted");
    let big = uid(&t.insert(&both, "Contract", contract("Platform licence", "250000", start.clone(), end.clone(), vec![pay])).await["id"]);
    // At the threshold or more, the package's policy asks a Contract Approver other than whoever submitted it.
    let asked = t.act(&both, big, "submit_contract", Value::Null).await.unwrap();
    assert!(asked["approval"]["steps"][0]["label"].as_str().is_some(), "asked: {asked}");
    assert_eq!(t.status(big).await, "In Review");
    // The policy is the only way to Approved: running its action again asks nothing new and approves nothing.
    assert!(t.act(&both, big, "approve", Value::Null).await.is_err(), "the approval is the policy's");
    assert_eq!(t.status(big).await, "In Review");
    // While it waits, the contract stays as it is (the policy's lock), its title too.
    let held = t.set(&both, "Contract", big, json!({"title": "Platform licence, renamed"})).await;
    assert!(held.as_ref().is_err_and(|e| e.to_string().contains("stays as it is")), "{held:?}");
    refused(t.k.decide_approval(&both, "Contract", big, true, None).await, "forbidden", "");
    t.k.decide_approval(&ca, "Contract", big, false, Some("Cap the liability")).await.expect("sent back");
    // Asked again directly before upkeep, by an approver who does not draft contracts: refused, as through Submit It.
    refused(t.act(&ca, big, "approve", Value::Null).await, "invalid", "a Contract Manager or Legal asks");
    // Asked again directly before upkeep by another Contract Manager, in their own name, whoever submitted it could decide
    // it: only whoever submitted it asks again.
    refused(t.act(&cm, big, "approve", Value::Null).await, "invalid", "only by whoever submitted it");
    assert_eq!(t.status(big).await, "In Review", "nothing asked, nothing approved");
    ferrum_app_contracts::maintenance_on(&k, tenant, today).await.unwrap();
    assert_eq!(
        (t.status(big).await, t.get("Contract", big).await["decision_note"].as_str().map(str::to_string)),
        ("Draft".to_string(), Some("Cap the liability".to_string()))
    );
    t.set(&ca, "Clause", liab, json!({"approved": true})).await.unwrap();
    t.set(&both, "Contract", big, json!({"clauses": [{"clause": pay.to_string()}, {"clause": liab.to_string()}]})).await.expect("a draft changes");
    t.act(&both, big, "submit_contract", Value::Null).await.unwrap();
    t.k.decide_approval(&ca, "Contract", big, true, None).await.expect("approved by another");
    let approved = t.get("Contract", big).await;
    assert_eq!(approved["status"].as_str(), Some("Approved"));
    assert!(approved["approved_digest"].as_str().is_some_and(|d| d.len() == 64), "bound to what was approved: {approved:?}");
    // Who approved it is the platform's record of the request.
    let record = t.k.approval_of(&t.admin, "Contract", big).await.unwrap();
    assert_eq!((record["status"].as_str(), record["decisions"][0]["decider"].clone()), (Some("approved"), json!(ca.user.to_string())), "{record}");

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
    // No PDF service: a body kept as a page is approved and kept, but cannot go for signature, and nothing is left half done.
    let page = k.store_file(&cm, None, "body.html", "application/octet-stream", b"<style>@page{size:A4}</style><h1>Terms</h1>").await.unwrap();
    let paged = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Terms as a page", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "value": "5000",
                   "starts_on": start, "ends_on": end, "document": page["url"]}),
        )
        .await["id"]);
    t.act(&cm, paged, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.get("Contract", paged).await["status"], "Approved", "approving and keeping the page still works");
    refused(t.act(&cm, paged, "send_for_signature", Value::Null).await, "invalid", "FERRUM_PDF_URL");
    let after = t.get("Contract", paged).await;
    assert!(after["status"] == "Approved" && after["sign_request"].is_null(), "it stays approved, no request");
    let orphans =
        t.k.list(&cm, "SignRequest", &ferrum_kernel::query::ListQuery { search: Some("Terms as a page".into()), ..Default::default() }).await.unwrap();
    assert!(orphans.rows.is_empty(), "no orphan request");
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
    let record = t.k.approval_of(&t.admin, "Contract", bound).await.unwrap();
    assert_eq!(
        (record["decisions"][0]["decider"].clone(), record["decisions"][0]["comment"].as_str()),
        (json!(ca.user.to_string()), Some("The PDF says what the clauses say")),
        "{record}"
    );
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
    // An attached PDF is a version at each submit: its file and SHA-256, no words (they are inside the PDF).
    let sha_of = |url: &Value| {
        let (k, id) = (k.clone(), uid(&json!(url.as_str().unwrap().rsplit('/').next().unwrap())));
        async move {
            let mut tx = k.db.tenant_tx(tenant).await.unwrap();
            let sha: String =
                sqlx::query_scalar("select sha256 from file_object where tenant_id = $1 and id = $2").bind(tenant).bind(id).fetch_one(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            sha
        }
    };
    let attached = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Attached PDF", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "starts_on": start, "ends_on": end,
                   "document": path}),
        )
        .await["id"]);
    t.act(&cm, attached, "submit_contract", Value::Null).await.unwrap();
    let versions = t.get("Contract", attached).await["versions"].as_array().unwrap().clone();
    assert_eq!(versions.len(), 1, "an attached PDF is a version: {versions:?}");
    assert_eq!((versions[0]["file"].as_str(), versions[0]["sha256"].as_str()), (Some(path.as_str()), Some(sha_of(&json!(path)).await.as_str())));
    assert!(versions[0]["text"].is_null(), "no words of a PDF");
    // The same PDF submitted again is not a new version; another PDF is.
    let resubmit = |c: Uuid, reason: &'static str| {
        let (k, cm) = (k.clone(), cm.clone());
        async move {
            k.run_action(&cm, "Contract", c, "withdraw_approval", json!({"reason": reason})).await.expect("withdrawn");
            k.run_action(&cm, "Contract", c, "submit_contract", Value::Null).await.expect("submitted again");
        }
    };
    resubmit(attached, "Checked again").await;
    assert_eq!(t.get("Contract", attached).await["versions"].as_array().unwrap().len(), 1, "the same PDF is not a new version");
    t.act(&cm, attached, "withdraw_approval", json!({"reason": "Another PDF"})).await.unwrap();
    t.set(&cm, "Contract", attached, json!({"document": other["url"]})).await.expect("a draft's PDF changes");
    t.act(&cm, attached, "submit_contract", Value::Null).await.unwrap();
    let versions = t.get("Contract", attached).await["versions"].as_array().unwrap().clone();
    assert_eq!((versions.len(), versions[1]["file"].clone()), (2, other["url"].clone()), "another PDF, another version");
    refused(t.act(&cm, attached, "compare_versions", json!({"from": 1, "to": 2})).await, "invalid", "no words kept");
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
    // Made through Sign's operation at the platform's gate: its receipt names Contracts, and the person it acted for.
    let receipt: Option<(Option<String>, Option<Uuid>)> = {
        let mut tx = k.db.tenant_tx(tenant).await.unwrap();
        let r = sqlx::query_as("select caller, actor from logic_effect where tenant_id = $1 and capability = 'sign.sign_request.create' and effect_key = $2")
            .bind(tenant)
            .bind(format!("contracts:sign:{signed_one}:first"))
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert_eq!(receipt, Some((Some("contracts".to_string()), Some(cm.user))), "a send is recorded as Contracts' call");
    ferrum_app_contracts::maintenance_on(&k, tenant, today).await.unwrap();
    assert_eq!(t.status(signed_one).await, "Out for Signature", "waiting while Sign waits");
    t.sign_ends(uid(&c["sign_request"]), "Completed").await;
    ferrum_app_contracts::maintenance_on(&k, tenant, today).await.unwrap();
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
    ferrum_app_contracts::maintenance_on(&k, tenant, today).await.unwrap();
    let back = t.get("Contract", nda).await;
    assert_eq!((back["status"].as_str(), back["decision_note"].as_str()), (Some("Approved"), Some("The signature request was declined: send it again")));
    // Sent again, it is a new request, which the contract names.
    let first = uid(&back["sign_request"]);
    t.act(&cm, nda, "send_for_signature", Value::Null).await.expect("sent again");
    let again = uid(&t.get("Contract", nda).await["sign_request"]);
    assert_ne!(again, first, "a second request");
    assert_eq!(t.get("SignRequest", again).await["status"], "Sent");
    assert_eq!(t.get("SignRequest", first).await["status"], "Declined", "the first one stays as it ended");

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
    ferrum_app_contracts::maintenance_on(&k, tenant, today).await.unwrap();
    assert_eq!(t.status(future).await, "Signed", "signed, not yet started");
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(30)).await.unwrap();
    assert_eq!(t.status(future).await, "Active");
    // Renewing by itself: notice falls due before the end, then a new term. Its notice days are its terms: a person's
    // change is refused, and the 30 days it was signed with stand (notice due 25 days before its end, below).
    refused(t.set(&cm, "Contract", signed_one, json!({"notice_days": 5})).await, "invalid", "keeps its terms");
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_contract set auto_renew = true, renewal_months = 12, notice_days = 30 where tenant_id = $1 and id = $2")
        .bind(tenant)
        .bind(signed_one)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(300)).await.unwrap();
    assert_eq!(t.get("Contract", signed_one).await["notice_due"], false, "65 days before its end");
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(340)).await.unwrap();
    assert_eq!(t.get("Contract", signed_one).await["notice_due"], true, "25 days before its end");
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(370)).await.unwrap();
    let renewed = t.get("Contract", signed_one).await;
    let next_end = (today + Duration::days(365)).checked_add_months(chrono::Months::new(12)).unwrap().to_string();
    assert_eq!((renewed["status"].as_str(), renewed["renewals"].as_i64(), renewed["ends_on"].as_str()), (Some("Active"), Some(1), Some(next_end.as_str())));
    assert_eq!(renewed["notice_due"], false, "a new term, and its notice far off");
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(450)).await.unwrap();
    assert_eq!(t.status(future).await, "Expired", "no renewal: it ends");
    refused(t.act(&cm, signed_one, "terminate_contract", Value::Null).await, "invalid", "says why");
    t.act(&cm, signed_one, "terminate_contract", json!({"reason": "Globex was acquired"})).await.unwrap();
    assert_eq!(t.status(signed_one).await, "Terminated");

    // ───────────── obligations ─────────────
    let ob = |what: &str, due: String| json!({"contract": signed_one.to_string(), "description": what, "owner": "cm@northwind.test", "due_on": due});
    let o1 = uid(&t.insert(&cm, "Obligation", ob("Quarterly service review", (today + Duration::days(90)).to_string())).await["id"]);
    let o2 = uid(&t.insert(&cm, "Obligation", ob("Annual security report", (today + Duration::days(200)).to_string())).await["id"]);
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(100)).await.unwrap();
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
    assert!(v1[0]["text"].as_str().is_some_and(|w| w.contains("Notices to Globex go to legal@globex.test.")), "a version keeps its words: {:?}", v1[0]);
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

    // ───────────── versions keep their words, and compare (CTR-02, CTR-03) ─────────────
    t.set(&ca, "Clause", pay, json!({"text": "Invoices are paid within 30 days."})).await.unwrap();
    // Clauses outside the type count nothing here: this compares versions, the playbook is below.
    t.set(&cm, "ContractType", kind, json!({"unlisted_points": 0})).await.unwrap();
    let revised = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Revised terms", "kind": kind.to_string(), "counterparty_name": "Globex", "counterparty_email": "legal@globex.test",
                   "value": "5000", "starts_on": start, "ends_on": end, "confidential": true,
                   "clauses": [{"clause": pay.to_string()}, {"clause": short.to_string()}]}),
        )
        .await["id"]);
    t.act(&cm, revised, "submit_contract", Value::Null).await.unwrap();
    refused(t.act(&cm, revised, "compare_versions", json!({"from": 1, "to": 2})).await, "invalid", "is not available");
    t.act(&cm, revised, "withdraw_approval", json!({"reason": "New payment terms"})).await.unwrap();
    t.set(&ca, "Clause", pay, json!({"text": "Invoices are paid within 45 days."})).await.unwrap();
    let scripted =
        uid(&t.insert(&cm, "Clause", json!({"clause_name": "Scripted", "category": "Other", "text": "<script>alert(1)</script> applies."})).await["id"]);
    t.set(&ca, "Clause", scripted, json!({"approved": true})).await.unwrap();
    t.set(&cm, "Contract", revised, json!({"clauses": [{"clause": pay.to_string()}, {"clause": short.to_string()}, {"clause": scripted.to_string()}]}))
        .await
        .expect("a draft takes the library's wording");
    t.act(&cm, revised, "submit_contract", Value::Null).await.unwrap();
    let r = t.get("Contract", revised).await;
    let texts: Vec<String> = r["versions"].as_array().unwrap().iter().map(|v| v["text"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(texts.len(), 2);
    assert!(texts[0].contains("within 30 days") && texts[1].contains("within 45 days"), "earlier versions keep their words: {texts:?}");
    let before = r["version"].clone();
    let compared = t.act(&cm, revised, "compare_versions", json!({"from": 1, "to": 2})).await.expect("compared");
    let changes = compared["changes"].as_array().unwrap();
    assert!(changes.iter().any(|c| c["op"] == "delete" && c["text"].as_str().unwrap().contains("30")), "{compared}");
    assert!(changes.iter().any(|c| c["op"] == "insert" && c["text"].as_str().unwrap().contains("45")), "{compared}");
    assert!(compared["removed"].as_i64() >= Some(1) && compared["added"].as_i64() >= Some(1), "{compared}");
    let redline = compared["file"]["content"].as_str().unwrap();
    assert!(!redline.contains("<script") && redline.contains("&lt;script&gt;"), "the redline escapes what the contract says");
    assert_eq!((compared["file"]["type"].as_str(), compared["from"].as_i64(), compared["to"].as_i64()), (Some("text/html"), Some(1), Some(2)));
    assert_eq!(t.get("Contract", revised).await["version"], before, "compare writes nothing");
    refused(t.act(&cm, revised, "compare_versions", json!({"from": 2, "to": 2})).await, "invalid", "one of this contract's versions");
    refused(t.act(&cm, revised, "compare_versions", json!({"from": 1, "to": 9})).await, "invalid", "one of this contract's versions");
    let other_cm = t.user("cm-other@northwind.test", &["Contract Manager"]).await;
    refused(t.act(&other_cm, revised, "compare_versions", json!({"from": 1, "to": 2})).await, "not found", "");

    // ───────────── a type's playbook and the deviation score (CTR-01) ─────────────
    let clause = |name: &str, text: &str, fallback: Option<Uuid>| {
        let (t, cm, ca) = (&t, cm.clone(), ca.clone());
        let v = json!({"clause_name": name, "category": "Other", "text": text, "fallback": fallback.map(|f| f.to_string())});
        async move {
            let id = uid(&t.insert(&cm, "Clause", v).await["id"]);
            t.set(&ca, "Clause", id, json!({"approved": true})).await.unwrap();
            id
        }
    };
    let pb_fb = clause("Short warranty", "Warranted for 6 months.", None).await;
    let pb_std = clause("Warranty", "Warranted for 12 months.", Some(pb_fb)).await;
    let pb_extra = clause("Audit right", "Either side may audit once a year.", None).await;
    let pb = uid(&t
        .insert(
            &cm,
            "ContractType",
            json!({"type_name": "Playbook Agreement", "template": "contracts.services_agreement", "unlisted_points": 1,
                   "clauses": [{"clause": pb_std.to_string(), "deviation_points": 3, "fallback_points": 2}]}),
        )
        .await["id"]);
    let draft = |title: &str, kind: Option<Uuid>, clauses: Value| {
        json!({"title": title, "kind": kind.map(|k| k.to_string()), "counterparty_name": "Globex", "counterparty_email": "legal@globex.test",
               "value": "500", "starts_on": start, "ends_on": end, "clauses": clauses})
    };
    // Own wording is what is printed and counted.
    let own = t
        .insert(
            &cm,
            "Contract",
            draft("Own words", Some(pb), json!([{"clause": pb_std.to_string(), "own_wording": "Warranted for {{ doc.counterparty_name }}'s 24 months."}])),
        )
        .await;
    assert_eq!(
        (own["clauses"][0]["text"].as_str(), own["clauses"][0]["deviation"].as_str(), own["clauses"][0]["points"].as_i64(), own["deviation_score"].as_i64()),
        (Some("Warranted for Globex's 24 months."), Some("Own Wording"), Some(3), Some(3)),
        "{own:?}"
    );
    assert_eq!(own["clauses"][0]["clause_version"].as_i64(), Some(1), "the version it was drafted from");
    let own = uid(&own["id"]);
    // A fallback and a clause outside the type are counted; a contract with no type scores nothing.
    let mixed = t.insert(&cm, "Contract", draft("Fallback and more", Some(pb), json!([{"clause": pb_fb.to_string()}, {"clause": pb_extra.to_string()}]))).await;
    let marks: Vec<(String, i64)> =
        mixed["clauses"].as_array().unwrap().iter().map(|c| (c["deviation"].as_str().unwrap().to_string(), c["points"].as_i64().unwrap())).collect();
    assert_eq!(marks, [("Fallback".to_string(), 2), ("Not In Type".to_string(), 1)]);
    assert_eq!(mixed["deviation_score"].as_i64(), Some(3));
    let untyped = t.insert(&cm, "Contract", draft("No type", None, json!([{"clause": pb_extra.to_string()}]))).await;
    assert_eq!((untyped["deviation_score"].as_i64(), untyped["clauses"][0]["deviation"].as_str()), (Some(0), Some("None")));
    // Nobody holds Legal yet: a deviating contract cannot be asked of anyone, and nothing of the submit is left.
    assert!(t.act(&cm, own, "submit_contract", Value::Null).await.is_err(), "nobody to ask");
    let still = t.get("Contract", own).await;
    assert!(still["status"] == "Draft" && still["versions"].as_array().unwrap().is_empty(), "{still:?}");
    // A deviating contract goes to Legal first, however little it is worth; one that keeps to the playbook does not.
    let lawyer = t.user("lawyer@northwind.test", &["Legal"]).await;
    let asked = t.act(&cm, own, "submit_contract", Value::Null).await.unwrap();
    let steps: Vec<&str> = asked["approval"]["steps"].as_array().into_iter().flatten().filter_map(|s| s["label"].as_str()).collect();
    assert!(steps.contains(&"Legal"), "a deviating contract goes to Legal first: {asked}");
    assert_eq!(t.status(own).await, "In Review");
    t.k.decide_approval(&lawyer, "Contract", own, true, Some("Agreed")).await.expect("Legal approves");
    assert_eq!(t.status(own).await, "Approved");
    let plain = uid(&t.insert(&cm, "Contract", draft("By the playbook", Some(pb), json!([{"clause": pb_std.to_string()}]))).await["id"]);
    t.act(&cm, plain, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(plain).await, "Approved", "score 0, approved at once");
    // Own wording is kept once out of the draft.
    refused(
        t.set(&cm, "Contract", own, json!({"clauses": [{"clause": pb_std.to_string(), "own_wording": "Warranted for ever."}]})).await,
        "invalid",
        "keeps its terms",
    );
    // A contract remembers the version it used.
    t.set(&ca, "Clause", pb_std, json!({"text": "Warranted for 18 months."})).await.unwrap();
    assert_eq!(t.get("Contract", own).await["clauses"][0]["clause_version"].as_i64(), Some(1));
    // Contracts made before the score are scored zero.
    let before = uid(&t.insert(&cm, "Contract", draft("Made before", None, json!([]))).await["id"]);
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_contract set deviation_score = null where tenant_id = $1 and id = $2").bind(tenant).bind(before).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    // The package's backfill, as the platform's job runs it (its `set` where its `needs`).
    let package = ferrum_app_contracts::packages().unwrap().into_iter().find(|p| p.manifest.package == "contracts").unwrap();
    let fill = package.manifest.backfills.iter().find(|b| b.name == "contract_deviation_zero").expect("the backfill");
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    let sql = format!("update e_contract t set {} where t.tenant_id = $1 and ({})", fill.set, fill.needs);
    sqlx::query(sqlx::AssertSqlSafe(sql)).bind(tenant).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(t.get("Contract", before).await["deviation_score"].as_i64(), Some(0));
    t.act(&cm, before, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(before).await, "Approved");
    // Who may set the playbook: not a Contract Approver.
    refused(t.set(&ca, "ContractType", pb, json!({"unlisted_points": 0})).await, "forbidden", "");

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
    let (a, b) = tokio::join!(ferrum_app_contracts::maintenance_on(&k, tenant, today), ferrum_app_contracts::maintenance_on(&k, tenant, today));
    a.unwrap();
    b.unwrap();
    assert_eq!(sends(soon, "contracts.obligation_due").await, 2, "its owner and the contract's, once each");
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(1)).await.unwrap();
    assert_eq!(sends(soon, "contracts.obligation_due").await, 2, "not again");
    // The contract signed on paper does not renew: its owner hears before it ends, once.
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(355)).await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(356)).await.unwrap();
    assert_eq!(sends(wet, "contracts.ending").await, 1);

    // ───────────── renewal as a new version, or as a new contract (DC-75) ─────────────
    t.set(&cm, "ContractType", kind, json!({"renewal": "New Version"})).await.unwrap();
    let wet_end = t.get("Contract", wet).await["ends_on"].as_str().unwrap().to_string();
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(400)).await.unwrap();
    assert_eq!(t.status(wet).await, "Expired", "it did not renew by itself");
    ferrum_app_contracts::maintenance_on(&k, tenant, today + Duration::days(401)).await.unwrap();
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
    assert!(v["versions"][0]["text"].as_str().is_some() && v["versions"][1]["text"].is_null(), "a renewal's version keeps no stale words");
    assert_eq!(v["body_text"], frozen["body_text"], "its words stay those of the body last submitted");
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

    // ───────────── two upkeeps at once keep one signed copy ─────────────
    let mut out = vec![];
    for n in 0..5 {
        let c = uid(&t
            .insert(
                &cm,
                "Contract",
                json!({"title": format!("Batch {n}"), "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "document": path,
                       "starts_on": start, "ends_on": end}),
            )
            .await["id"]);
        t.act(&cm, c, "submit_contract", Value::Null).await.unwrap();
        t.act(&cm, c, "send_for_signature", Value::Null).await.unwrap();
        let request = uid(&t.get("Contract", c).await["sign_request"]);
        t.sign_ends(request, "Completed").await;
        out.push((c, file(&t.get("SignRequest", request).await["sealed_document"])));
    }
    let (a, b) = tokio::join!(ferrum_app_contracts::maintenance_on(&k, tenant, today), ferrum_app_contracts::maintenance_on(&k, tenant, today));
    for (c, sealed) in out {
        let mut tx = k.db.tenant_tx(tenant).await.unwrap();
        let copies: i64 = sqlx::query_scalar(
            "select count(*) from file_object f where f.tenant_id = $1 and f.entity = 'Contract' and f.record_id = $2 \
             and f.sha256 = (select s.sha256 from file_object s where s.tenant_id = $1 and s.id = $3)",
        )
        .bind(tenant)
        .bind(c)
        .bind(sealed)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(copies, 1, "one copy of the sealed PDF on each contract");
        assert_eq!(t.status(c).await, "Active");
    }
    a.unwrap();
    b.unwrap();

    // ───────────── the workspace's day, not UTC's ─────────────
    // No clock can be set: as the platform's own suites do, the workspace moves to a zone whose day is not UTC's now
    // (UTC-11 before 10:00 UTC, UTC+14 after), and what the app dates or decides "today" is held to the workspace's day.
    use chrono::Timelike;
    let zone = if Utc::now().hour() < 10 { "Pacific/Pago_Pago" } else { "Pacific/Kiritimati" };
    t.k.update_tenant_settings(&t.admin, &obj(json!({"timezone": zone}))).await.expect("the workspace's zone");
    let cm = t.k.load_ctx(tenant, cm.user, None, "it".into()).await.expect("ctx");
    let day = cm.today();
    assert_ne!(day, Utc::now().date_naive(), "{zone}'s day is not UTC's");
    t.act(&cm, wet, "terminate_contract", json!({"reason": "Moved in house"})).await.unwrap();
    assert_eq!(t.get("Contract", wet).await["terminated_on"], json!(day.to_string()), "terminated on the workspace's day");
    let late = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Signed late", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "document": path,
                   "starts_on": day.to_string(), "ends_on": (day + Duration::days(365)).to_string()}),
        )
        .await["id"]);
    t.act(&cm, late, "submit_contract", Value::Null).await.unwrap();
    t.act(&cm, late, "send_for_signature", Value::Null).await.unwrap();
    t.sign_ends(uid(&t.get("Contract", late).await["sign_request"]), "Completed").await;
    ferrum_app_contracts::maintenance(&k, tenant).await.unwrap();
    assert_eq!(t.get("Contract", late).await["signed_on"], json!(day.to_string()), "signed on the workspace's day of Sign's moment");
    let finished = uid(&t.insert(&cm, "Obligation", ob("Kick-off held", day.to_string())).await["id"]);
    t.set(&cm, "Obligation", finished, json!({"status": "Done"})).await.unwrap();
    assert_eq!(t.get("Obligation", finished).await["done_on"], json!(day.to_string()), "done on the workspace's day");
    // Upkeep's day is each workspace's own: due yesterday there is overdue, due today there is not.
    let yesterday = uid(&t.insert(&cm, "Obligation", ob("Due yesterday", (day - Duration::days(1)).to_string())).await["id"]);
    let due_today = uid(&t.insert(&cm, "Obligation", ob("Due today", day.to_string())).await["id"]);
    ferrum_app_contracts::maintenance(&k, tenant).await.unwrap();
    assert_eq!(
        (t.get("Obligation", yesterday).await["overdue"].as_bool(), t.get("Obligation", due_today).await["overdue"].as_bool()),
        (Some(true), Some(false)),
        "overdue as of the workspace's day"
    );

    // ───────────── a workspace's own policy on Terminate It and on Send for Signature (CTR-15) ─────────────
    let policy = |action: &str, approval: Value| -> ferrum_kernel::customize::NewCustomization {
        serde_json::from_value(json!({"entity": "Contract", "op": {"op": "set_approval", "on": {"action": action}, "approval": approval}})).unwrap()
    };
    let legal_step = json!({"steps": [{"label": "Legal", "approvers": {"role": "Legal"}}]});
    t.k.batch_customizations(&t.admin, vec![policy("terminate_contract", legal_step.clone())], false)
        .await
        .map_err(|(_, e)| e)
        .expect("a policy on Terminate It");
    // A person's context carries the workspace's schema as it was when loaded: loaded again, it has the policy.
    let fresh = |c: &Ctx| {
        let (k, user) = (k.clone(), c.user);
        async move { k.load_ctx(tenant, user, None, "it".into()).await.expect("ctx") }
    };
    let (cm, legal) = (fresh(&cm).await, fresh(&legal).await);
    // A workspace's policy on Terminate It asks first.
    assert_eq!(t.status(late).await, "Active");
    let asked = t.act(&cm, late, "terminate_contract", json!({"reason": "Supplier exit"})).await.unwrap();
    assert!(asked["_approval"].is_object(), "it waits for Legal: {asked}");
    assert_eq!(t.status(late).await, "Active", "nothing done while it waits");
    t.k.decide_approval(&legal, "Contract", late, true, None).await.expect("Legal approves");
    let ended = t.get("Contract", late).await;
    assert_eq!((ended["status"].as_str(), ended["termination_reason"].as_str()), (Some("Terminated"), Some("Supplier exit")), "with the reason asked");
    // A workspace's policy on Send for Signature sends once it is approved, once.
    t.k.batch_customizations(&t.admin, vec![policy("send_for_signature", legal_step)], false)
        .await
        .map_err(|(_, e)| e)
        .expect("a policy on Send for Signature");
    let (cm, legal) = (fresh(&cm).await, fresh(&legal).await);
    let held = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Held for Legal", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "document": path,
                   "starts_on": day.to_string(), "ends_on": (day + Duration::days(365)).to_string()}),
        )
        .await["id"]);
    t.act(&cm, held, "submit_contract", Value::Null).await.unwrap();
    let asked = t.act(&cm, held, "send_for_signature", Value::Null).await.unwrap();
    assert!(asked["_approval"].is_object(), "it waits for Legal: {asked}");
    let requests = |contract: Uuid| {
        let k = k.clone();
        async move {
            let mut tx = k.db.tenant_tx(tenant).await.unwrap();
            let n: i64 = sqlx::query_scalar("select count(*) from e_sign_request where tenant_id = $1 and reference_id = $2")
                .bind(tenant)
                .bind(contract.to_string())
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            n
        }
    };
    assert_eq!((t.status(held).await, requests(held).await), ("Approved".to_string(), 0), "nothing sent while it waits");
    t.k.decide_approval(&legal, "Contract", held, true, None).await.expect("Legal approves");
    assert_eq!((t.status(held).await, requests(held).await), ("Out for Signature".to_string(), 1), "sent once it is approved, once");
    t.k.batch_customizations(&t.admin, vec![policy("send_for_signature", Value::Null), policy("terminate_contract", Value::Null)], false)
        .await
        .map_err(|(_, e)| e)
        .expect("the policies taken away");
    let cm = fresh(&cm).await;

    // ───────────── a workspace whose Contracts does not declare Sign's operations ─────────────
    // The same database, served by a build whose Contracts package uses nothing: the gate refuses its call.
    let mut packages = ferrum_domain::packages().expect("platform packages");
    packages.extend(ferrum_app_sign::packages().expect("Sign's packages"));
    let have: Vec<String> = packages.iter().map(|p| p.name().to_string()).collect();
    packages.extend(
        ferrum_app_contracts::packages().unwrap().into_iter().filter(|p| p.manifest.depends.iter().all(|d| have.contains(d) || d == "contracts")).map(
            |mut p| {
                p.manifest.uses.clear();
                p
            },
        ),
    );
    let mut hooks = HookRegistry::default();
    ferrum_domain::register(&mut hooks);
    hooks.app("sign", &sign_packages, ferrum_app_sign::register);
    hooks.own("sign", &sign_packages);
    hooks.app("contracts", &names, ferrum_app_contracts::register);
    hooks.own("contracts", &names);
    let k2 = Kernel::boot(k.db.clone(), Catalog::from_packages(packages).expect("catalog"), hooks, settings, None).await.expect("boot without the uses");
    k2.set_file_store(files);
    let unused = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "No use declared", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "document": path,
                   "starts_on": day.to_string(), "ends_on": (day + Duration::days(365)).to_string()}),
        )
        .await["id"]);
    t.act(&cm, unused, "submit_contract", Value::Null).await.unwrap();
    let cm2k = k2.load_ctx(tenant, cm.user, None, "it".into()).await.expect("ctx");
    refused(k2.run_action(&cm2k, "Contract", unused, "send_for_signature", Value::Null).await, "forbidden", "does not use sign.sign_request.create");
    let after = t.get("Contract", unused).await;
    assert!(after["status"] == "Approved" && after["sign_request"].is_null(), "it stays approved, no request");
    assert_eq!(requests(unused).await, 0, "nothing left of the refused call");

    // ───────────── requests from a form: the queue, Take It, Withdraw It (CTR-05) ─────────────
    // A form's response arrives as the system makes it: a draft with who asked and no owner.
    let system = Ctx::system(tenant, k.registry.schema(tenant).await.unwrap());
    let asked = |title: &str, confidential: bool| {
        json!({"title": title, "counterparty_name": "Initech", "starts_on": day.to_string(), "ends_on": (day + Duration::days(365)).to_string(),
               "requested_by": "asker@outside.test", "request_note": "We need an NDA before the demo", "confidential": confidential})
    };
    let sent = |record: Uuid, template: &'static str| {
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
            tx.commit().await.unwrap();
            n
        }
    };
    let request = t.insert(&system, "Contract", asked("NDA for the demo", false)).await;
    assert!(request["owner"].is_null(), "a request arrives without an owner: {request:?}");
    let request = uid(&request["id"]);
    // Legal and the Contract Managers are told, each once: cm, lead, cm-other, cm2, lawyer, counsel.
    assert_eq!(sent(request, "contracts.request_received").await, 6, "Legal and the Contract Managers are told");
    // One person takes it.
    let lead = k.load_ctx(tenant, both.user, None, "it".into()).await.unwrap();
    let taken = t.act(&cm, request, "take_request", Value::Null).await.expect("taken");
    assert_eq!(taken["owner"], "cm@northwind.test");
    refused(t.act(&lead, request, "take_request", Value::Null).await, "invalid", "is not available");
    assert_eq!(t.get("Contract", request).await["owner"], "cm@northwind.test");
    assert_eq!(sent(request, "contracts.request_taken").await, 1, "whoever asked is told, once");
    // A contract a person drafts is not a request.
    let drafted = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Drafted here", "counterparty_name": "Initech", "starts_on": day.to_string(),
                                                         "ends_on": (day + Duration::days(365)).to_string()}),
        )
        .await["id"]);
    assert_eq!(sent(drafted, "contracts.request_received").await, 0);
    refused(t.act(&cm, drafted, "take_request", Value::Null).await, "invalid", "is not available");
    // A draft is withdrawn saying why; nothing leads out of Withdrawn.
    let unwanted = uid(&t.insert(&system, "Contract", asked("Not needed after all", false)).await["id"]);
    refused(t.act(&cm, unwanted, "withdraw_contract", Value::Null).await, "invalid", "says why");
    t.act(&cm, unwanted, "withdraw_contract", json!({"reason": "The demo was cancelled"})).await.expect("withdrawn");
    let gone = t.get("Contract", unwanted).await;
    assert_eq!(
        (gone["status"].as_str(), gone["withdrawn_on"].as_str(), gone["withdrawal_reason"].as_str()),
        (Some("Withdrawn"), Some(day.to_string().as_str()), Some("The demo was cancelled"))
    );
    assert_eq!(sent(unwanted, "contracts.request_withdrawn").await, 1, "whoever asked is told");
    for action in ["submit_contract", "send_for_signature", "take_request", "withdraw_approval"] {
        refused(t.act(&cm, unwanted, action, json!({"reason": "again"})).await, "invalid", "is not available");
    }
    // An approved contract can be withdrawn; one in review cannot.
    t.act(&cm, drafted, "submit_contract", Value::Null).await.unwrap();
    t.act(&cm, drafted, "withdraw_contract", json!({"reason": "Signed on paper by the other side's form"})).await.expect("an approved one");
    assert_eq!(t.status(drafted).await, "Withdrawn");
    let reviewed = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "In review", "counterparty_name": "Initech", "value": "250000", "starts_on": day.to_string(),
                                                         "ends_on": (day + Duration::days(365)).to_string()}),
        )
        .await["id"]);
    t.act(&cm, reviewed, "submit_contract", Value::Null).await.unwrap();
    assert_eq!(t.status(reviewed).await, "In Review");
    refused(t.act(&cm, reviewed, "withdraw_contract", json!({"reason": "No"})).await, "invalid", "is not available");
    // Who may take and withdraw: nobody without a Contracts role; a Contract Manager not a confidential request they cannot read.
    let other = t.insert(&system, "Contract", asked("Another request", false)).await;
    let other = uid(&other["id"]);
    let nobody = k.load_ctx(tenant, nobody.user, None, "it".into()).await.unwrap();
    for action in ["take_request", "withdraw_contract"] {
        let r = t.act(&nobody, other, action, json!({"reason": "Mine"})).await;
        assert!(matches!(r, Err(Error::Forbidden(_) | Error::NotFound(_))), "{action}: {r:?}");
    }
    let secret = uid(&t.insert(&system, "Contract", asked("A confidential request", true)).await["id"]);
    refused(t.act(&cm, secret, "take_request", Value::Null).await, "not found", "");
    // The upkeep leaves a withdrawn contract alone, a year on.
    ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(366)).await.unwrap();
    assert_eq!(t.status(unwanted).await, "Withdrawn");
    assert_eq!(sent(unwanted, "contracts.ending").await + sent(unwanted, "contracts.renewal_notice").await, 0, "no reminder");
    // An erasure blanks who asked, and the contract remains.
    t.k.erase_personal_data(&t.admin, "asker@outside.test", "asked").await.expect("erased");
    let blank = t.get("Contract", request).await;
    assert!(blank["requested_by"].is_null() && blank["request_note"].is_null() && blank["title"] == "NDA for the demo", "{blank:?}");

    // ───────────── obligations that repeat, and evidence before Done (CTR-07) ─────────────
    let ends = day + Duration::days(365);
    let duties = uid(&t
        .insert(&cm, "Contract", json!({"title": "Managed duties", "counterparty_name": "Globex", "starts_on": day.to_string(), "ends_on": ends.to_string()}))
        .await["id"]);
    let duty = |what: &str, due: chrono::NaiveDate, extra: Value| {
        let mut v = json!({"contract": duties.to_string(), "description": what, "owner": "ops@northwind.test", "due_on": due.to_string()});
        v.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
        v
    };
    let after = |o: Uuid| {
        let k = k.clone();
        async move {
            let mut tx = k.db.tenant_tx(tenant).await.unwrap();
            let rows: Vec<(Uuid, chrono::NaiveDate)> = sqlx::query_as("select id, due_on from e_obligation where tenant_id = $1 and previous = $2")
                .bind(tenant)
                .bind(o)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            rows
        }
    };
    let months = |d: chrono::NaiveDate, n: u32| d.checked_add_months(chrono::Months::new(n)).unwrap();
    // Obligations without a predecessor are not each other's duplicates.
    t.insert(&cm, "Obligation", duty("Kick-off", day + Duration::days(3), json!({}))).await;
    t.insert(&cm, "Obligation", duty("Handover", day + Duration::days(4), json!({}))).await;
    // Evidence first.
    let audited = uid(&t.insert(&cm, "Obligation", duty("Annual audit", day + Duration::days(30), json!({"evidence_required": true}))).await["id"]);
    let r = t.set(&cm, "Obligation", audited, json!({"status": "Done"})).await;
    assert!(matches!(&r, Err(Error::Validation(v)) if v.iter().any(|f| f.field == "evidence")), "done only with its evidence: {r:?}");
    let minutes = k.store_file(&cm, Some(("Obligation", audited)), "audit.pdf", "application/pdf", &pdf("Audit", 1)).await.unwrap();
    t.set(&cm, "Obligation", audited, json!({"status": "Done", "evidence": minutes["url"]})).await.expect("done with its evidence");
    // Done, a repeating obligation makes the next, once, even when reopened and done again.
    let first_due = day + Duration::days(10);
    let review = uid(&t.insert(&cm, "Obligation", duty("Quarterly service review", first_due, json!({"repeat_months": 3}))).await["id"]);
    t.set(&cm, "Obligation", review, json!({"status": "Done"})).await.unwrap();
    let next = after(review).await;
    assert_eq!(next.iter().map(|r| r.1).collect::<Vec<_>>(), [months(first_due, 3)], "done, it makes the next three months on");
    let made = t.get("Obligation", next[0].0).await;
    assert_eq!(
        (made["status"].as_str(), made["owner"].as_str(), made["description"].as_str(), made["previous"].as_str()),
        (Some("Open"), Some("ops@northwind.test"), Some("Quarterly service review"), Some(review.to_string().as_str()))
    );
    t.set(&cm, "Obligation", review, json!({"status": "Open"})).await.unwrap();
    t.set(&cm, "Obligation", review, json!({"status": "Done"})).await.unwrap();
    assert_eq!(after(review).await.len(), 1, "reopened and done again, it makes no second one");
    // The day of the month is kept: from the 31st, the end of a shorter month, then the 31st again.
    let jan31 = chrono::NaiveDate::from_ymd_opt(day.year() + 1, 1, 31).unwrap();
    let monthly = uid(&t.insert(&cm, "Obligation", duty("Monthly report", jan31, json!({"repeat_months": 1}))).await["id"]);
    t.set(&cm, "Obligation", monthly, json!({"status": "Done"})).await.unwrap();
    let feb = after(monthly).await;
    assert_eq!(feb[0].1, months(jan31, 1), "the end of February");
    t.set(&cm, "Obligation", feb[0].0, json!({"status": "Done"})).await.unwrap();
    assert_eq!(after(feb[0].0).await[0].1, chrono::NaiveDate::from_ymd_opt(day.year() + 1, 3, 31).unwrap(), "the 31st again");
    // It stops at its last day, and at the contract's end.
    let until = uid(&t
        .insert(
            &cm,
            "Obligation",
            duty("Until the launch", day + Duration::days(1), json!({"repeat_months": 1, "repeat_until": (day + Duration::days(20)).to_string()})),
        )
        .await["id"]);
    t.set(&cm, "Obligation", until, json!({"status": "Done"})).await.unwrap();
    assert!(after(until).await.is_empty(), "past its last day");
    let last = uid(&t.insert(&cm, "Obligation", duty("Last month", ends - Duration::days(10), json!({"repeat_months": 1}))).await["id"]);
    t.set(&cm, "Obligation", last, json!({"status": "Done"})).await.unwrap();
    assert!(after(last).await.is_empty(), "past the contract's end");
    refused(
        t.k.insert(&cm, "Obligation", obj(duty("Backwards", day + Duration::days(9), json!({"repeat_months": 1, "repeat_until": day.to_string()})))).await,
        "invalid",
        "on or after the day it is due",
    );
    // An ended contract makes no more.
    let ending = uid(&t
        .insert(&cm, "Contract", json!({"title": "Ending soon", "counterparty_name": "Globex", "starts_on": day.to_string(), "ends_on": ends.to_string()}))
        .await["id"]);
    let orphan = uid(&t
        .insert(
            &cm,
            "Obligation",
            json!({"contract": ending.to_string(), "description": "Monthly call", "owner": "ops@northwind.test",
                                          "due_on": (day + Duration::days(5)).to_string(), "repeat_months": 1}),
        )
        .await["id"]);
    t.act(&cm, ending, "withdraw_contract", json!({"reason": "Never went ahead"})).await.unwrap();
    t.set(&cm, "Obligation", orphan, json!({"status": "Done"})).await.unwrap();
    assert!(after(orphan).await.is_empty(), "an ended contract makes no more");
    // A missed one still brings the next, and two upkeeps at once make one.
    let missed = uid(&t.insert(&cm, "Obligation", duty("Monthly check", day + Duration::days(2), json!({"repeat_months": 1}))).await["id"]);
    let (a, b) = tokio::join!(
        ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(3)),
        ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(3))
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(t.get("Obligation", missed).await["overdue"], true);
    assert_eq!(after(missed).await.len(), 1, "a missed one still brings the next, once under two upkeeps");
    // Who may mark it done: not a Contract Approver.
    refused(t.set(&ca, "Obligation", missed, json!({"status": "Done"})).await, "forbidden", "");

    // ───────────── amendments, and contracts under a master agreement (CTR-08) ─────────────
    let copy = k.store_file(&both, None, "base-signed.pdf", "application/pdf", &pdf("Base, signed", 2)).await.unwrap();
    let base = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Base services", "counterparty_name": "Globex", "counterparty_email": "legal@globex.test", "value": "5000",
                   "starts_on": day.to_string(), "ends_on": (day + Duration::days(200)).to_string(), "notice_days": 30}),
        )
        .await["id"]);
    let lead = k.load_ctx(tenant, both.user, None, "it".into()).await.unwrap();
    t.act(&lead, base, "record_signed_copy", json!({"signed_file": copy["url"], "signed_by": "Both sides", "signed_on": day.to_string()})).await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, tenant, day).await.unwrap();
    assert_eq!(t.status(base).await, "Active");
    // Amend It makes a draft from the contract's terms.
    let made = t.act(&cm, base, "amend_contract", Value::Null).await.expect("amended");
    assert_eq!(made["entity"], "Contract");
    let first = t.get("Contract", uid(&made["id"])).await;
    assert_eq!(
        (first["amends"].as_str(), first["amendment_no"].as_i64(), first["starts_on"].as_str(), first["status"].as_str(), first["value"].as_str()),
        (Some(base.to_string().as_str()), Some(1), Some(day.to_string().as_str()), Some("Draft"), Some("5000"))
    );
    // One amendment at a time.
    refused(t.act(&cm, base, "amend_contract", Value::Null).await, "invalid", "already under way");
    t.act(&cm, uid(&first["id"]), "withdraw_contract", json!({"reason": "Started over"})).await.unwrap();
    let second = uid(&t.act(&cm, base, "amend_contract", Value::Null).await.expect("after the first is withdrawn")["id"]);
    assert_eq!(t.get("Contract", second).await["amendment_no"].as_i64(), Some(2));
    // Signed, an amendment waits for its day.
    let new_end = day + Duration::days(500);
    t.set(
        &cm,
        "Contract",
        second,
        json!({"starts_on": (day + Duration::days(1)).to_string(), "value": "7000", "ends_on": new_end.to_string(), "notice_days": 45,
               "auto_renew": true, "renewal_months": 6}),
    )
    .await
    .expect("the amendment's terms");
    t.act(&cm, second, "submit_contract", Value::Null).await.unwrap();
    let paper = k.store_file(&cm, None, "amendment.pdf", "application/pdf", &pdf("Amendment", 1)).await.unwrap();
    t.act(&cm, second, "record_signed_copy", json!({"signed_file": paper["url"], "signed_by": "Both sides", "signed_on": day.to_string()})).await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, tenant, day).await.unwrap();
    let still = t.get("Contract", base).await;
    assert_eq!((still["value"].as_str(), t.get("Contract", second).await["applied_on"].clone()), (Some("5000"), Value::Null), "not before its day");
    // On its day it changes the contract, once.
    ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(1)).await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(1)).await.unwrap();
    let amended = t.get("Contract", base).await;
    assert_eq!(
        (
            amended["value"].as_str(),
            amended["ends_on"].as_str(),
            amended["notice_days"].as_i64(),
            amended["auto_renew"].as_bool(),
            amended["renewal_months"].as_i64(),
            amended["amendments"].as_i64()
        ),
        (Some("7000"), Some(new_end.to_string().as_str()), Some(45), Some(true), Some(6), Some(1)),
        "on its day it changes the contract once"
    );
    assert_eq!(t.get("Contract", second).await["applied_on"], json!((day + Duration::days(1)).to_string()));
    // Two upkeeps at once apply an amendment once.
    let third = uid(&t.act(&cm, base, "amend_contract", Value::Null).await.unwrap()["id"]);
    t.set(&cm, "Contract", third, json!({"starts_on": (day + Duration::days(2)).to_string(), "value": "9000"})).await.unwrap();
    t.act(&cm, third, "submit_contract", Value::Null).await.unwrap();
    t.act(&cm, third, "record_signed_copy", json!({"signed_file": paper["url"], "signed_by": "Both sides", "signed_on": day.to_string()})).await.unwrap();
    let (a, b) = tokio::join!(
        ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(2)),
        ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(2))
    );
    a.unwrap();
    b.unwrap();
    let twice = t.get("Contract", base).await;
    assert_eq!((twice["value"].as_str(), twice["amendments"].as_i64()), (Some("9000"), Some(2)), "two upkeeps at once apply an amendment once");
    // An amendment has no term of its own; the contract's reminders follow the amended end.
    ferrum_app_contracts::maintenance_on(&k, tenant, day + Duration::days(400)).await.unwrap();
    ferrum_app_contracts::maintenance_on(&k, tenant, new_end - Duration::days(50)).await.unwrap();
    for a in [second, third] {
        assert_eq!(t.status(a).await, "Signed", "never Active or Expired");
        assert_eq!(sent(a, "contracts.ending").await + sent(a, "contracts.renewal_notice").await, 0, "an amendment reminds nobody");
        for action in ["renew_contract", "terminate_contract", "amend_contract"] {
            refused(t.act(&cm, a, action, json!({"reason": "No"})).await, "invalid", "is not available");
        }
    }
    assert_eq!(t.status(base).await, "Active");
    assert_eq!(sent(base, "contracts.renewal_notice").await, 1, "the contract's reminders follow the amended end");
    assert_eq!(t.get("Contract", base).await["notice_reminded_for"], json!(new_end.to_string()));
    // Who may amend.
    refused(t.act(&ca, base, "amend_contract", Value::Null).await, "forbidden", "");
    refused(t.act(&cm, secret, "amend_contract", Value::Null).await, "not found", "");
    // Reports count a contract once: an amendment's value is not counted beside its contract's.
    let states = reports[0].run(&k, &t.admin, "contracts_by_state", &HashMap::new()).await.unwrap();
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    let (all, kept): (i64, i64) = sqlx::query_as("select count(*), count(*) filter (where amends is null) from e_contract where tenant_id = $1")
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(all > kept, "amendments exist");
    assert_eq!(states["totals"]["contracts"].as_i64(), Some(kept), "{states}");
    // A master is another contract, not itself and not an amendment.
    let under = uid(&t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Order form 1", "counterparty_name": "Globex", "starts_on": day.to_string(), "ends_on": (day + Duration::days(90)).to_string()}),
        )
        .await["id"]);
    let own = t.set(&cm, "Contract", under, json!({"master": under.to_string()})).await;
    assert!(matches!(&own, Err(Error::Validation(v)) if v.iter().any(|f| f.field == "master")), "not itself: {own:?}");
    refused(t.set(&cm, "Contract", under, json!({"master": second.to_string()})).await, "invalid", "is another contract, not an amendment");
    t.set(&cm, "Contract", under, json!({"master": base.to_string()})).await.expect("under a master");
    let listed = t.k.list(&cm, "Contract", &lq(ferrum_kernel::query::Filter::eq("master", base.to_string()))).await.unwrap().rows;
    assert!(listed.iter().any(|r| r["id"] == json!(under.to_string())), "listed under its master");

    // ───────────── contracts signed before, created signed, many at a time (CTR-12) ─────────────
    let upload = |who: &Ctx, name: &str| {
        let (k, who, name) = (k.clone(), who.clone(), name.to_string());
        async move { k.store_file(&who, None, &name, "application/pdf", &pdf(&name, 1)).await.unwrap()["url"].as_str().unwrap().to_string() }
    };
    let loose = |url: String| {
        let k = k.clone();
        async move {
            let id: Uuid = url.rsplit('/').next().unwrap().parse().unwrap();
            let mut tx = k.db.tenant_tx(tenant).await.unwrap();
            let on: (Option<String>, Option<Uuid>) = sqlx::query_as("select entity, record_id from file_object where tenant_id = $1 and id = $2")
                .bind(tenant)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            on
        }
    };
    let before = |title: &str, url: &str, on: Option<String>| {
        let mut v = json!({"title": title, "counterparty_name": "Initech", "starts_on": day.to_string(), "ends_on": (day + Duration::days(365)).to_string(),
                           "recorded_copy": url, "recorded_signed_by": "Bill for Initech; Nora for us"});
        if let Some(on) = on {
            v["recorded_signed_on"] = json!(on);
        }
        v
    };
    let signed_on = (day - Duration::days(30)).to_string();
    // A contract signed before is created signed, its copy attached to it.
    let old_copy = upload(&legal, "old-msa.pdf").await;
    let made = t.k.insert(&legal, "Contract", obj(before("Old MSA", &old_copy, Some(signed_on.clone())))).await.expect("created signed");
    let old = uid(&made["id"]);
    let got = t.get("Contract", old).await;
    assert_eq!(
        (got["status"].as_str(), got["signed_document"].as_str(), got["signed_elsewhere"].as_bool(), got["signed_on"].as_str()),
        (Some("Signed"), Some(old_copy.as_str()), Some(true), Some(signed_on.as_str())),
        "a contract signed before is created signed"
    );
    assert_eq!(loose(old_copy.clone()).await, (Some("Contract".to_string()), Some(old)), "its copy is attached to it");
    ferrum_app_contracts::maintenance_on(&k, tenant, day).await.unwrap();
    assert_eq!(t.status(old).await, "Active");
    // An import brings in many; a refused row keeps no file on a contract; one file, one contract.
    let (a, b, c, d) = (upload(&legal, "a.pdf").await, upload(&legal, "b.pdf").await, upload(&legal, "c.pdf").await, upload(&legal, "d.pdf").await);
    let rows = vec![
        before("Imported A", &a, Some(signed_on.clone())),
        before("Imported B", &b, Some(signed_on.clone())),
        before("Imported C", &c, Some(signed_on.clone())),
        before("Imported D", &d, None),
        before("Imported A again", &a, Some(signed_on.clone())),
    ];
    let (ids, errors) = t.k.import_rows(&legal, "Contract", &rows).await.expect("imported");
    assert_eq!(ids.len(), 3, "three good rows: {errors:?}");
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0]["row"], 3);
    assert!(errors[0].to_string().contains("recorded_signed_on"), "{errors:?}");
    assert_eq!(errors[1]["row"], 4, "one file, one contract: {errors:?}");
    assert_eq!(loose(d).await, (None, None), "a refused row keeps no file on a contract");
    for id in &ids {
        assert_eq!(t.get("Contract", uid(id)).await["status"], "Signed");
    }
    // Who may: not a Contract Manager; without the three fields, a draft as before.
    let theirs = upload(&cm, "cm.pdf").await;
    refused(t.k.insert(&cm, "Contract", obj(before("By a manager", &theirs, Some(signed_on.clone())))).await, "forbidden", "Contract Approver or Legal");
    let plain = t
        .insert(
            &cm,
            "Contract",
            json!({"title": "Plain draft", "counterparty_name": "Initech", "starts_on": day.to_string(),
                                                 "ends_on": (day + Duration::days(365)).to_string()}),
        )
        .await;
    assert_eq!(plain["status"], "Draft");
    // A draft is not signed by editing.
    refused(
        t.set(&cm, "Contract", uid(&plain["id"]), json!({"recorded_copy": theirs, "recorded_signed_by": "x", "recorded_signed_on": signed_on})).await,
        "invalid",
        "recorded when the contract is created",
    );
    // Someone else's upload is refused, and no contract is left.
    refused(t.k.insert(&legal, "Contract", obj(before("Not theirs", &theirs, Some(signed_on.clone())))).await, "not found", "");
    assert_eq!(loose(theirs.clone()).await, (None, None));
    // The system (a form) cannot record a signed copy.
    let spare = upload(&legal, "spare.pdf").await;
    let system = Ctx::system(tenant, k.registry.schema(tenant).await.unwrap());
    refused(t.k.insert(&system, "Contract", obj(before("By a form", &spare, Some(signed_on.clone())))).await, "forbidden", "recorded by a person");
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    let none: i64 = sqlx::query_scalar("select count(*) from e_contract where tenant_id = $1 and title in ('Not theirs', 'By a form', 'By a manager')")
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(none, 0, "no contract left by a refused create");
    // Obligations come in from a sheet.
    let sheet = vec![
        json!({"contract": old.to_string(), "description": "Annual price review", "owner": "ops@northwind.test", "due_on": (day + Duration::days(60)).to_string()}),
        json!({"contract": old.to_string(), "description": "Insurance certificate", "owner": "ops@northwind.test", "due_on": (day + Duration::days(90)).to_string()}),
    ];
    let (made, failed) = t.k.import_rows(&cm, "Obligation", &sheet).await.expect("obligations imported");
    assert_eq!((made.len(), failed.len()), (2, 0), "{failed:?}");
}
