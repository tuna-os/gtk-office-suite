# GTK Office Suite: Document Encryption & Digital Signature Specification

## Context & Objectives

As GTK Office Suite targets managed enterprise workstations, government compliance environments (Section 508 / EN 301 549), and privacy-conscious desktop users, robust document encryption, cryptographic signature verification, and centralized security policy enforcement are vital features.

This specification outlines the technical architecture for client-side cryptographic document protection across **Letters**, **Tables**, and **Decks**, building on pure Rust cryptographic primitives in `suite-common-core`.

---

## 1. Cryptographic Standards & Document Formats

### 1.1 OpenDocument Format (ODF 1.3 / ISO/IEC 26300)
- **Encryption**: Standard ODF package encryption using AES-256 GCM / CBC with Argon2id / PBKDF2 key derivation.
- **Digital Signatures**: W3C XMLDSig (XML Digital Signature) compliance for `.odt`, `.ods`, and `.odp` package manifests and content streams (`META-INF/documentsignatures.xml`).

### 1.2 PDF Archival & Signatures (PDF/A & ISO 32000-2)
- **PAdES (PDF Advanced Electronic Signatures)**: Support for ETSI TS 102 778 / PAdES-BES & PAdES-T signatures embedded within PDF document structures.
- **X.509 Certificate Validation**: Integration with system certificate stores (OpenSSL / GNet / PKCS#11 / NSS) for chain-of-trust verification and OCSP/CRL revocation checking.

---

## 2. Core Architecture (`suite-common-core`)

All cryptographic calculations, key derivation, and signature verification MUST reside in `suite-common-core` (GTK-free, pure Rust):

```
suite-common-core
├── src
│   ├── crypto
│   │   ├── cipher.rs        # AES-256-GCM package encryption & decryption
│   │   ├── kdf.rs           # PBKDF2 / Argon2id passphrase derivation
│   │   ├── xmldsig.rs       # ODF XMLDSig parsing, hashing, and RSA/ECDSA verification
│   │   └── pades.rs         # PDF PAdES signature container extraction & verification
│   └── policy
│       └── security.rs      # Enterprise security policy rules and dconf settings
```

---

## 3. Desktop UX & Security Policy Integration

### 3.1 UI Indicators & Banners
- **Password-Protected Documents**: Prompt for decryption passphrase upon opening; enforce zeroization of sensitive key material in memory (`zeroize` crate).
- **Signature Status Header Bar**:
  - 🟢 **Valid Signature**: Verified trusted signer certificate.
  - 🟡 **Untrusted / Self-Signed**: Certificate valid but CA not in system trust store.
  - 🔴 **Invalid / Tampered**: Hash mismatch or document modified post-signing.
- **Read-Only Enforced State**: Signed documents automatically lock editing controls unless explicit signature invalidation is acknowledged by the user.

### 3.2 Enterprise Fleet Policy (`dconf` integration)
Administrators can configure suite-wide security constraints via dconf keybindings under `/org/gnome/desktop/gtk-office/security/`:
- `require-valid-signature` (boolean): Warn or block opening documents with invalid signatures.
- `minimum-key-length` (integer): Disallow weak RSA/ECC key sizes.
- `allow-unencrypted-exports` (boolean): Mandatory encryption enforcement for sensitive export profiles.

---

## 4. Implementation Roadmap

1. **Phase 1 (Q4 2026)**: Integrate ODF package encryption & decryption in `suite-common-core` + password prompt dialogs in `suite-common`.
2. **Phase 2 (Q1 2027)**: Implement ODF XMLDSig and PDF PAdES signature verification engine and header bar status banner.
3. **Phase 3 (Q2 2027)**: Implement document signing workflow with GPG/PKCS#11 hardware token support and dconf security policy enforcement.

---
*Maintained by the strategist agent (tuna-os hive).*
