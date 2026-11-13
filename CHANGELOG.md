# Changelog

## 0.3.1

* **A page cannot be sent for signature, and the refusal says why.** Without a PDF service a typed contract's body is
  kept as a page (HTML). Send for Signature now reads the file and refuses anything that is not a PDF, whatever
  its content type says (a page was served as a plain download, so the earlier check by type missed it and Sign
  answered "the file is not a PDF" later). The message names the missing service, `FERRUM_PDF_URL`. The contract stays
  Approved with its page as the document, and no request is made. Approving and keeping the page work as before.

## 0.3.0

Closes the documents audit's tier-1 rows for Contracts (DC-57, 58, 63, 65, 66, 67, 69, 70, 71, 74, 75, 77).

* **A body from a contract type.** New Contract Type (template, clauses with a mandatory mark, renewal as New
  Version or New Contract); two generic types and their document templates (`contracts.services_agreement`,
  `contracts.non_disclosure`) seeded and editable. A contract of a type starts with its clauses; Submit It
  refuses one missing a mandatory clause (or its fallback), freezes the body as a kept file (Document, Document
  SHA-256) and lists it under Versions with who, when and why. Body Text keeps its words for search.
* **Clauses** take merge fields (`{{ doc.<field> }}`) filled when drafted, and a fallback clause.
* **Parties.** Signatories (name, email, role, order) and Readers as lines; Department; connector packages
  linking a contract to a customer, supplier, contact, project, opportunity, sales order, purchase order or
  employee where those are installed.
* **Approval by policy.** The package's workflow Contract Approval: Submit It takes `Approve`, whose default
  policy asks a Contract Approver (not the submitter) at the threshold or more; its completion runs the hidden
  action Approval Granted, which binds the approval as before. Rejected or recalled, upkeep sends it back to its
  draft with the reason. Without an `Approve` transition, the single approver as before.
* **Signature** goes to the signatories in their order, boxes on the signature page; Record Signed Copy records
  one signed elsewhere (Signed Elsewhere, Signed Elsewhere By).
* **Term.** Renewal Notice On (computed), Renew It (Previous, Renewed By), reminders once each with three
  message templates and Contract Settings' Reminder Days (14); upkeep locks the rows it moves.
* **Obligations** have a kind (Obligation, Milestone) and a side (Us, Counterparty).
* **Reports**: Contracts by State, Contracts Ending or Renewing, Obligations Due, Days from Draft to Signed.
* **Confidential** contracts read by their owner, readers and the new Legal role; Contract Manager may import.
  Existing contracts are marked not confidential by a backfill.

## 0.2.2

Rebuilt with Ferrum-Frontend 9e7190c, the design system as tuned after the landed build was reviewed. The left menu's icons are chosen so that none repeats on more than three rows and each fits its row; their drawings load beside the first page instead of inside it, so the page starts lighter; a row that scrolls sideways fades at the edge where there is more, in every browser and with reduced motion; a printed record leaves out empty fields and keeps its totals with the table; menus and popovers sit inside the page's landmarks for assistive technology; a selection bar folds what does not fit into More on a narrow screen. Nothing else changes.

## 0.2.1

The interface follows the upgraded design system, rebuilt with Ferrum-Frontend 76c4fad. Text uses the kit's type roles (nothing under 12px) and controls its height tokens; loading shows skeletons shaped like what is coming; empty, no matches, denied, offline and failed are told apart; status colours come from stored state ids. The interface check now runs the UI lint against an empty baseline. The home is rebuilt on the kit's work-home pattern: one page title, the main action as a button, queues with their counts, status at its natural width; its other own pages follow the same anatomy.

## 0.2.0

An approval is bound to the exact wording and PDF it was given (Ferrum's closure review, FEO-038). Approving a contract records Approved As: the SHA-256 of its terms, each clause's wording as drafted and its PDF as stored. A contract that has a PDF is approved only by someone who says they read it against the clauses (The PDF Says What The Clauses Say), named as PDF Checked By; one approved below the threshold names no one. It goes for signature only while its digest is still the one approved, and the request Sign sends names it. Terms were already kept once a contract left its draft.

## 0.1.17

Rebuilt with Ferrum-Frontend 43d7779: the names of its models, fields and manifest in Arabic and Hindi (92 each in ui/src/locales/names, which the platform serves to every page of the workspace, so a record's labels, the launcher and the bar read in the person's language), and the sentences its pages still showed in English (split around a link or a figure, built in code from a template or word by word) whole and translated (11 words each in ui/src/locales). Translated by the model, to be reviewed by native speakers.

## 0.1.16

The assistant knows what its records mean in its pages (contributes.shell.assistant): their states, what counts and where figures come from, with questions it suggests asking there.

## 0.1.15

Rebuilt with Ferrum-Frontend 389b2af: its own words carry built-in Arabic and Hindi (10 words each, ui/src/locales, translated by the model and to be reviewed by native speakers), fetched for the page's language only. The labels it keeps in objects, its dialogs, its page titles and the words it keeps in variables go through t() as well, and the names and sizes it shows as they are keep their order right to left. Its pages are drawn once their words are in, and its entry carries only the parts of the kit it uses.

## 0.1.14

Rebuilt with Ferrum-Frontend 1f5981d: an action's link, code or token shown once is kept in a dialog to copy, and on a page that reads right to left, English left untranslated keeps its own order. Its own words go through the kit's t() (9 places), so a workspace's own words for its language reach them, and Admin, Interface words lists them under it (dist/words.json).

## 0.1.13

Rebuilt with Ferrum-Frontend 8a1c2e7: a workspace's own words for its interface (Admin, Interface words) reach its pages as they load; an attachment field that holds a picture shows it; and a value another record already has is said so, where the form said the record had been changed by someone else.

## 0.1.12

Rebuilt with Ferrum-Frontend 9d80f4f: right to left, its pages mirror as they read (icons that point along the line, margins and alignment by the line's start and end, arrow keys along it), and the records pages' words are in Arabic and Hindi.

## 0.1.11

Rebuilt with Ferrum-Frontend 204b713: a new record's box whose default is 1 shows ticked, as it is saved.

## 0.1.10

Rebuilt with Ferrum-Frontend 6cfda39: an action's dialog says every refusal, the errors on what the action made (another record's rows) among them, and an action with parameters and a confirmation asks once.

## 0.1.9

Rebuilt with Ferrum-Frontend b39fffb: its pages show times on a person's own clock when they chose one on their Account page, while today and due dates stay on the workspace's day.

## 0.1.8

Rebuilt with Ferrum-Frontend d350579: its pages show times on the workspace's clock, naming the zone when it is not the browser's, a draft numbered on submit is headed as a draft, and a record of an app that is not installed says so.

## 0.1.7

Rebuilt with Ferrum-Frontend 20d08e5: a record page reads an empty number or check as the server does, so an action whose condition needs one shows, and a workspace's settings open as their one record.

## 0.1.6

Rebuilt with Ferrum-Frontend ae13c69: in a workflow state that names what may change, the other fields show read-only, and the fields the state needs show required.

## 0.1.5

Personal-data requests find the contracts a person is the counterparty of, by email or name; they are kept when erasure is asked, as contract records, and the answer says why (Ferrum's FERRUM-PRIVACY-002).

## 0.1.4

Rebuilt with Ferrum-Frontend daeadda: after an action runs, a record page shows what the action said it did rather than only that it is done.

## 0.1.3

From the browser walk of a contract through Sign.

- A signed contract keeps a copy of the sealed PDF attached to itself. It linked Sign's own, which
  is kept with Sign's audit and read only by who may read Sign's records, so an approver who could
  read the contract was refused its signed document ("not permitted to read signevent").
- Rebuilt with Ferrum-Frontend 355dfca.

## 0.1.2

Rebuilt with Ferrum-Frontend 89b7cc7: a field's label names its control for assistive technology, and a record whose title is a link shows that record's title, not its id.

## 0.1.1

From a browser walk of a contract to its signature.

- Send for Signature is offered only for a contract with its PDF, which is part of its terms and attached before it is submitted, as the field now says: an approved contract without one was offered it and then refused.

## 0.1.0

Contract lifecycle management (FERRUM-ECOSYSTEM-008).

- A clause library approved by Contract Approvers; contracts keep each clause's wording as drafted.
- Approval above a threshold by someone other than the owner; contracts sent back saying why;
  terms kept once out of the draft.
- Signature through Sign, the counterparty first, followed to its signature or back to approved.
- Activation from the start, renewal or expiry at the end, notice before renewal, termination.
- Obligations, overdue when late.
- Its own interface at `/contracts/`.
