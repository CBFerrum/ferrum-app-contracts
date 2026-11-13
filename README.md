<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/banner-dark.svg">
  <img alt="Ferrum Contracts: from clause library to signed, renewed and tracked" src="docs/assets/banner-light.svg" width="100%">
</picture>

<br>

[![Version](https://img.shields.io/badge/version-0.3.1-334155?style=flat-square)](CHANGELOG.md)
[![Licence: AGPL-3.0](https://img.shields.io/badge/licence-AGPL--3.0-blue?style=flat-square)](LICENSE)
[![Ferrum platform](https://img.shields.io/badge/runs%20on-Ferrum-2f5bea?style=flat-square)](https://github.com/CBFerrum/Ferrum)
![Rust](https://img.shields.io/badge/Rust-server-b7410e?style=flat-square&logo=rust)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-data-336791?style=flat-square&logo=postgresql&logoColor=white)
![React](https://img.shields.io/badge/React%20%2B%20TypeScript-interface-149eca?style=flat-square&logo=react&logoColor=white)

### Draft a contract from approved clauses, get it approved, signed, renewed and tracked, in one place.

Ferrum Contracts is for legal, sales, procurement and operations teams that keep agreements in shared folders and
spreadsheets. It replaces the mix of a word processor, an approval chain by mail, a signing tool and a renewal
calendar with one record per contract, and ties each contract to the customer, supplier, deal, order or project it
governs.

[💡 Why](#-why-contracts) · [📸 Screenshots](#-screenshots) · [✨ Features](#-features) · [📊 Compare](#-how-it-compares) · [📍 Roadmap](#-roadmap) · [🚀 Install](#-install) · [🔧 Develop](#-develop)

</div>

## 💡 Why Contracts

- 🧱 **Built from approved wording.** A contract starts from a type, a document template and a library of
  clauses that only a Contract Approver can approve. The contract keeps each clause as it was worded when it was drafted.
- 🔏 **An approval that means something.** The approval is bound to a digest of the terms, the clauses and the PDF.
  Change any of them and the approval no longer holds.
- ✍️ **Signature without leaving Ferrum.** Approved contracts go to the Sign app, to as many signatories as needed,
  in order. A copy signed on paper can be recorded instead.
- 🔗 **One data set with the rest of Ferrum.** A contract can link to the customer, supplier, contact, deal, sales
  order, purchase order, project or employee it concerns. "Contracts for this record" is one filter away.
- 🔔 **Nobody misses a date.** Notice dates for renewal, end dates and obligations send reminders once, by mail and
  in the app.
- 🏠 **Self-hosted and open source.** AGPL-3.0, run on your own server and database. Your contracts stay yours.
- 🧩 **Changeable without code.** The approval policy, the document templates, the contract types and the reminder
  texts are records a workspace edits in Settings, Customize and Studio.

## 🎯 At a glance

| | |
|---|---|
| **Version** | 0.3.1 (platform `>=0.3.2, <0.4`) |
| **Kind** | Own interface at `/contracts/`, plus Ferrum's generic records screens for every entity |
| **What it adds** | 5 main entities and 5 child tables, 3 roles, 1 approval workflow, 4 reports, 2 document templates (print formats), 3 message templates, 1 menu module, 2 seeded contract types |
| **Depends on** | Ferrum core and **Sign** (installing Contracts installs Sign) |
| **Works with** | Sign, Parties (customers and suppliers), CRM, Selling, Buying, HR, Projects, Documents (templates) |
| **Languages** | English, Arabic (right to left), Hindi; a workspace can change any interface word |
| **Tests** | 5 Rust tests (one walks a whole year of contracts end to end) and UI tests |
| **Licence** | AGPL-3.0-only |

## 📸 Screenshots

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-home-dark.webp">
  <img alt="Contracts home" src="docs/assets/screens/contracts-home-light.webp" width="100%">
</picture>

**📋 Contracts home**<br><sub>See what needs attention: notice due before renewal, open obligations, and contracts in the works, out for signature and active.</sub>

<table>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-contract-list-dark.webp"><img alt="Contract list" src="docs/assets/screens/contracts-contract-list-light.webp"></picture>
<p><b>📄 Contracts list</b><br><sub>Browse, search and filter every contract by counterparty, value, end date and status.</sub></p>
</td>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-contract-detail-dark.webp"><img alt="Contract record" src="docs/assets/screens/contracts-contract-detail-light.webp"></picture>
<p><b>🔎 Contract record</b><br><sub>Read one contract with its terms, clauses, signatories, versions and the actions its state allows.</sub></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-clause-list-dark.webp"><img alt="Clause library" src="docs/assets/screens/contracts-clause-list-light.webp"></picture>
<p><b>📚 Clause library</b><br><sub>Find approved wording by name and category before drafting.</sub></p>
</td>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-clause-detail-dark.webp"><img alt="Clause record" src="docs/assets/screens/contracts-clause-detail-light.webp"></picture>
<p><b>🧱 Clause record</b><br><sub>Read a clause from the library, its merge fields and the fallback that can stand in for it.</sub></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-obligation-list-dark.webp"><img alt="Obligation list" src="docs/assets/screens/contracts-obligation-list-light.webp"></picture>
<p><b>⏰ Obligations list</b><br><sub>See what each side must do and by when, with the late ones marked.</sub></p>
</td>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-obligation-detail-dark.webp"><img alt="Obligation record" src="docs/assets/screens/contracts-obligation-detail-light.webp"></picture>
<p><b>✅ Obligation record</b><br><sub>See who owes what under a contract, by when, and mark it done.</sub></p>
</td>
</tr>
</table>

<table>
<tr>
<td align="center" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-home-phone-dark.webp"><img alt="Contracts home on a phone" src="docs/assets/screens/contracts-home-phone-light.webp" width="260"></picture>
<p><b>📱 Home on a phone</b></p>
</td>
<td align="center" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/contracts-contract-list-phone-dark.webp"><img alt="Contract list on a phone" src="docs/assets/screens/contracts-contract-list-phone-light.webp" width="260"></picture>
<p><b>📱 Contracts on a phone</b></p>
</td>
</tr>
</table>

## ✨ Features

### 🗂️ Contract types and bodies

- **Contract Type.** A kind of contract: its name, the document template its body is made from, its clauses (each
  can be marked mandatory) and how it renews (New Version or New Contract). A type can be disabled.
- **Two generic types ship.** Services Agreement (`contracts.services_agreement`) and Non-Disclosure Agreement
  (`contracts.non_disclosure`). They name no country and no law. A workspace edits them under Settings, Documents.
- **A body made from the template.** While a contract is a draft, its body is the template rendered live from the
  contract's fields, parties and clauses. Print the contract with that template at any time.
- **A body kept by version.** Each Submit It freezes the body as a file kept on the contract and never replaced.
  The Versions table lists each one with its number, digest (SHA-256), who made it, when and why (submitted,
  submitted again after being sent back, renewed).
- **Contracts without a type.** A contract with no type keeps an attached PDF instead.
- **Numbering.** Every contract gets a number such as `CTR-2026-00001`.
- **Category.** Sales, Purchase, Service, Non-Disclosure, Employment, Lease or Other, for filters and reports.

### 📚 Clause library

- **Clause.** Named wording with a category (Payment, Liability, Confidentiality, Termination, Data Protection,
  Intellectual Property, Other). The text is markdown and may hold `{{ doc.<field> }}` of the contract, such as
  `counterparty_name`, `starts_on` or `value`, filled in when the clause is drafted.
- **Approved for use.** Only a Contract Approver approves a clause, and only a Contract Approver changes an approved
  clause's wording.
- **Fallbacks.** A clause can name a fallback: the wording offered when this one is refused. Where a type makes a
  clause mandatory, a contract may carry the fallback in its place.
- **Mandatory clauses.** A contract of a type is submitted only with each mandatory clause or its fallback.
- **Wording kept as drafted.** A contract keeps the wording of each clause as it was when drafted. Changing the
  library later does not change a contract that already used it.

### 📝 Drafting

- **Parties.** A typed counterparty (name and email) always stays on the contract.
- **Parties as records.** With the shared customers and suppliers installed, a contract also links to a Customer,
  a Supplier and a Contact. See [Integrations](#-integrations) for the other links.
- **Term and value.** Start and end dates, a value, auto-renew with its months, and the days of notice.
- **Renewal Notice On.** Worked out for you: the end date less the notice days.
- **Department and owner.** The creator becomes the owner. The department is read by approval policies that route
  by department.
- **Terms are kept.** Out of its draft, a contract keeps its party, type, value, term, renewal, clauses and
  document. They no longer change under an approval.
- **Contract Settings.** The approval threshold (default 10,000), who signs for the organisation when a contract
  names no signatories (name and email, required) and the reminder days (default 14, 0 for none).

### ✅ Approval by policy

- **Submit It** puts a contract In Review and asks the package's approval workflow, Contract Approval.
- **The default policy.** A contract worth the settings' threshold or more needs one Contract Approver other than
  whoever submitted it, with a reminder after two days. A cheaper contract is approved at once.
- **Your own policy.** A workspace replaces the workflow on Contract (Customize, workflow) and keeps a transition
  named `Approve` from its first state. Steps can be set by value band, by type, by department, in order or
  together, as N of M, with stand-ins and escalation.
- **Approvers decide under Approvals.** A rejection sends the contract back to Draft, with the reason, at the next
  upkeep.
- **Send It Back.** Returns a contract in review to its draft with a reason that is required.
- **An approval bound to what was approved.** The approval is bound to a digest of the terms, the clauses' wording
  and the PDF. The approver also confirms the PDF says what the clauses say. A contract goes for signature only as
  approved.
- **Separation of duties.** A contract is approved by someone other than its owner.
- **Fallback.** A workspace whose workflow has no `Approve` transition gets the single approver this app had
  before: Approve It and Send It Back.

### ✍️ Signature

- **Send for Signature.** An approved contract goes to Sign, to its signatories in their order. Equal order numbers
  sign together. A contract that lists none goes to the counterparty, then the Contract Settings' signatory.
- **Signatories.** A table of name, email, role (such as Counterparty, Organisation, Witness) and order, at most
  twelve.
- **Signature boxes.** Each signatory's box is on the last page, the signature page the packaged templates end
  with. A contract with no type and no signatories keeps the two boxes it always had.
- **Follows Sign.** At the next upkeep, a completed request makes the contract Signed with the sealed PDF; a
  request that is declined, expired or voided returns the contract to Approved and says why.
- **A page is refused up front.** Where there is no PDF service, a body is kept as a page. Send for Signature
  then refuses and names `FERRUM_PDF_URL`. The contract stays Approved and no request is made.
- **Signed elsewhere.** Record Signed Copy takes the uploaded signed PDF, who signed and when. The contract becomes
  Signed, is marked Signed Elsewhere and keeps its own copy. From a draft (an import), only a Contract Approver or
  Legal can record it.
- **The sealed copy.** A signed contract keeps its own copy of the sealed PDF. Whoever may read the contract may
  open it.

### 🔄 Term, renewal and ending

- **Active from its start.** A signed contract becomes Active on its start date.
- **Renewal.** Past its end date a contract renews for its months when it says so, and expires otherwise, once.
- **Renew It.** Makes the next term: on the same contract with its body as a new version, or as a new draft
  contract linked both ways (Previous and Renewed By), as its type says.
- **Notice due.** A self-renewing contract inside its notice period is flagged and shown on the home page
  under "Notice due before renewal".
- **Terminate It.** Ends a Signed or Active contract with a required reason and the date.
- **Counters.** The number of renewals is kept on the contract.

### ⏰ Obligations and reminders

- **Obligation.** Something a contract commits to, or a milestone in it. It has a contract, a description, a kind
  (Obligation or Milestone), whose it is (Us or Counterparty), an owner, a due date and a status (Open or Done).
- **Overdue** is set when a date passes with the obligation open. The home page puts the overdue ones first.
- **Reminders.** Reminder Days before an obligation is due, before a self-renewing contract's notice date and
  before a non-renewing contract ends, the owners are told once, by mail and in the app, using three message
  templates (`contracts.obligation_due`, `contracts.renewal_notice`, `contracts.ending`) that a workspace edits
  under Settings, Messages.
- **Once only.** Two upkeeps running at once send each reminder once: each step locks its rows and records what it
  sent in the same transaction.

### 🔍 Find, report, import

| Feature | What it does |
|---|---|
| **Search** | A contract is found by counterparty and by the words of its latest frozen body. Text inside an uploaded PDF is not searched. |
| **Contracts by State** | Contracts with their value, grouped by state, type, owner or category. |
| **Contracts Ending or Renewing** | Contracts ending or renewing in a period. |
| **Obligations Due** | Obligations due up to a date. |
| **Days from Draft to Signed** | Cycle time of contracts signed in a period. |
| **CSV import** | Import contracts as drafts (title, counterparty, dates, value, type, `confidential`) as Contract Manager or Legal, then upload each signed copy and Record Signed Copy on it. |
| **Export** | Contract Manager and Legal export contracts. Obligations can be exported too. |
| **Print** | Print a contract with its type's template. |

### 🔐 Access, privacy and records

- **Confidential contracts.** A confidential contract is read only by its owner, its readers, its signatories and
  Legal (and, while their decision is awaited, its approvers). Contract Manager and Contract Approver read every
  other contract.
- **Readers.** A table of email addresses who may read a contract while it is confidential.
- **Personal data.** Requests for a person's data find the contracts naming them as counterparty or signatory.
  Those contracts are kept, as contract law and audit require, and a reader's address is erased.
- **Audit trail.** Every change and state move is recorded by the platform.

### 🌍 Interface

- **Its own pages.** `/contracts/` shows queues for notice due, open obligations and the three stages (in the
  works, out for signature, active). Every queue item opens its record.
- **Notices on the shell.** Reminders arrive in the in-app inbox and by mail.
- **Languages and right to left.** Arabic and Hindi words for the interface and the names of records; pages mirror
  for right-to-left languages.
- **Assistant.** The Ferrum assistant knows the contract lifecycle and suggests questions such as "Which contracts
  expire in the next 90 days?"
- **Phone.** The pages work on a phone's browser.

## 🧭 How it works

A contract moves through eight states. Approval and signature run on their own platform engines; the app moves the
contract when each answers.

```mermaid
flowchart LR
    A["Draft"] -->|"Submit It"| B["In Review"]
    B -->|"Approved by policy"| C["Approved"]
    B -->|"Send It Back"| A
    C -->|"Send for Signature"| D["Out for Signature"]
    D -->|"Declined or expired"| C
    D -->|"Signed in Sign"| E["Signed"]
    A -->|"Record Signed Copy"| E
    C -->|"Record Signed Copy"| E
    E -->|"Start date"| F["Active"]
    F -->|"End date, renews"| F
    F -->|"End date, no renewal"| G["Expired"]
    F -->|"Terminate It"| H["Terminated"]
```

Each Submit It freezes the body as a version and binds the approval to its digest. Past an end date the contract
renews or expires, once. Terminate It is available from Signed and Active.

```mermaid
flowchart TD
    CT["Contract Type"] --> T["Document template"]
    CT --> CL["Clause library"]
    T --> C["Contract"]
    CL --> C
    C --> S["Sign"]
    C --> OB["Obligations"]
    C --- P["Customers, suppliers, contacts"]
    C --- CRM["CRM deals"]
    C --- SO["Sales and purchase orders"]
    C --- PR["Projects"]
    C --- HR["Employees"]
```

The type picks the template and starts the contract with its clauses. Sign carries the signature. The linked
records are optional and appear when the matching app is installed.

## 🧱 Data model

| Entity | What it is | Key fields or states |
|---|---|---|
| **Contract** | An agreement from draft to signature and through its term | Number, title, type, category, counterparty, owner, department, confidential, value, start, end, auto-renew, notice days, status (Draft, In Review, Approved, Out for Signature, Signed, Active, Expired, Terminated) |
| **Contract Type** | A kind of contract | Name, template, renewal (New Version or New Contract), clauses, disabled |
| **Clause** | Approved wording | Name, category, text with merge fields, fallback, approved |
| **Obligation** | A commitment or milestone | Contract, description, kind, party, owner, due on, status, overdue |
| **Contract Settings** | Workspace settings | Approval threshold, signatory name and email, reminder days |
| Type Clause (child) | A clause a type starts with | Clause, mandatory |
| Clause on contract (child) | A clause as drafted | Clause, text as drafted |
| Signatory (child) | One signer | Name, email, role, order |
| Reader (child) | Who may read a confidential contract | Email |
| Version (child) | One frozen body | Number, file, SHA-256, who, when, reason |

| Role | What it can do |
|---|---|
| **Contract Manager** | Drafts contracts, keeps clauses, types, settings and obligations, imports and exports contracts, sends them for signature. Reads a confidential contract only as owner or reader. |
| **Contract Approver** | Approves or sends back contracts as the policy asks, approves clauses and changes approved wording. Reads a confidential contract only as owner, reader or asked approver. |
| **Legal** | Reads and keeps every contract, confidential ones too, and every obligation. Records signed copies. |

| Item | Names |
|---|---|
| **Reports** | Contracts by State, Contracts Ending or Renewing, Obligations Due, Days from Draft to Signed |
| **Print formats** | Services agreement, Non-disclosure agreement (each ends in a signature page) |
| **Messages** | Obligation due, Renewal notice date coming, Contract ending |
| **Workflow** | Contract Approval (Open to Approved, with approval steps by value) |
| **Upkeep** | Activation, renewal, expiry, notice flags and reminders on a schedule |

## 🔌 Integrations

| With | How | Needs |
|---|---|---|
| **Sign** | Signature requests, order of signing, sealed PDF back on the contract | Sign installed (comes with Contracts); `FERRUM_PDF_URL` for the PDF service |
| **Parties** (`contracts_parties`) | Customer, Supplier and Contact links on a contract | Parties app |
| **CRM** (`contracts_crm`) | Opportunity the contract closes | CRM app |
| **Selling** (`contracts_selling`) | Sales order the contract governs | ERP selling |
| **Buying** (`contracts_buying`) | Purchase order the contract governs | ERP buying |
| **Projects** (`contracts_projects`) | Project the contract governs | Projects app |
| **HR** (`contracts_hr`) | Employee the contract is with | HR app |
| **Documents** | Templates edited under Settings, Documents | Platform |
| **Approvals** | Approval policy engine, stand-ins and escalation | Platform |
| **Mail and in-app notices** | Reminder messages | Platform mail settings |

Each connector is a small package that adds one link field and installs only where the app it joins is present.

## 📊 How it compares

Compared with five contract lifecycle management products: Ironclad, DocuSign CLM, Icertis, Agiloft and Juro.

| Capability | Ferrum Contracts | Ironclad | DocuSign CLM | Icertis | Agiloft | Juro |
|---|:-:|:-:|:-:|:-:|:-:|:-:|
| Contract types with templates that generate the body | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Clause library with approved wording and fallbacks | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Clause versions and playbooks | ❌ | ✅ | 🟡 | ✅ | ✅ | ✅ |
| Merge fields filled from record data | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Version history of the body | 🟡¹ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Compare two versions | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Edit and redline with the counterparty | ❌ | ✅ | 🟡² | 🟡² | 🟡² | ✅ |
| Business intake request triaged by legal | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Approval routing by value, type or department | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Approver reminders, stand-ins and escalation | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| E-signature in the same product | 🟡³ | ✅ | ✅ | 🟡⁴ | 🟡⁴ | ✅ |
| Several signatories in order | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Record a contract signed on paper | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Renewal and notice reminders | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Obligations tracked with owner and due date | ✅ | 🟡 | ✅ | ✅ | ✅ | 🟡 |
| Recurring obligations, evidence upload | ❌ | 🟡 | ✅ | ✅ | ✅ | 🟡 |
| Amendments and master agreements with orders beneath | ❌ | ✅ | 🟡 | ✅ | ✅ | 🟡 |
| Searchable repository by metadata | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Full-text search inside uploaded PDFs | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| AI reads a contract and fills its fields | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| AI review against a playbook | ❌ | ✅ | 🟡 | ✅ | 🟡 | ✅ |
| Confidential contracts by owner, reader and role | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Import existing contracts in bulk | 🟡⁵ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Reports and dashboards | 🟡⁶ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Counterparty portal | ❌ | ✅ | 🟡 | 🟡 | 🟡 | ✅ |
| Same data set as CRM, orders, projects and accounts | ✅ | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 |
| Workflow editable by an administrator without code | 🟡⁷ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Self-hosted | ✅ | ❌ | ❌ | ❌ | ✅⁸ | ❌ |
| Open source | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |

¹ Each submit freezes a version with its digest, who and why; edits between submits are not kept as versions.
² Redlining through Word or a companion editor rather than a browser editor, as far as public product pages say.
³ Through Ferrum's Sign app, installed with Contracts, not inside Contracts itself.
⁴ Through integrations with external e-signature products as well as, in some cases, their own.
⁵ CSV of contracts as drafts, then each signed copy is uploaded and recorded one by one.
⁶ Four reports. Studio queries and dashboards can be added; no contract dashboard ships.
⁷ The approval workflow is editable in Customize; other lifecycle states are fixed in code.
⁸ Agiloft offers an on-premises option.

### 🏆 Where Contracts stands out

- 🔏 **Approvals bound to what was approved**, with separation of duties and reminders built in.
- 🔗 **One data set** with customers, suppliers, deals, orders, projects and employees, not a connector.
- 🏠 **Self-hosted, open source and your own database**, with personal-data requests handled by the platform.
- 🧩 **Everything a legal team edits is a record**: templates, types, clauses, the approval policy, reminder texts.
- 🌍 **Arabic and Hindi** words and right-to-left layout.
- ✍️ **Paper copies are first-class**: record a contract signed elsewhere with who and when.

### 🚧 Where the leaders are still ahead

- ✏️ **Redlining and negotiation** with the counterparty, and a counterparty portal.
- 🤖 **AI** that reads a contract, fills its fields and checks it against a playbook.
- 📥 **Intake and playbooks**: business request forms, clause versions and deviation scoring.
- 🔎 **Full-text search** inside uploaded PDFs.
- 🧬 **Contract hierarchies**: amendments, master agreements and orders beneath them.
- 📈 **Dashboards** and analytics, and years of integrations with other vendors' systems.

> [!NOTE]
> Leader columns are from public product information as of November 2026 and may change.

## 📍 Roadmap

### ✅ Recently shipped

- [x] **0.3.1.** Send for Signature refuses a page up front and names the missing PDF service.
- [x] **0.3.0.** Contract types and generated bodies kept by version, clauses with merge fields and fallbacks,
  parties and governed records as links, approval by policy, signatories in order, signed-elsewhere copies,
  renewals as versions or new contracts, reminders, four reports and confidential contracts.
- [x] **0.2.2.** Rebuilt on the tuned design system: better menu icons, lighter first page, cleaner print.
- [x] **0.2.1.** The redesigned interface with shaped loading and told-apart states.
- [x] **0.2.0.** An approval is bound to the wording and PDF it was given.
- [x] **0.1.17.** Names in Arabic and Hindi.

### 🔜 Next up

- [ ] **Redlining.** Edit a body with tracked changes and let the counterparty respond, because most contracts are negotiated.
- [ ] **Compare two versions** of a body, as the versions are already kept.
- [ ] **Amendments and master agreements.** A parent link with orders and statements of work beneath it.
- [ ] **Recurring obligations** with a linked clause and an evidence upload.
- [ ] **Business intake.** A request from outside legal that legal triages into a contract.
- [ ] **Counterparty portal** to review and sign without an account.

### 🔭 Later

**Drafting and negotiation**
- [ ] Clause versions and playbooks with preferred and fallback wording per type.
- [ ] Deviation scoring that routes an approval by how far a contract strays from the playbook.
- [ ] Signature boxes placed by marks in an uploaded PDF.

**Repository and search**
- [ ] Search inside the text of an uploaded PDF.
- [ ] Contract read by AI: parties, dates, value and clauses proposed into the fields.
- [ ] Bulk import of existing files with their metadata.

**Term and termination**
- [ ] A renewal task and a notice-sent step before the notice date passes.
- [ ] Price uplift on renewal and a renewal opportunity in CRM.
- [ ] Termination details: notice served, cure period and fees.

**Money**
- [ ] Contract value and milestones driving purchase orders, invoices or revenue schedules.
- [ ] Spend against contract value.
- [ ] Recurring value and multiple currencies on a contract.

**Platform**
- [ ] A contract dashboard of expiring, renewing and overdue items.
- [ ] Follow a Sign request by its event rather than at upkeep.
- [ ] Approval turnaround reported by approver.

## 🚀 Install

Contracts is a native Ferrum app, compiled into the Ferrum server. A workspace gets it from the App Manager or the
store. It needs platform `>=0.3.2, <0.4`, and installing it installs **Sign**. The connector packages install by
themselves where the matching app is present.

1. Install Contracts (and Sign with it).
2. To send contracts for signature, an operator sets `FERRUM_PDF_URL` (the PDF service the platform renders
   documents with). Without it contracts can still be approved and kept, and a copy signed elsewhere recorded.
3. Open Contract Settings: set who signs for the organisation (name and email), the approval threshold and the
   reminder days.
4. Give people the roles: Contract Manager, Contract Approver and Legal.
5. Open Clause Library, write or import clauses, and have a Contract Approver approve them.
6. Edit the two packaged types (and their templates under Settings, Documents) or add your own.
7. Create a contract at `/contracts/` with New contract, or import existing ones from CSV.

## 🔧 Develop

```bash
# server (needs a PostgreSQL role that is not a superuser, so row-level security applies)
export FERRUM_CONTRACTS_TEST_DATABASE_URL=postgres://ferrum:ferrum@localhost:5432/ferrum_contracts_test
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The interface is built with Ferrum-Frontend, checked out next to this repository as `../ferrum/web`:

```bash
cd ui
npm run build       # writes ui/dist, which is committed and embedded by build.rs
npm run check
npm run typecheck
npm test
```

`tests/contracts_it.rs` walks a year of contracts with Sign installed and a stand-in PDF service: clauses approved
and their wording kept, contracts approved at once, by another approver or sent back, a type's body frozen and
found by its words, one signed through Sign to three signatories in order, one declined, one signed on paper,
activation, notice, renewal, expiry, termination, reminders sent once from two upkeeps at once, obligations, the
four reports, confidential contracts read as three people, and who may do what.

## 📂 Project layout

```
app.yaml                      the app's manifest (version, roles, shell entries, assistant notes)
Cargo.toml, build.rs          the Rust crate; build.rs embeds packages/ and ui/dist
src/
  lib.rs                      registers the packages, hooks, reports and the upkeep
  contract.rs                 submit, approve, send back, clauses, confidentiality rules
  sign.rs                     send for signature and follow the Sign request
  term.rs                     activation, renewal, expiry, notice and reminders
  reports.rs                  the four reports
packages/
  contracts/                  entities, roles, approval workflow, messages, templates, seed types
  contracts_parties|crm|selling|buying|projects|hr/   one link field each
ui/src/                       the interface at /contracts/ (home queues, locales ar and hi)
tests/contracts_it.rs         the year-of-contracts walk
docs/PROVENANCE.md            where the design comes from
```

## 📜 Changelog

Every release is described in [CHANGELOG.md](CHANGELOG.md).

## 🤝 Contributing

Issues and pull requests are welcome at [github.com/CBFerrum/ferrum-app-contracts/issues](https://github.com/CBFerrum/ferrum-app-contracts/issues).
Run the checks in [Develop](#-develop) first.

## 📄 License

AGPL-3.0-only. See [LICENSE](LICENSE).

<div align="center">

<sub>Part of <a href="https://github.com/CBFerrum/Ferrum">Ferrum</a>, the open business platform · Made with 🦀 Rust and 🐘 PostgreSQL</sub>

</div>
