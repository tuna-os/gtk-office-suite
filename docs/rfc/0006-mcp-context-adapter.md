# RFC-0006: a local, read-only MCP document-context adapter (evaluation)

Status: evaluation with a prototype (#1786). Not wired into any app.

## What was evaluated

Frostnotes' MCP server (`internal/mcpserver`, at f334af5) registers read and
write tools separately, has a read-only option, and attributes agent writes.
Three ideas carry over; one does not.

| Frostnotes idea | Here |
| --- | --- |
| Read tools registered apart from write tools; read-only option | Adopted, tightened: the prototype has **no** write tools. A mutation tool is a separate design (below). |
| Agent writes attributed to the agent | Deferred with mutation. Office would route them through the live model, undo and collab revision identity (`suite-collab`), not through files. |
| Bounded edit operations on Markdown text | Not applicable: Office documents are not Markdown files. |
| Server reads the notes directory | **Rejected.** The adapter never opens a document file. |

## Design

- **Transport:** stdio only, started by the app (or a launcher the user starts).
  No listener, no remote endpoint, no external model call. The crate depends on
  `serde_json` alone.
- **Off by default:** `Config::default().enabled == false`; `serve` returns
  `Disabled` before reading a byte. No background discovery: the adapter has no
  way to enumerate documents.
- **Explicit grant:** the agent never supplies a path. The user's action
  ("share this document with my agent") builds a `Grant` from the path the
  app already holds. `Grant::new` refuses symlinks (including symlinked parent
  directories) and non-files; every call re-validates it, so revoking,
  replacing the file with a link, or closing/switching the document fails
  closed (`selection_revoked`, `selection_invalid`, `document_not_open`).
- **Live model, not the file:** content comes from `ContextSource`, implemented
  by the app over the same model the window renders, so unsaved edits are seen
  and the answer agrees with the GUI by construction. The app must implement it
  in the GTK-free core crates, reusing the extraction used for local AI
  (#548/#688) rather than a second extractor.
- **Revision snapshot:** every answer carries `revision` and `dirty`. A caller
  may pass `expected_revision`; a mismatch is `stale_revision`. The app's edit
  counter is the natural revision; for a collaborative document it would be the
  collab version heads.
- **Bounds:** 64 KiB per request line, 256 KiB of text, 10,000 table cells;
  cuts land on UTF-8 boundaries and set `truncated`.
- **Tools:** `office_get_metadata`, `office_get_context` (selection when there is
  one, otherwise the document), both `readOnlyHint`.

## Not done / open

- No app implements `ContextSource` yet, so the acceptance item "context agrees
  with the actual GUI document" is **not** shown. It needs the Letters/Tables/
  Decks wiring, a user-facing enable and share control (and its wording), and a
  GUI test that compares the answer with the window.
- No run against a real external MCP client; the prototype is tested over
  in-memory pipes with hand-written JSON-RPC.
- Only the MCP stdio basics are implemented (`initialize`, `ping`,
  `tools/list`, `tools/call`). Version negotiation is not.
- Mutation stays out. Any later tool needs its own design: edits applied
  through the live model as one undo step, checked against `expected_revision`,
  attributed to the agent, and refused on a read-only or commenter role.
