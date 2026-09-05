# Document Security, Digital Signatures, and Encryption Architecture

**Author**: Strategist Agent | **Status**: Strategy / Draft | **Date**: 2026-09-05

---

## Executive Summary

As **gtk-office-suite** (Letters, Tables, Decks) transitions toward enterprise deployment, government document exchange, and privacy-preserving daily-driver workflows, document-level security is a strategic prerequisite. Standardizing cryptographic operations—specifically ODF XML digital signatures (W3C XML Signature), AES-256 GCM encrypted document containers, key management, and trust verification—ensures that sensitive documents remain secure, authentic, and tamper-evident across desktop and cloud storage environments.

---

## Strategic Goals

1. **ODF & OOXML Signature Parity**: Full verification and creation of digital signatures conforming to ODF 1.3 / ISO/IEC 26300 and OOXML ISO/IEC 29500 standards.
2. **Container Encryption**: Support for password-protected ODF (`.odt`, `.ods`, `.odp`) and OOXML (`.docx`, `.xlsx`, `.pptx`) packages with PBKDF2/Argon2id key derivation and AES-256-GCM encryption.
3. **Pure-Rust GTK-Free Security Model**: Encapsulate cryptographic primitives, hashing, payload parsing, and validation strictly inside `suite-common-core` (or a dedicated `suite-security` crate) to maintain unit testability without GTK/GObject runtime requirements.
4. **Desktop & System Integration**: Seamless integration with standard desktop keyrings (Secret Service API via `libsecret`/GTK portals) and hardware key tokens (PKCS#11 / YubiKey / PGP / GnuPG).

---

## Architectural Layout

```
+-------------------------------------------------------------------+
|               GTK4 / Libadwaita Application Layer                 |
|   (Letters / Tables / Decks - UI dialogs, password prompts, icons)|
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                     suite-common (GTK Helpers)                    |
|   (Password entry dialogs, certificate picker, signature toasts)  |
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                  suite-common-core (GTK-Free Engine)              |
|  +---------------------------+   +-----------------------------+  |
|  |     Document Crypto       |   |      Digital Signatures     |  |
|  | - Package Encryption/Dec  |   | - Manifest XML Signature    |  |
|  | - AES-256 GCM / PBKDF2    |   | - Canonical XML (C14N)      |  |
|  | - Salt & Key Derivation   |   | - Certificate Chain Trust   |  |
|  +---------------------------+   +-----------------------------+  |
+-------------------------------------------------------------------+
```

---

## Core Specification & Implementation Phases

### Phase 1: Cryptographic Container Engine (`suite-common-core`)

- **Container Encryption**: Implement ODF package stream encryption parsing (`META-INF/manifest.xml` encryption elements).
- **Key Derivation**: Support standard PBKDF2-SHA256 and Argon2id key derivation.
- **Payload Cipher**: Implement stream/block decryption for zipped XML content using AES-256-CBC and AES-256-GCM.
- **Verification Bench**: Pure-Rust unit tests evaluating synthetic encrypted fixtures against LibreOffice interop baselines (`REQUIRE_SOFFICE=1 cargo test`).

### Phase 2: XML Digital Signature Engine

- **Canonicalization (C14N)**: Standardized XML canonicalization for manifest entries and content documents (`content.xml`, `styles.xml`).
- **Digest & Signing**: SHA-256 digest creation and RSA / ECDSA signature verification.
- **Trust Anchor Integration**: Certificate validation against system trust stores (`/etc/ssl/certs` / WebPKI).

### Phase 3: UX & Desktop Integration

- **UI Status Surface**: Non-modal signature status indicators in libadwaita headerbars (Valid, Warning/Untrusted, Tampered).
- **Password & Key Prompting**: Secure GTK entry dialogs utilizing memory zeroing (`zeroize` crate) for sensitive passphrase handling.
- **Secret Service Portal**: Optional integration with GNOME Keyring / Secret Service portal for session password caching.

---

## Strategic Roadmap Alignment

| Milestone | Target Horizon | Deliverable |
|-----------|----------------|-------------|
| **M1: Encryption Engine Core** | Q4 2026 | Pure-Rust ODF decryption/encryption in `suite-common-core` with unit tests. |
| **M2: Signature Verification** | Q1 2027 | XML C14N & signature verification pipeline with LibreOffice parity test suites. |
| **M3: UI & Token Integration** | Q1 2027 | Libadwaita dialogs, PKCS#11 token selection, and Flathub portal key storage. |

---

## Verification & Interoperability Compliance

- **LibreOffice Parity**: Test suite targets 100% interoperability when opening password-protected or signed documents produced by LibreOffice 24.x/25.x.
- **Security Audit**: Codebase strict adherence to no-panic Rust safety, memory zeroing (`zeroize`), and formal fuzzing targets in `fuzz/fuzz_targets/document_crypto.rs`.
