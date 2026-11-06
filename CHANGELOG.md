# Changelog

## 0.1.12

Rebuilt with Ferrum-Frontend 63280d3: right to left, its pages mirror as they read (icons that point along the line, margins and alignment by the line's start and end, arrow keys along it), and the records pages' words are in Arabic and Hindi.

## 0.1.11

Rebuilt with Ferrum-Frontend a64416a: a new record's box whose default is 1 shows ticked, as it is saved.

## 0.1.10

Rebuilt with Ferrum-Frontend 922fcfb: an action's dialog says every refusal, the errors on what the action made (another record's rows) among them, and an action with parameters and a confirmation asks once.

## 0.1.9

Rebuilt with Ferrum-Frontend a76295e: its pages show times on a person's own clock when they chose one on their Account page, while today and due dates stay on the workspace's day.

## 0.1.8

Rebuilt with Ferrum-Frontend 24c3951: its pages show times on the workspace's clock, naming the zone when it is not the browser's, a draft numbered on submit is headed as a draft, and a record of an app that is not installed says so.

## 0.1.7

Rebuilt with Ferrum-Frontend bb60115: a record page reads an empty number or check as the server does, so an action whose condition needs one shows, and a workspace's settings open as their one record.

## 0.1.6

Rebuilt with Ferrum-Frontend 00dea07: in a workflow state that names what may change, the other fields show read-only, and the fields the state needs show required.

## 0.1.5

Personal-data requests find the contracts a person is the counterparty of, by email or name; they are kept when erasure is asked, as contract records, and the answer says why (Ferrum's FERRUM-PRIVACY-002).

## 0.1.4

Rebuilt with Ferrum-Frontend daeadda: after an action runs, a record page shows what the action said it did rather than only that it is done.

## 0.1.3

From the browser walk of a contract through Sign.

- A signed contract keeps a copy of the sealed PDF attached to itself. It linked Sign's own, which
  is kept with Sign's audit and read only by who may read Sign's records, so an approver who could
  read the contract was refused its signed document ("not permitted to read signevent").
- Rebuilt with Ferrum-Frontend 16b5c57.

## 0.1.2

Rebuilt with Ferrum-Frontend 80ff179: a field's label names its control for assistive technology, and a record whose title is a link shows that record's title, not its id.

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
