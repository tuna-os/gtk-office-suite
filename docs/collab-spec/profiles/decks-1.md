# Decks Collaboration Profile (decks/1)

**Status**: Phase 4 - In Planning
**DCP Version**: 1
**Apps**: Decks (presentations), uses Tables/Streams binding profile

## Overview

Decks (presentations app) adds collaboration support by extending the Tables collaboration profile. Slide-level operations (insert/delete/move slides) map to document-level operations in the DCP model, with slide content (text, shapes, images) serialized as discrete updates.

## Architecture

- **Slide model**: Each slide is a leaf in the presentation tree
- **Bindings**: Inherits from Tables binding (Streams/Matrix support)
- **Sync unit**: Per-slide atomic updates via DCP lease protocol
- **Conflict resolution**: Slide move/delete conflicts use lease-based last-writer-wins

## Requirements

### Slide Operations
- Insert slide: creates new slide node in tree
- Delete slide: mark slide as deleted (tombstone)
- Move slide: reorder slide position, protected by lease
- Edit slide content: delegated to Tables cell-level model

### Presence
- Not implemented (out of scope, L3 feature)
- Status only: connection indicator, active slide indicator

### Undo/Redo
- Per-step rule (ADR 0011) applied to presentation operations
- Slide move/delete must not clear entire history on conflict

## Testing
- AT-SPI journeys: Add slide → Share → Join → Concurrent edits
- Verify slide ordering preserved across sessions
- Measure presentation size vs slide count
- Test with both Streams and Matrix bindings

## Not In Scope
- Comments on slides (out of scope)
- Slide-level presence (L3)
- Annotation layers (future)

## References
- DCP Spec (docs/DOCUMENT-COLLABORATION-SPEC.md)
- RFC-0005 (docs/rfc/0005-collaboration-in-the-apps.md), Phase 4

## Open Questions
- Should we support slide-level comments? (future)
- Should we track user who edited each slide? (future, requires presence)
