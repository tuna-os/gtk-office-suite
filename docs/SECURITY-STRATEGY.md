# Application Security Architecture & Sandboxing Strategy

## Overview & Threat Model

GTK Office Suite processes complex, user-supplied, third-party document archives across **Letters** (Word Processor), **Tables** (Spreadsheets), and **Decks** (Presentations). Office file formats (\`.docx\`, \`.xlsx\`, \`.pptx\`, \`.odt\`, \`.ods\`, \`.odp\`, \`.rtf\`) represent a historically rich attack vector in desktop operating systems.

This strategy document outlines the security principles, isolation boundaries, safe container handling rules, Flatpak sandbox hardening trajectory, and vulnerability triage mechanisms to maintain enterprise-grade security and zero-trust document handling across all GNOME desktop environments.

---

## 1. Core Security Principles

1. **Memory Safety by Default**: All document parsing, styling calculation, and canvas rendering logic is strictly implemented in memory-safe Rust (\`suite-common-core\`, \`letters-core\`, \`tables-core\`, \`decks-core\`). Unsafe Rust blocks (\`unsafe\`) are forbidden in core parsers and subject to strict architectural justification and automated CI auditing.
2. **Defensive Container & XML Parsing**: Zip container extraction, XML deserialization (\`quick-xml\`), and binary stream reads enforce deterministic resource budgets (allocation bounds, decompression ratios, recursion limits) to neutralize zip-bomb and algorithmic complexity attacks.
3. **Sandbox Least Privilege**: Applications distributed via Flatpak run with minimal required capabilities, replacing raw filesystem and IPC grants with hardened XDG Desktop Portals (\`ashpd\` / \`xdg-desktop-portal\`).
4. **Opaque & Unexecutable Foreign Payloads**: Embedded macros (VBA, StarBasic), executable OLE streams, and active scripts within imported documents are treated as untrusted opaque binary parts. They are never executed, and are either preserved structurally without execution or warned upon export.
5. **Deterministic Headless Verification**: Security constraints are continuously enforced via continuous fuzzing (\`cargo-fuzz\` / libFuzzer), dependency vulnerability scanning (\`cargo-deny\`, \`cargo-audit\`), and static lint gates.

---

## 2. Threat Vectors & Mitigation Matrix

| Threat Vector | Attack Mechanism | Architectural Mitigation & Boundary |
|---|---|---|
| **Zip-Bomb / Decompression Bomb** | Nested zip archives or high-compression payload designed to exhaust RAM/disk. | Hardened streaming extraction in \`suite-common-core\`. Max uncompressed ratio limit (e.g. 100:1), per-part size cap (50MB), total decompressed archive cap (250MB). |
| **XML Entity & Deep Nesting Attacks** | Quadratic blowup via deeply nested elements or circular references. | \`quick-xml\` reader configured with maximum XML element depth bounds (max depth: 128) and buffer size caps. |
| **Untrusted Media & Linked URLs** | Document referencing remote URLs or malicious network endpoints to leak IP/metadata or exploit network stack. | Network isolation: Flatpak manifest omits \`--share=network\`. External image links require explicit local user approval before fetching via portals. |
| **Malicious Embedded Macros / OLE** | Embedded VBA scripts, OLE objects, or shell execution payloads in OOXML/ODF. | Execution engine is completely absent. Macros are parsed solely as inert byte blobs. OLE objects are treated as unexecuted binary attachments. |
| **Unbounded Cell / Grid Allocation** | Spreadsheets declaring millions of sparse rows/columns to trigger out-of-memory crashes. | Sparse cell storage with allocation caps (budget limit: 500,000 active cells per workbook) and streaming row evaluation. |
| **Privilege Escalation via Host Filesystem** | Exploiting overly broad Flatpak filesystem permissions to read/write arbitrary host paths (\`~/.ssh\`, \`~/.config\`). | Phased removal of raw \`--filesystem\` finish-args; full transition to File Chooser Portal (\`org.freedesktop.portal.FileChooser\`). |

---

## 3. Flatpak Sandboxing & Portal Least-Privilege Roadmap

### Manifest Hardening Milestones

\`\`\`
Current Baseline (Broad)               Target Production Posture (Hardened)
┌─────────────────────────────────┐    ┌─────────────────────────────────┐
│ --share=ipc                     │    │ --socket=wayland                │
│ --socket=wayland                │    │ --socket=fallback-x11           │
│ --socket=fallback-x11           │───>│ --device=dri                    │
│ --device=dri                    │    │ --own-name=org.tunaos.<app>     │
│ --filesystem=/run/user          │    │ XDG Desktop Portals (ashpd)     │
│ --filesystem=xdg-documents      │    │ (No direct filesystem grants)   │
└─────────────────────────────────┘    └─────────────────────────────────┘
\`\`\`

### Phase 1: Removal of Broad Filesystem Grants (Q4 2026 Milestone 1)
- **Eliminate \`--filesystem=/run/user\`**: Replace internal shared socket/file discovery with isolated runtime directories.
- **Enforce Portal File Choosers**: Ensure all Open, Save, and Save-As flows in \`letters\`, \`tables\`, and \`decks\` route through \`xdg-desktop-portal\` FileChooser dialogs with file descriptor passing.

### Phase 2: Complete Sandbox Containment (Q4 2026 Milestone 2)
- **Eliminate \`--filesystem=xdg-documents\`**: Move fully to dynamic portal file access (document portal tokens and recent file references).
- **Network Isolation Verification**: Retain zero network permissions (\`--share=network\` omitted) across all office applications.
- **Strict Device Permissions**: Retain \`--device=dri\` strictly for hardware-accelerated Cairo/Pango rendering; block access to input devices, cameras, or audio recording hardware.

---

## 4. Safe Document Container & Decompression Contract

To prevent resource exhaustion and parsing crashes on malformed files, all container parsers must adhere to the following decompression budgets:

\`\`\`rust
pub struct ContainerSafetyLimits {
    /// Maximum uncompressed size of any single part/file within the zip container (default: 50 MB)
    pub max_part_size_bytes: usize,
    /// Maximum aggregate uncompressed size for the entire document package (default: 250 MB)
    pub max_total_size_bytes: usize,
    /// Maximum expansion ratio (uncompressed size / compressed size) before triggering abort (default: 100)
    pub max_expansion_ratio: usize,
    /// Maximum number of file entries permitted inside the archive (default: 2,000)
    pub max_entry_count: usize,
    /// Maximum XML element nesting depth (default: 128)
    pub max_xml_depth: usize,
}
\`\`\`

Any violation of these limits must fail gracefully with a typed \`InteroperabilityError::ResourceBudgetExceeded\` error, preventing application crash or denial of service.

---

## 5. Vulnerability Triage & Supply-Chain Security Policy

1. **Automated Dependency Auditing**:
   - \`cargo-audit\` runs on every pull request and nightly CI build to detect known RustSec advisory CVEs.
   - \`deny.toml\` strictly enforces license compliance (GPL-3.0-or-later compatible) and bans unvetted crates with unsafe memory operations.
2. **Vulnerability Disclosure & Triage Timeline**:
   - **Critical (Remote Code Execution, Arbitrary File Access)**: Triage < 24 hours, patch release < 72 hours.
   - **High (Deterministic Application Crash / Memory Exhaustion via Document)**: Triage < 48 hours, patch release < 7 days.
   - **Medium / Low (Minor UI Disruption, Non-exploitable Parsing Error)**: Addressed in standard milestone release cycle.
3. **Security Reporting Channel**:
   - Security issues must be reported responsibly via private repository security advisories rather than public issues.

---

## 6. Verification & Audit Acceptance Gates

| Quality Gate | Requirement | Verification Command / Target |
|---|---|---|
| **Memory Safety Audit** | Zero unchecked \`unsafe\` blocks in format parsers | \`cargo clippy --workspace -- -D clippy::undocumented_unsafe_blocks\` |
| **Dependency CVE Scan** | Zero unresolved vulnerabilities in active dependency graph | \`cargo audit\` / \`cargo deny check advisories\` |
| **Zip-Bomb Resilience** | Golden test suite containing high-ratio compression fixtures rejected safely | \`cargo test -p suite-common-core --test container_security\` |
| **Flatpak Manifest Audit** | \`flatpak-builder\` manifest passes linter with no unneeded permissions | \`flatpak-builder --dry-run\` validation |
