# Contracts

Contract lifecycle management for [Ferrum](https://github.com/CBFerrum/Ferrum):
- contracts drafted from a library of approved clauses;
- approval above a threshold, by someone other than the owner;
- signature through Sign, the counterparty first;
- activation, renewal, notice, termination and obligations.

| | |
|---|---|
| 📄 App | `contracts`, native, in the server |
| 🧩 Package | `contracts` (needs Sign) |
| 🖥️ Interface | its own, at `/contracts/`: contracts by stage, notice due before renewals, obligations, and the records |
| 🔐 Roles | Contract Manager, Contract Approver |
| 📜 Licence | AGPL-3.0-only |

## Highlights

- **The clause library.** Only a Contract Approver approves a clause for use, or changes an
  approved clause's wording. A contract keeps each clause's wording as it was when drafted.
- **Approval.** Submitted, a contract worth less than the threshold is approved. One worth more
  waits for a Contract Approver who does not own it, who approves it or sends it back saying why.
- **Terms are kept.** Out of its draft, a contract keeps its party, value, term, renewal, clauses
  and document.
- **Signature through Sign.** An approved contract with its PDF goes to Sign as a request: the
  counterparty signs first, then the organisation's signatory, each on the document's last page.
  Upkeep follows the request. Completed, the contract is signed with the sealed PDF. Declined,
  expired or voided, it is approved again, saying so.
- **The term.** A signed contract is active from its start. Past its end, it renews for its months
  when it says so, and expires otherwise. Notice falls due inside the notice period before a
  renewal. A termination says why.
- **Obligations.** Each is owned and due, overdue when late, and keeps the day it was done.

## Develop

```bash
cargo test                       # needs FERRUM_CONTRACTS_TEST_DATABASE_URL's database
cd ui && npm run build && npm run check
```

`tests/contracts_it.rs` walks a year of contracts with Sign installed:
- clauses approved, and their wording kept;
- contracts approved below the threshold and by another approver;
- one signed through Sign, one declined;
- activation, notice, renewal, expiry and termination;
- obligations;
- who may do what.

## Not yet

- Generating the contract's PDF from its clauses (it is attached), and redlining with the
  counterparty.
- Several approvers, or approval by contract type.
- Reminders to obligation owners by mail.
- A counterparty's portal.

## Changelog

| Version | Highlights |
|---|---|
| **0.1.3** | A signed contract keeps its own copy of the sealed PDF, which whoever reads the contract may open. |
| **0.1.2** | Rebuilt with Ferrum-Frontend 80ff179: labels name their fields, and linked titles show their titles. |
| **0.1.1** | Send for Signature only with the PDF, which the field says to attach before submitting. |
| **0.1.0** | Contracts from approved clauses, approval above a threshold, signature through Sign, activation, renewal, notice and obligations (FERRUM-ECOSYSTEM-008). |

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
