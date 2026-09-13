# Client-Side Document Encryption & Digital Signature Specification

This specification defines the cryptographic security and digital signature architecture for GTK Office Suite documents (Letters, Tables, Decks), establishing compliance requirements for enterprise document confidentiality and authenticity.

---

## 1. Objectives

1. **Client-Side Zero-Trust Encryption**: Support AES-GCM (256-bit) and Argon2id key derivation for standard OpenDocument Format (ODF ISO/IEC 26300) encrypted streams.
2. **Cryptographic Signatures**: Provide X.509 certificate validation and OpenPGP/GnuPG signature verification for document content streams.
3. **Decoupled Security Core**: Implement cryptography and security policy in `suite-common-core` (pure Rust, GTK-free) for deterministic unit testing and headless processing.
4. **Policy Configuration**: Support enterprise dconf keyfile overrides for requiring digital signatures prior to export.

---

## 2. Architecture & Crate Boundaries

Security logic is integrated cleanly into the GTK Office Suite crate ecosystem:

- **`suite-common-core::security`**: Contains pure-Rust cryptographic primitives, stream cipher wrappers, digest verification, and X.509 certificate parsing.
- **`suite-common::dialogs::security`**: GTK4/libadwaita modal dialogs for password entry, certificate selection, and digital signature status display.
- **`letters`, `tables`, `decks`**: Wire application export and save signals to enforce document encryption and signature verification policies.

---

## 3. Threat Model & Safeguards

| Threat | Safeguard | Implementation Boundary |
|---|---|---|
| Unauthorized access to saved files | Argon2id KDF + AES-256-GCM stream encryption | `suite-common-core::security::crypto` |
| Document tampering in transit | Cryptographic SHA-256 manifest digest & X.509 signature | `suite-common-core::security::signature` |
| Key exposure in memory | Zeroize buffer allocations on drop (`zeroize` crate) | `suite-common-core::security::keys` |

---

## 4. Quality Gates & Verification

- **Unit Testing**: 100% test coverage of encryption, decryption, and signature verification in `suite-common-core`.
- **Interop Testing**: Verified compatibility against LibreOffice ODF encryption fixtures (`REQUIRE_SOFFICE=1 cargo test`).
