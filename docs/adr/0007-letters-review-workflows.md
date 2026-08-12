# ADR 0007: Letters review workflows

## Status

Model and scope admitted; format/UI integration staged separately.

## Decision

Letters uses Unicode scalar offsets for anchored review ranges, matching the
existing document and GtkTextBuffer addressing. `letters-core::review` owns
revisions, comments, deterministic next/previous navigation, explicit
accept/reject/resolve transitions, heading-derived TOC entries, and strong
Unicode direction detection. GTK actions can therefore provide keyboard
navigation without making review state depend on a display server.

The first admitted review policy is:

- heading styles 1–6 generate a deterministic TOC in document order;
- the first strong Arabic/Hebrew character supplies the paragraph RTL fallback,
  while Pango remains responsible for shaping and caret placement;
- pending revisions and unresolved comments are navigable in start-offset then
  id order, and actions never mutate state implicitly;
- DOCX `w:ins`/`w:del` and comments parts, ODT change-info, and their reverse
  LibreOffice fixtures are not yet claimed. Until adapters and fixtures land,
  export must warn that review metadata is not serialized rather than silently
  promising round-trip fidelity.

## Verification

GTK-free tests cover review transitions, wraparound navigation, TOC ordering,
empty/invalid headings, and mixed LTR/RTL direction detection. The next UI
slice should bind `review-next`, `review-previous`, `accept-change`,
`reject-change`, and `resolve-comment` to the existing action registry and
cover them in a Letters keyboard journey.
