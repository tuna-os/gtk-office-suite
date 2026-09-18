# RFC 0002: Privacy-First Local AI Document Intelligence Architecture

**Status**: draft  
**Author**: strategist agent  
**Date**: 2026-09-12  
**Target Milestone**: Q4 2026 / 2027-Q1  

---

## Executive Summary

As GTK Office Suite reaches daily-driver editing maturity in Q4 2026, enterprise and privacy-conscious users require intelligent document assistance (summarization, smart layout generation, grammar refinement, and table data extraction) without exposing sensitive documents to third-party cloud APIs. Integrating local LLM/SLM inference engines (e.g., via `llama.cpp` bindings or local IPC sockets) with strict zero-egress sandboxing presents a major strategic differentiator over cloud-bound office suites.

This RFC defines the architectural blueprint, zero-egress security model, IPC interface contracts, and UI integration patterns for client-side local document AI across **Letters**, **Tables**, and **Decks**.

---

## Strategic & Product Rationale

### 1. Privacy-First & Enterprise Compliance
Enterprise users in legal, healthcare, finance, and government sectors are strictly prohibited from transmitting internal document contents to public cloud AI endpoints (GDPR, HIPAA, SOC2 compliance). A local-first, zero-egress inference engine ensures complete document confidentiality while offering modern AI productivity enhancements.

### 2. Linux Desktop Differentiator
Commercial office suites (Microsoft 365 Copilot, Google Workspace Duet AI) tie AI productivity features directly to cloud subscriptions and remote inference. GTK Office Suite can lead the Linux ecosystem by delivering high-efficiency local model inference utilizing desktop GPU/NPU acceleration (Vulkan/SYCL/OpenCL/ROCm).

### 3. Sub-System Scoping
In accordance with our core architecture rule (**No business logic in widget code**), all text chunking, prompt assembly, token stream parsing, and inference client state reside in `suite-common-core` (or a dedicated `suite-ai-core` crate). GTK widgets only bind to async UI signals and present non-modal suggestion flows.

---

## Architectural Constraints

1. **Zero Egress Enforcement**: The local AI engine MUST operate strictly within a network-isolated environment. Under Flatpak, AI process sockets or IPC interfaces MUST NOT require broad network access (`--net`).
2. **Resource Budgeting**: Model footprint MUST be bounded (defaulting to quantized 3B–7B parameter models, e.g., Q4_K_M). Inference MUST run asynchronously on background threads or separate processes without stalling GTK main loops or render pipelines.
3. **Optional Opt-In**: AI features MUST be completely optional and disabled by default until model weights are explicitly configured by the user or system administrator (via GSettings/dconf policy).
4. **Pure Rust / C-FFI Isolation**: Core prompt context construction, text tokenization boundaries, and response parsing MUST be testable without requiring real GPU/NPU hardware or actual model weights in unit test suites.

---

## Proposed Architecture & Interface Contract

```
┌─────────────────────────────────────────────────────────┐
│              GTK Applications (Letters/Tables/Decks)   │
│   (UI Widgets, Action Handlers, Suggestion Popovers)    │
└────────────────────────────┬────────────────────────────┘
                             │ Async Signal / Event Channel
┌────────────────────────────▼────────────────────────────┐
│                    suite-common-core                    │
│   (Document Context Chunking, Prompt Templates, Parser) │
└────────────────────────────┬────────────────────────────┘
                             │ Zero-Egress IPC / C-FFI
┌────────────────────────────▼────────────────────────────┐
│                suite-ai Local Inference                 │
│      (llama.cpp / ONNX Runtime Local Engine / NPU)       │
└─────────────────────────────────────────────────────────┘
```

### Async Interface Contract (`suite-common-core`)

```rust
/// Scope of the document context passed to the local inference engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiContextScope {
    Selection(String),
    Paragraph(String),
    DocumentSummary { title: String, excerpt: String },
    TableRegion { headers: Vec<String>, rows: Vec<Vec<String>> },
}

/// AI Task Intent requested by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTaskIntent {
    Summarize,
    FixGrammar,
    Elaborate,
    ExtractTableInsights,
    GenerateSlideOutline,
}

/// Result stream chunk emitted during local model generation.
#[derive(Debug, Clone)]
pub enum AiStreamChunk {
    Token(String),
    Completed { total_tokens: usize, duration_ms: u64 },
    Error(String),
}

/// Interface contract implemented by the local inference backend.
pub trait LocalAiProvider: Send + Sync {
    fn is_available(&self) -> bool;
    fn stream_completion(
        &self,
        intent: AiTaskIntent,
        scope: AiContextScope,
        callback: Box<dyn Fn(AiStreamChunk) + Send + 'static>,
    ) -> Result<(), String>;
}
```

---

## Security & Flatpak Deployment Model

To maintain zero-egress guarantees:
* **Flatpak Sandbox**: The Flatpak manifest remains configured without `--share=network` for local AI processing.
* **Model Weight Storage**: Model files reside in `$XDG_DATA_HOME/gnome-office/models/` or system-wide locations (`/usr/share/models/`).
* **dconf Administrative Control**: Enterprise fleet administrators can disable local AI features entirely via dconf policies (`org.gnome.Office.AI.enabled = false`).

---

## Open Questions & Verification Criteria

1. **Quantization & Model Distribution**: Should default model weights be offered as separate Flatpak extensions (`org.gnome.Office.Model.Llama3`) or downloaded on-demand by the user?
2. **Performance Benchmarks**: What is the target latency budget for initial token generation (TTFT) on standard integrated GPU hardware?
3. **Evaluation Matrix**: How will document transformation accuracy be tracked in automated tests without introducing huge model binary dependencies in CI? (Mock providers in `suite-common-core` unit tests).

---

## Implementation Phasing

* **Phase 1 (Q4 2026)**: Implement `LocalAiProvider` interface and mock engine in `suite-common-core` with unit tests.
* **Phase 2 (Q1 2027)**: Implement `llama.cpp` / Vulkan C-FFI bridge and background thread manager.
* **Phase 3 (Q1 2027)**: Add UI popovers and action entries for Letters (summarize/fix text) and Tables (formula/insight helper).
