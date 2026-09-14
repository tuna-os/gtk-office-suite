# GTK Office Suite Template Catalog & Manifest Specification

## Overview

This document specifies the document template architecture, catalog schema, and distribution format for **Letters** (word processor), **Tables** (spreadsheet), and **Decks** (presentations).

## Template Manifest Format (`template.json`)

Each document template package MUST include a top-level `template.json` manifest describing metadata, application target, and asset layout.

```json
{
  "$schema": "https://gtk-office.org/schemas/template-v1.json",
  "id": "org.gtk_office.letters.resume-modern",
  "version": "1.0.0",
  "target_app": "letters",
  "name": "Modern Professional Resume",
  "description": "Clean, two-column resume template optimized for modern typography and clear hierarchy.",
  "author": "GTK Office Suite Core Contributors",
  "license": "CC0-1.0",
  "tags": ["resume", "career", "professional"],
  "document_file": "template.odt",
  "thumbnail_file": "thumbnail.png",
  "min_app_version": "1.0.0"
}
```

## Catalog Structure (`catalog.json`)

Repositories serving template collections host a central `catalog.json` file aggregating available templates.

```json
{
  "catalog_version": "1.0",
  "updated_at": "2026-09-12T12:00:00Z",
  "templates": [
    {
      "id": "org.gtk_office.letters.resume-modern",
      "app": "letters",
      "name": "Modern Professional Resume",
      "summary": "Clean two-column professional resume layout",
      "category": "Documents/Resumes",
      "download_url": "https://templates.gtk-office.org/letters/resume-modern-1.0.0.tar.xz",
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }
  ]
}
```

## Offline Bundles & Storage Path

- System-provided default templates: `/app/share/gtk-office-suite/templates/`
- User-installed custom templates: `~/.var/app/org.gtk_office.Suite/data/templates/` or `~/.local/share/gtk-office-suite/templates/`

## Integration Requirements

1. **First-Run Launcher Modal**: Present curated starter templates when launching Letters, Tables, or Decks.
2. **Deterministic Validation**: Verify SHA256 checksums and target app schema compatibility prior to unpacking.
