# Matrix Binding Profile (matrix/1)

**Status**: Phase 3 - In Planning
**DCP Version**: 1
**Binding**: Matrix (Synapse 1.163.2+)

## Overview

Matrix provides L2 (document collaboration) with full power level enforcement, event encryption (future), and federation (future). This profile documents the Synapse 1.163.2 configuration and client patterns for gtk-office-suite.

## Requirements

### Authentication
- `oo7` (Secret Service) stores homeserver credentials
- User must be logged in before initiating Share
- Matrix room created per document session
- Power level 50 for encryption (spec 8.1)

### Event Model
- Power level resolved at event via hybrid rule (linked forward walk + `/context` fallback)
- Lease renewal counter required (dcp: 1 format)
- M_LIMIT_EXCEEDED handling: batch updates, respect token bucket

### UI Flow
1. Share button → login prompt (if not authenticated)
2. Enter homeserver URL + username/password
3. Create room (+ initial invite list from share dialog)
4. Return locator (durable-stream type)
5. Join button → prompt for room ID or invite link

## Not In Scope
- Real federation (testing only on single Synapse instance)
- Presence (L3, out of scope)
- Comments (out of scope)
- Key backup (out of scope)

## Testing
- Test against Synapse 1.163.2 in collab-apps.yml (new job)
- GUI journeys: Share/Login → Create room → Join → Concurrent edits
- Verify power levels and lease renewal
- Measure latency and throughput vs Durable Streams baseline

## References
- DCP Spec (docs/DOCUMENT-COLLABORATION-SPEC.md), section 8.1
- RFC-0005 (docs/rfc/0005-collaboration-in-the-apps.md), section on Matrix

## Open Questions
- Should we support invite links for anonymous joins? (future)
- Should we batch presence updates? (future)
- Should we implement adaptive token bucket tuning? (future)
