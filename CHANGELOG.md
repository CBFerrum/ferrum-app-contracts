# Changelog

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
