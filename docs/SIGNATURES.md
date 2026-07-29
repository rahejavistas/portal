# E-Signatures: How It Actually Works

This is a plain-language walkthrough of the signing feature — who can sign, how a
document gets sent out, and how we find out it's been signed. See `docs/API.md` for
the actual endpoints; this doc is about the *flow*, not the wire format.

## Who can sign?

Being a "signer" is opt-in and self-service. It works like this:

1. There's no special permission for this — anyone holding **any** designation at all
   (not a plain resident with none) is eligible. That doesn't make anyone a signer
   yet, it just makes them eligible.
2. Someone who's eligible goes to their own **Profile page** and fills in their contact
   details: name, phone, email. Only they can do this — there's no way for an admin to
   fill this in on someone's behalf.
3. Once submitted, they show up in the "signer directory" — the list a process
   reviewer picks from when attaching signers to a document.
4. Their name is locked after that first submission (it's the name that'll appear on
   signed paperwork, so it shouldn't be casually editable), but phone and email can be
   updated any time.

So the flow is: **hold any designation → the actual person fills in their own info →
they become pickable.** Nobody is ever registered as a signer without doing it
themselves, and nobody — not a reviewer, not even super_admin — can edit *or even
view* another person's registered phone/email. The picker a reviewer uses only ever
shows a name and login email, enough to pick the right person without browsing their
private contact details; the phone/email that actually go to the signing provider stay
visible only to the individual themselves, on their own Profile page.

## Attaching signers to a document

When a resident uploads a document into a process (say, an NOC), a process reviewer
can open that document and attach one or more signers to it, **in the order they
should sign**. If two signers are attached, the second one can't do anything until the
first one has finished — the queue simply won't show it to them yet.

A document isn't considered "done" until every attached signer has signed. Until then,
the process step it belongs to stays blocked.

### Restricting *who* and *in what order* — designation requirements

A template field can optionally declare a list of designations, in order — e.g.
"Staff, then Secretary." This does two things when a reviewer opens "Add Signers" for
that field:

- The picker only offers people who currently hold one of those designations, so a
  reviewer can't accidentally attach the wrong person.
- Whoever covers "Staff" is always ordered before whoever covers "Secretary,"
  regardless of the order they were clicked in the picker.

This is checked again on the server, not just trusted from the picker — attaching a
signer who doesn't hold the right designation, or attaching them out of order, is
rejected outright.

It's also re-checked at the moment of signing, not just when they're attached. If
someone is attached as (say) the "Staff" signer and then loses that designation before
they actually sign, their pending signature becomes invalid — they can no longer
complete it. This matters because designations can change after a document is already
out for signature (someone moves teams, is offboarded, etc.), and a stale signature
from someone no longer in the right role shouldn't count.

There is deliberately no way to cancel or remove a signature requirement once it's
attached — a required signature is required, full stop. So if a signer permanently
loses the designation a field requires before they sign, that requirement has no
resolution short of restoring their designation. Get the signer selection right at
attach time.

A field with no designation requirement behaves exactly as before: any eligible signer
can be attached, in whatever order the reviewer clicks them.

## Two ways to actually get a signature

There are two independent paths, and which one is used is a per-attachment choice:

### 1. Manual

The simplest path, and the default. The signer opens their signing queue, downloads
the document, signs it themselves (physically or however they normally would), and
uploads the signed copy back through the portal. We treat that upload as proof of
signature. Nothing leaves our system.

### 2. Through an external provider (SignYu)

This is where it gets more interesting, and where "using someone else's API to decide
things for ourselves" comes in.

**SignYu** is a third-party Aadhaar e-signature service. When a reviewer picks SignYu
instead of manual, here's what happens:

1. We take the document and hand it to SignYu, along with the list of signers (their
   name, phone, email) *in the order they need to sign*.
2. SignYu takes it from there — they email each signer a link, in turn, and handle the
   actual Aadhaar OTP identity verification and signing on their own website. We are
   not involved in that part at all; we don't see the OTP, we don't see the signing
   screen, nothing. SignYu also hands that same signing link back to us in their
   response, so we show an "Open Signing Page" button directly in our own signing
   queue — a signer doesn't have to go dig through their email to find it.
3. At some point, the document is fully signed by everyone. SignYu now has the final,
   signed PDF sitting on their servers.

The important thing to understand: **SignYu doesn't have access to our database.**
It can't reach into our system and mark something as "signed" for us. All it can do is
answer a question when we ask it, or ping us to say "something changed, go check."

So we do the checking. Either:
- An admin clicks "Refresh Status" on the document, or
- SignYu pings a webhook URL of ours the moment someone signs, or the moment the whole
  document is done — a background notification we treat purely as a nudge to "go check
  now"; we don't trust anything in the ping's own body, even though we do verify it's
  genuinely from SignYu (see below).

Every webhook ping is cryptographically signed — SignYu computes an HMAC using a
secret only we and SignYu know, and we check that signature before doing anything else
with the request. A ping that doesn't check out (wrong signature, or missing entirely)
is rejected outright, so nobody can forge a "your document is signed" notification.

Either way, we then ask SignYu directly: "what's the status of this document?" SignYu
tells us whether it's still in progress or fully completed, and — once it's
completed — gives us a link to download the final signed PDF.

Where does that "download the final PDF" link actually point? SignYu stores the
signed file in **their own** S3 storage, and hands us a temporary, pre-authorized link
straight into it (the same "presigned URL" trick this app uses for its own uploads —
the link itself is the permission slip, no login needed to fetch it, and it stops
working after a while). We just do a plain download from that link — nothing
SignYu-specific about the download itself.

Once we hear "it's done," **we** do the actual work:
- Download the signed PDF from that link and save it as a brand-new document in **our
  own** storage (the original, unsigned document is never touched or overwritten).
  From this point on we no longer depend on SignYu's copy or their link — it's ours
  permanently, the same as any other document in the portal.
- Mark every signer's requirement as complete on our side.
- Unblock the process step so the resident's process can move forward.

In other words: SignYu is a source of *information* we go and ask, not a system that
writes to our database. Every single thing that changes on our side — a requirement
going from "pending" to "completed," the process unblocking — is us reacting to what
SignYu told us, not SignYu doing it directly.

## How we keep our data and SignYu's data straight

We and SignYu each have our own identifiers for the same things, and we keep a small
mapping table so we always know which is which:

| What | Our identifier | SignYu's identifier |
|---|---|---|
| The document being signed | our document's UUID | SignYu's own "documentId" |
| Each signer | our own user account (and their entry in the signer directory) | SignYu's own "signerId" for that person on that document |

Every time we send something to SignYu, we get their id back and store it alongside
ours. Later, when we ask SignYu for a status update, we're asking about *their* id —
and when we get an answer back, we translate it back to *our* records using that same
mapping. This is deliberately kept in its own small table rather than mixed into the
main signature data, so that if we ever add a second provider, it gets its own mapping
table too, without touching anything provider-agnostic in the core signature schema.

## The audit trail

There's one log, not several: a raw record of every single request/response we
exchanged with an external provider like SignYu — the exact bytes sent and received,
success or failure. That's purely for debugging the integration if something ever goes
wrong with SignYu specifically.

There's no separate "what happened to this requirement" event log on top of that — a
requirement only ever has two states (`PENDING`, then `COMPLETED`) and no way to be
cancelled or reassigned, so `document_signature_requirements` itself already tells the
whole story: when it was created (`created_at`), and if signed, by whom and when
(`signed_by`, `signed_at`). A separate history table would just duplicate one of these
two sources, so we don't have one.

## Adding a different provider later

The design deliberately keeps "SignYu-specific" details out of the shared tables:
provider-specific correlation ids live in their own table, and the shared signature
data has no SignYu-only columns. Wiring up a second provider means writing a new small
client module and one new mapping table for it — the rest of the system (who can sign,
how signers get attached, how the process reacts once something's signed) doesn't
change.
