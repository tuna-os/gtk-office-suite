# Executable file-opening corpus

Tracks #447 and the readiness architecture in #443. This is actual office-file
input, distinct from metadata-only interoperability inventories.

Generate into a fresh directory:

```sh
python3 interop/build_open_corpus.py target/open-corpus
# For a Flatpak-only LibreOffice installation, add --flatpak.
GUI_FILE_CORPUS="$PWD/target/open-corpus/manifest.json" \
  tests/gui/run_gui_tests.sh test_file_corpus.py --junitxml="$PWD/target/file-corpus.xml"
```

The initial 42 files cover DOCX/ODT, XLSX/ODS/XLS, PPTX/ODP, TXT/Markdown/HTML,
CSV/TSV, Unicode, quoted multiline values, multiple sheets/slides, basic rich
text/tables/shapes, long documents, uppercase extensions, non-ASCII paths with
spaces, empty packages and truncated packages. Sources are project-authored;
LibreOffice converts the packaged formats. These are **not Microsoft-authored
samples**. Producer version, source/output hashes and licensing are recorded.
The generator refuses existing output directories. No user documents are used.

Every valid case opens a copy, checks content markers, edits, undoes/redoes,
saves a new canonical-format copy, restarts and checks semantic content. Original
hashes must remain unchanged. Invalid packages must produce a visible error
without crashing. Missing manifests and tampered fixtures fail, never skip.
CI retains generated inputs, provenance, JUnit and failure artifacts for 30 days.

These are baseline content checks, not complete fidelity claims: Letters checks
paragraph text; Tables checks sheet names and active-sheet cells; Decks checks
slide count and object text/position. Full formatting, inactive-sheet values,
images, charts, formulas, notes/masters, encrypted documents, large-file limits,
external-producer samples and independent output validation remain roadmap work.
The workflow is intentionally allowed to expose unsupported formats and crashes;
do not turn failures into skips to declare compatibility.
