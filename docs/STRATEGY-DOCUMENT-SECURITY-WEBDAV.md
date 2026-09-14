# Document Security, Digital Signatures, and WebDAV Integration Strategy

> Strategic Roadmap & Architecture Specification for Enterprise Fleet Deployment, Document Encryption, Digital Signatures, and Cloud Storage Interoperability.

---

## Executive Summary

To enable enterprise fleet deployment and privacy-focused organizational adoption of GTK Office Suite (Letters, Tables, Decks), built-in support for client-side document encryption, digital signatures, and direct cloud storage sync via WebDAV / Nextcloud is required. 

This specification outlines the strategic roadmap, technical requirements, standards compliance, and phased rollout plan for document security and remote storage integration.

---

## Strategic Goals

1. **Client-Side Document Encryption**: Support opening and saving password-protected documents using standard ODF (AES-256 GCM / Argon2) and OOXML (Agile Encryption) schemes.
2. **Digital Signatures**: Implement XMLDSig / XAdES digital signature verification and creation for ODT, ODS, ODP, DOCX, XLSX, and PPTX documents using GnuPG / PKCS#11 smart cards.
3. **WebDAV / Nextcloud Sync**: Provide direct GTK Gio VFS integration for seamless remote file open/save dialogs, background auto-save sync, and conflict resolution over WebDAV.

---

## Technical Specifications

### 1. Document Encryption Standards

| Format | Encryption Standard | Key Derivation | Target Crate / Library |
|--------|---------------------|----------------|------------------------|
| ODF (.odt, .ods, .odp) | ODF 1.3 Encryption Spec (AES-256-GCM) | Argon2id / PBKDF2 | `letters-core`, `tables-core`, `decks-core` |
| OOXML (.docx, .xlsx, .pptx) | Office Agile Encryption | PBKDF2-HMAC-SHA512 + AES-256-CBC | `suite-common-core::crypto` |

#### Key Requirements:
- Zero cleartext temp files: In-memory decryption buffer stream for Zip archive extraction.
- User interface: Native `AdwPasswordEntryDialog` prompt on document open/save.

---

### 2. Digital Signatures (XMLDSig / XAdES)

- Signature validation status displayed in header bar status pill (e.g. "Validly Signed", "Signature Modified / Invalid").
- Support PKCS#11 hardware tokens / YubiKey and system GnuPG keyrings for signing on document export.
- Signature manifest parsing embedded directly within ODF `META-INF/documentsignatures.xml` and OOXML `_xmlsignatures/`.

---

### 3. Remote Storage & WebDAV / Nextcloud VFS

- Utilize `gio::File` VFS abstraction to support `dav://` and `davs://` location URIs seamlessly.
- Implement atomic save transactions (`gio::File::replace_async`) with fallback lockfile detection to avoid remote file clobbering.
- Nextcloud app password & token storage integrated with Freedesktop Secret Service API (`libsecret` / GNOME Keyring).

---

## Implementation Horizons

### Horizon 1 (Near-term): Specification & Schema Support
- Extract `suite-common-core::crypto` module.
- Add ODF Zip encryption header detection and password dialog wiring.

### Horizon 2 (Mid-term): Full Read/Write Encryption & Signatures
- Support password-protected saving across Letters, Tables, and Decks.
- Implement XAdES signature verification indicator in header bar.

### Horizon 3 (Long-term): WebDAV Cloud Sync & Fleet Management
- Deep Gio WebDAV integration with offline caching.
- Fleet dconf policy overrides for enforced encryption.

---

## Verification & Conformance

Evidence tracking will be recorded in `conformance/capabilities.json` under:
- `suite.crypto.odf-encryption`
- `suite.crypto.digital-signatures`
- `suite.vfs.webdav-sync`

---
