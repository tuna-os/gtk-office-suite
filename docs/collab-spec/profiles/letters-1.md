# Letters Collaboration Profile (letters/1)

**Status**: Phase 5 - Future, Awaits Live Model Foundation
**DCP Version**: 1
**Apps**: Letters (word processor), requires Phase 0 live collaborative model

## Overview

Letters (word processor) requires a fundamentally different collaboration model from Tables and Decks, since word processors benefit from character-level concurrent edits and operational transformation (OT) or CRDT semantics for optimal UX. Phase 5 depends on Phase 0: designing and implementing Letters' live collaborative text model.

## Architecture (Future)

- **Text model**: Character-level CRDT or OT-based text operations
- **Formatting**: Paragraph and character style ranges linked to positions
- **Bindings**: Streams/Matrix (inherited from Table CRDT model)
- **Sync unit**: Per-paragraph atomic groups with character-level resolution

## Phase 0 Deliverables (Prerequisite)

Before Phase 5 can proceed, Letters must:

1. Define text operation semantics (CRDT vs OT trade-offs)
2. Implement live model in suite-common or letters-core
3. Add undo/redo support for text operations (per ADR 0011)
4. Design presence model for cursor/selection sharing
5. Establish backward compatibility with static documents

## Testing (Phase 5+)

- AT-SPI journeys: Type/Format → Share → Join → Concurrent edits
- Verify text ordering and formatting preserved across sessions
- Measure word count vs edit latency
- Test with both Streams and Matrix bindings
- Verify cursor position updates (presence, future feature)

## Not In Scope

- Comments/track changes (out of scope, future)
- Presence-based cursor tracking (L3, requires Phase 0 design)
- Formatting beyond basic styles (future)
- Export to collaborative formats (ODT, OOXML, future)

## References

- Phase 0 design doc (TBD, suite-common or letters-core)
- DCP Spec (docs/DOCUMENT-COLLABORATION-SPEC.md)
- RFC-0005 (docs/rfc/0005-collaboration-in-the-apps.md), Phase 5 and Phase 0

## Open Questions

- Should text operations be character-level CRDT or paragraph-level OT? (Phase 0)
- How do we handle presence (cursor/selection) efficiently? (Phase 0)
- Should we support track changes / comment threads? (future)
- Should we version export formats for collaborative interop? (future)
