# Document encryption and digital signatures

**Status**: specification draft, not accepted · Consolidates #495, #711 (and the
security goals in #626)

## Scope

Three separable things, which the drafts partly conflate:

1. **Encryption** — opening and saving password-protected documents.
2. **Signatures** — creating and verifying digital signatures on documents.
3. **Remote storage** — WebDAV/Nextcloud open and save (#495 only).

Only the first two are security features. Remote storage is a file-access
feature that #495 bundles in; it is listed here so it is not lost, but it
should be planned separately and is not discussed further.

## Encryption

| Format | Scheme | KDF |
|---|---|---|
| ODF (`.odt`, `.ods`, `.odp`) | AES-256-GCM per the ODF 1.3 encryption spec | Argon2id (PBKDF2 for older documents) |
| OOXML (`.docx`, `.xlsx`, `.pptx`) | Office "agile encryption", AES-256-CBC | PBKDF2-HMAC-SHA512 |

Both drafts agree on these, and they are determined by the file formats rather
than chosen by us — the only real decision is whether OOXML encryption is in
scope at all for a first pass.

Two requirements that matter more than the algorithm choice:

- **No cleartext temporary files.** Decryption streams into memory; the
  archive is never unpacked to disk in the clear. Easy to get wrong, since the
  obvious zip-handling code does exactly that.
- **Key material is zeroized on drop** (the `zeroize` crate).

Reading an encrypted document that was saved by LibreOffice, and vice versa, is
the acceptance test. `REQUIRE_SOFFICE=1` interop fixtures already exist for
this kind of check.

## Signatures

Verification and creation of XMLDSig/XAdES signatures, stored where the formats
put them: `META-INF/documentsignatures.xml` for ODF, `_xmlsignatures/` for
OOXML. Signing keys come from the system GnuPG keyring or a PKCS#11 token.

Signature state belongs in the UI as a plain statement of fact — signed and
valid, signed but modified, unsigned — and the failure case is the one that
has to be unmistakable.

## Where the code goes

`suite-common-core` holds the cryptography, certificate parsing and policy: pure
Rust, GTK-free, unit-testable headless. `suite-common` holds the dialogs —
password entry, certificate selection, signature status. The apps wire save and
export to the policy. This follows ADR-0001 and both drafts state it.

## Open questions

1. **Is OOXML encryption in scope?** Two formats, two schemes, roughly twice
   the work and twice the attack surface. ODF alone is defensible for a first
   pass.
2. **Which crates?** Neither draft names the Rust crates for Argon2id, AES-GCM,
   X.509 or XMLDSig. XMLDSig in particular has no obvious mature Rust
   implementation, and that — not the symmetric crypto — is the part likely to
   determine whether signatures are feasible at all. Settle this before
   committing to signatures.
3. **"100% test coverage" of the crypto** (#711) is an assertion, not a plan.
   Coverage of what: the happy path, or wrong-password, truncated-stream,
   tampered-manifest and unsupported-KDF? The negative cases are the ones that
   matter.
4. **Enterprise policy for required signatures** (#711) depends on settings
   keys that do not exist. See the enterprise deployment consolidation.
5. **Threat model.** #711 has a table that names three threats. What this
   feature does *not* defend against — a compromised endpoint, a keylogger, an
   attacker with the passphrase — should be written down too, because document
   passwords are routinely oversold.

## Relationship to the readiness plan

Behind [#443]. Nothing here is started while save correctness and crash safety
are open — an encryption bug loses a document permanently rather than
temporarily.

[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
