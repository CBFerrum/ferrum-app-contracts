# Changelog

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
