# Contracts

Contract lifecycle management for [Ferrum](https://github.com/CBFerrum/Ferrum):
- contracts of a type, drafted from a library of approved clauses, their body made from a document template;
- approval through the workspace's approval policy (by value, type or department, in order or in parallel);
- signature through Sign, to the contract's signatories in their order, or a copy signed elsewhere recorded;
- activation, renewal, notice, termination, obligations and reminders; reports; confidential contracts.

| | |
|---|---|
| 📄 App | `contracts`, native, in the server |
| 🧩 Package | `contracts` (needs Sign); connectors `contracts_parties`, `contracts_projects`, `contracts_crm`, `contracts_selling`, `contracts_buying`, `contracts_hr` |
| 🖥️ Interface | its own, at `/contracts/`: contracts by stage, notice due before renewals, obligations, and the records |
| 🔐 Roles | Contract Manager, Contract Approver, Legal |
| 📜 Licence | AGPL-3.0-only |

## Highlights

- **Contract types and their body.** A Contract Type names a document template (Settings, Documents), its
  clauses (some mandatory) and how it renews. The package ships two generic ones that name no country or law:
  Services Agreement (`contracts.services_agreement`) and Non-Disclosure Agreement (`contracts.non_disclosure`),
  which a workspace edits. While a contract is a draft its body is the template rendered live
  (`GET /api/documents/<template>/render/<contract id>`, or Print with that template). Each Submit It freezes
  it as a file kept on the contract, never replaced: Document points at it, Document SHA-256 is its digest, and
  Versions lists each one with who and when (submitted, submitted again after being sent back, renewed). A
  contract with no type keeps an attached PDF, as before.
- **The clause library.** A clause's text may hold `{{ doc.<field> }}` of the contract (`counterparty_name`,
  `starts_on`, `value`...), filled when it is drafted; a clause may name a fallback that stands in for it where
  its type makes it mandatory. Only a Contract Approver approves a clause for use, or changes an approved
  clause's wording. A contract keeps each clause's wording as it was when drafted: a changed library clause does
  not change it.
- **Parties as records.** Where the shared customers and suppliers are installed, `contracts_parties` adds a
  contract's Customer, Supplier and Contact; `contracts_projects` its Project; `contracts_crm` its Opportunity;
  `contracts_selling` its Sales Order; `contracts_buying` its Purchase Order; `contracts_hr` its Employee. The
  typed counterparty stays. "Contracts for this record" is the Contract list filtered by that link
  (`/contracts/e/Contract?filters=[["pty_customer","=","<id>"]]`).
- **Approval by policy.** Submit It takes the `Approve` transition of the package's workflow, Contract Approval,
  whose default policy asks one Contract Approver other than whoever submitted it when the contract is worth
  Contract Settings' threshold or more, reminds after two days, and goes through at once otherwise. A workspace
  replaces it with its own workflow on Contract (Customize, workflow) keeping a transition named `Approve` from
  its first state, with steps by value band (`when: doc.value >= 100000`), by type (`doc.kind == '<id>'`), by
  department (`doc.department == 'Legal'`), in order or together (`with_next`), N of M, stand-ins and escalation
  (docs/api.md, "Approvals on a transition"). Approvers decide under Approvals; a rejection sends the contract
  back to its draft with the reason at the next upkeep. A workspace whose workflow has no `Approve` gets the
  single approver this app had before (Approve It, Send It Back).
- **Terms are kept.** Out of its draft, a contract keeps its party, type, value, term, renewal, clauses and
  document; the approval is bound to their digest and to its PDF's.
- **Signature through Sign.** An approved contract goes to Sign to its signatories in their order (equal orders
  sign together); one listing none goes to the counterparty, then Contract Settings' signatory. Each signatory's
  box is on the last page, the signature page the packaged templates end with: two columns, 72 and 320 points
  from the left, rows 110 points apart from 150 points down, at most twelve. (A contract with no type and no
  signatories keeps the two boxes it always had.) Upkeep follows the request: completed, the contract is signed
  with the sealed PDF; declined, expired or voided, it is approved again, saying so. A server without a PDF
  service keeps the body as a page, and refuses to send it for signature, saying so.
- **Signed elsewhere.** Record Signed Copy takes the uploaded signed PDF, who signed and when: the contract is
  Signed, marked Signed Elsewhere, and keeps its own copy. From a draft (an import), only a Contract Approver or
  Legal records it.
- **The term.** A signed contract is active from its start. Past its end, it renews for its months when it
  says so, and expires otherwise, once. Renewal Notice On is its last day to give notice. Renew It makes the
  next term: on the same contract with its body as a new version, or as a new draft contract linked both ways
  (Previous, Renewed By), as its type says.
- **Obligations and reminders.** An obligation (or a milestone) has an owner, a side and a due date, and is
  overdue when late. Contract Settings' Reminder Days before an obligation is due, before a self-renewing
  contract's notice date and before a non-renewing contract ends, the owners are told once, with the templates
  `contracts.obligation_due`, `contracts.renewal_notice` and `contracts.ending` (Settings, Messages; they read
  only the record). Two upkeeps at once send each once: each step locks its rows and records what it sent in the
  same transaction.
- **Reports.** Contracts by State (or Type, Owner, Category) with their value; Contracts Ending or Renewing in a
  period; Obligations Due; Days from Draft to Signed.
- **Search.** A contract is found by its counterparty and by the words of its latest body, kept in Body Text
  when it is frozen. Text inside an uploaded PDF is not searched: the platform does not extract it.
- **Confidential contracts.** A confidential contract is read only by its owner, its readers and Legal (and,
  while their decision is awaited, its approvers). Contract Manager and Contract Approver read every other one.
- **Importing contracts.** Import a CSV of contracts as drafts (Contract Manager or Legal: title, counterparty,
  dates, value, type, `confidential`), then upload each signed copy and Record Signed Copy on it.

## Develop

```bash
cargo test                       # needs FERRUM_CONTRACTS_TEST_DATABASE_URL's database
cd ui && npm run build && npm run check
```

`tests/contracts_it.rs` walks a year of contracts with Sign installed and a stand-in PDF service:
- clauses approved, their wording kept and merge fields filled; a type's mandatory clause or its fallback;
- contracts approved at once below the threshold, by another approver above it, sent back with a reason;
- a type's body frozen at submit, its digest kept, found by its words; renewed as a new version or contract;
- one signed through Sign to three signatories in order, one declined, one signed on paper and recorded;
- activation, notice, renewal, expiry once, termination; reminders once each from two upkeeps at once;
- obligations; the four reports; a confidential contract read as three people;
- a workspace with no approval policy falls back to the single approver; who may do what.

## Not yet

- Redlining with the counterparty, and a counterparty's portal.
- Searching the text of an uploaded PDF (the platform extracts none).
- Following a Sign request by its event rather than at upkeep; an approval's rejection is seen at upkeep too.
- Signature boxes placed by marks in an uploaded PDF (the signature page of the packaged templates places them).

## Changelog

| Version | Highlights |
|---|---|
| **0.3.0** | Contract types whose body is generated and kept by version; clauses with merge fields and fallbacks; parties and governed records as links; approval by policy; signatories in order; signed-elsewhere copies; renewals as versions or new contracts; reminders; reports; confidential contracts. |
| **0.2.2** | Rebuilt on the tuned design system: better menu icons, a lighter first page, a visible scroll cue, cleaner print. |
| **0.2.1** | The redesigned interface: the upgraded design system, shaped loading, told-apart states. |
| **0.2.0** | An approval is bound to the wording and PDF it was given: a contract goes for signature only as approved, its PDF read against its clauses by its approver. |
| **0.1.17** | Its names in Arabic and Hindi; the last English sentences translated. |
| **0.1.16** | The assistant knows its records. |
| **0.1.15** | Rebuilt with Ferrum-Frontend 5b22e7b: its own words in Arabic and Hindi. |
| **0.1.14** | Rebuilt with Ferrum-Frontend 6537d65: once-shown answers kept; English isolated right to left; its own words translatable by a workspace. |
| **0.1.13** | Rebuilt with Ferrum-Frontend d5d98f2: a workspace's own interface words; pictures in attachment fields; a duplicate value said so. |
| **0.1.12** | Rebuilt with Ferrum-Frontend 63280d3: right to left, pages mirror; the records pages' words in Arabic and Hindi. |
| **0.1.11** | Rebuilt with Ferrum-Frontend a64416a: a box whose default is 1 shows ticked. |
| **0.1.10** | Rebuilt with Ferrum-Frontend 922fcfb: action dialogs say every refusal and ask once. |
| **0.1.9** | Rebuilt with Ferrum-Frontend a76295e: times on a person's own clock when they chose one. |
| **0.1.8** | Rebuilt with Ferrum-Frontend 24c3951: times on the workspace's clock; drafts headed as drafts. |
| **0.1.7** | Rebuilt with Ferrum-Frontend bb60115: empty numbers read as the server reads them; settings open as their record. |
| **0.1.6** | Rebuilt with Ferrum-Frontend 00dea07: a workflow state's kept fields are read-only, its needed ones required. |
| **0.1.5** | Personal-data requests find a counterparty's contracts, kept as contract records. |
| **0.1.4** | Rebuilt with Ferrum-Frontend daeadda: actions say what they did. |
| **0.1.3** | A signed contract keeps its own copy of the sealed PDF, which whoever reads the contract may open. |
| **0.1.2** | Rebuilt with Ferrum-Frontend 80ff179: labels name their fields, and linked titles show their titles. |
| **0.1.1** | Send for Signature only with the PDF, which the field says to attach before submitting. |
| **0.1.0** | Contracts from approved clauses, approval above a threshold, signature through Sign, activation, renewal, notice and obligations (FERRUM-ECOSYSTEM-008). |

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
