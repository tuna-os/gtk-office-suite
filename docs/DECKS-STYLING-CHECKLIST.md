# Decks Authoring: Themes, Layouts, Object Styling & Image Workflows

This document verifies compliance with issue #116 requirements for presentation authoring workflows in **Decks**.

---

## 1. Preserved Master Relationships & Layout Selection
- Master slide mapping (`master_idx`) and background properties are preserved across slide addition, deletion, duplication, and reordering.
- Theme backgrounds and default fonts inherit from the master slide without flattening relationships.

## 2. Shape & Object Styling
- **Shape Fill & Stroke**: Supports hex color format specification, line width, and outline options.
- **Text Styling**: Supports font family, font size, text color, alignment, and formatting tags (bold/italic/underline/strike).
- **Opacity & Rotation**: Transform properties and layer ordering preserve object styling attributes.

## 3. Image Workflows
- **Aspect Lock & Dimensions**: Maintains width/height ratio during resize operations.
- **Sizing Modes**: Supports `contain` and `cover` viewport fitting.
- **Crop & Replace**: Preserves original image source reference while storing crop boundaries and alt text attributes.

## 4. Slide Duplicate & Reorder
- Slide duplicate/reorder operations preserve presenter notes, slide transition types/durations, and master slide linkages without data loss.

## 5. PPTX / ODP Format Parity & Unsupported Warnings
- Supported shape/text/image attributes round-trip through `decks-core` engine format adapters.
- Unsupported formatting properties emit explicit user warnings prior to file export.
