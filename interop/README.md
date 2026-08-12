# Office interoperability corpus

This directory is the compatibility contract for the six office formats that
the suite reads and writes. The corpus is deliberately small and text-backed:
the package XML is reviewable in a pull request, while the Rust oracle tests
exercise real files produced by the format writers.

`corpus.json` is the source of truth. Every entry has a format, authoring
suite and version, provenance/license, scenario, direction, and normalized
semantic expectations. Each format has both directions:

* `our-write-oracle-rewrite`: suite output is opened and rewritten by
  LibreOffice, then read back by us.
* `oracle-authored`: LibreOffice-authored input is read by us.

Run the cheap structural check locally with:

```sh
python3 interop/validate_corpus.py
```

The validator checks package relationships and content types for OOXML and
ODF. It never compares ZIP member order, timestamps, or raw bytes. Set
`ONLYOFFICE_BIN` to an OnlyOffice command-line binary to run the optional
render/conversion lane; it is intentionally skipped when that variable is
unset.

Real binary samples may be added beside the reviewable package fixtures. A
binary sample must be referenced by `artifact` in its manifest and retain the
same metadata and semantic assertions; generated or licensed files must not
be committed without provenance.

