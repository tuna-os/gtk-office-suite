# Interop evidence: what crosses LibreOffice, in which direction

Every supported feature named in the interoperability readiness row (text,
styles, images, links, notes, geometry, formulas, sheet order), per app and
format, with the test that proves it in each direction (#1276).

- **Ours → LibreOffice**: our writer produced the file in this format,
  LibreOffice opened it, and the feature was checked on the other side,
  either in LibreOffice's own output or back through our reader.
- **LibreOffice → ours**: LibreOffice wrote the file in this format and our
  reader read the feature from it. The file must not be a same-format
  re-save of our own file. A re-save keeps our writer's markup, so LibreOffice's
  output has to come from another format, or from a flat ODF document
  written by hand.

Feature names follow PARITY.md (see `*/fixtures.json`, #1275). A cell holds
either a test, as `path::function`, or `n/a` with the reason and, for
anything that should work, the issue that tracks it.
`tests/test_interop_evidence.py` fails on an empty cell, a cited test that
does not exist, or an `n/a` with no reason. The tests run in the nightly
oracle lane with `REQUIRE_SOFFICE=1`, so a missing LibreOffice fails them
instead of skipping them.

## Letters

| Feature | Format | Ours → LibreOffice | LibreOffice → ours |
|---|---|---|---|
| Text | docx | `letters-core/tests/soffice_oracle.rs::oracle_reads_plain_paragraphs` | `letters-core/tests/soffice_oracle.rs::we_read_soffice_output` |
| Text | odt | `letters-core/tests/soffice_oracle.rs::odt_oracle_reads_plain_paragraphs` | `letters-core/tests/soffice_oracle.rs::we_read_soffice_odt_output` |
| Styles | docx | `letters-core/tests/soffice_oracle.rs::indents_and_spacing_survive_a_conversion_between_the_two_formats` | `letters-core/tests/soffice_oracle.rs::odt_styles_survive_lo_conversion_to_docx` |
| Styles | odt | `letters-core/tests/soffice_oracle.rs::odt_styles_survive_lo_conversion_to_docx` | `letters-core/tests/soffice_oracle.rs::indents_and_spacing_survive_a_conversion_between_the_two_formats` |
| Images | docx | `letters-core/tests/soffice_oracle.rs::inline_image_survives_lo_docx_pass` | `letters-core/tests/soffice_oracle.rs::we_read_a_picture_writer_places_in_a_docx` |
| Images | odt | `letters-core/tests/soffice_oracle.rs::pictures_in_our_odt_survive_writer_rewriting_it` | `letters-core/tests/soffice_oracle.rs::we_read_a_picture_writer_places_in_an_odt` |
| Links | docx | `letters-core/tests/soffice_oracle.rs::hyperlinks_in_our_docx_reach_writers_odt` | `letters-core/tests/soffice_oracle.rs::hyperlink_survives_lo_pass` |
| Links | odt | `letters-core/tests/soffice_oracle.rs::hyperlink_survives_lo_pass` | `letters-core/tests/soffice_oracle.rs::hyperlinks_in_our_docx_reach_writers_odt` |
| Notes | docx | `letters-core/tests/soffice_oracle.rs::footnotes_survive_a_conversion_between_the_two_formats` | `letters-core/tests/soffice_oracle.rs::footnotes_survive_a_conversion_between_the_two_formats` |
| Notes | odt | `letters-core/tests/soffice_oracle.rs::footnotes_survive_a_conversion_between_the_two_formats` | `letters-core/tests/soffice_oracle.rs::footnotes_survive_a_conversion_between_the_two_formats` |
| Geometry | docx | `letters-core/tests/soffice_oracle.rs::columns_survive_a_conversion_between_the_two_formats` | `letters-core/tests/soffice_oracle.rs::page_geometry_survives_lo_conversion` |
| Geometry | odt | `letters-core/tests/soffice_oracle.rs::page_geometry_survives_lo_conversion` | `letters-core/tests/soffice_oracle.rs::columns_survive_a_conversion_between_the_two_formats` |

## Tables

Tables writes only xlsx (`tables_core::io::is_writable_format`), so ods has
nothing in the Ours → LibreOffice column.

| Feature | Format | Ours → LibreOffice | LibreOffice → ours |
|---|---|---|---|
| Text | xlsx | `tables-core/tests/soffice_oracle.rs::calc_reads_our_xlsx_grid` | `tables-core/tests/soffice_oracle.rs::we_read_calc_authored_xlsx` |
| Text | ods | n/a: Tables does not write ods | `tables-core/tests/soffice_oracle.rs::we_read_calc_authored_ods` |
| Styles | xlsx | `tables-core/tests/soffice_oracle.rs::cell_styles_survive_a_conversion_to_ods` | `tables-core/tests/soffice_oracle.rs::cell_styles_survive_calc_into_its_xlsx` |
| Styles | ods | n/a: Tables does not write ods | `tables-core/tests/soffice_oracle.rs::cell_styles_survive_a_conversion_to_ods` |
| Formulas | xlsx | `tables-core/tests/soffice_oracle.rs::calc_recalculates_our_formulas` | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` |
| Formulas | ods | n/a: Tables does not write ods | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` |
| Cross-sheet formulas | xlsx | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` |
| Cross-sheet formulas | ods | n/a: Tables does not write ods | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` |
| Sheet order | xlsx | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` |
| Sheet order | ods | n/a: Tables does not write ods | `tables-core/tests/soffice_oracle.rs::a_cross_sheet_formula_survives_calc_both_ways` |
| Notes | xlsx | `tables-core/tests/soffice_oracle.rs::notes_survive_calc_both_ways` | `tables-core/tests/soffice_oracle.rs::notes_survive_calc_both_ways` |
| Notes | ods | n/a: Tables does not write ods | `tables-core/tests/soffice_oracle.rs::notes_survive_calc_both_ways` |

The LibreOffice → ours xlsx cells read Calc's xlsx written from Calc's own
ods, not a re-save of ours. `cell_styles_survive_calc_into_its_xlsx`
records the one loss on that path, which is LibreOffice 24.2's:
- Its xlsx export writes a font colour as `theme="1"` once the workbook
  carries the Office theme.
- Calc then reads its own file back black.

## Decks

| Feature | Format | Ours → LibreOffice | LibreOffice → ours |
|---|---|---|---|
| Text | pptx | `decks-core/tests/soffice_oracle.rs::text_survives_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::text_and_run_styling_survive_a_conversion_between_the_two_formats` |
| Text | odp | `decks-core/tests/soffice_oracle.rs::odp_text_survives_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::we_read_impress_authored_odp` |
| Styles | pptx | `decks-core/tests/soffice_oracle.rs::bold_run_survives_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::text_and_run_styling_survive_a_conversion_between_the_two_formats` |
| Styles | odp | `decks-core/tests/soffice_oracle.rs::odp_bold_run_survives_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::text_and_run_styling_survive_a_conversion_between_the_two_formats` |
| Images | pptx | `decks-core/tests/soffice_oracle.rs::image_object_survives_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::impress_finds_the_picture_we_write_into_an_odp` |
| Images | odp | `decks-core/tests/soffice_oracle.rs::impress_finds_the_picture_we_write_into_an_odp` | `decks-core/tests/soffice_oracle.rs::we_read_the_picture_impress_writes_into_an_odp` |
| Notes | pptx | `decks-core/tests/soffice_oracle.rs::notes_survive_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::speaker_notes_survive_a_conversion_between_the_two_formats` |
| Notes | odp | `decks-core/tests/soffice_oracle.rs::odp_notes_survive_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::speaker_notes_survive_a_conversion_between_the_two_formats` |
| Geometry | pptx | `decks-core/tests/soffice_oracle.rs::positions_approx_survive_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::geometry_survives_a_conversion_between_the_two_formats` |
| Geometry | odp | `decks-core/tests/soffice_oracle.rs::odp_geometry_survives_impress_rewrite` | `decks-core/tests/soffice_oracle.rs::geometry_survives_a_conversion_between_the_two_formats` |

Decks has no hyperlinks, formulas or sheets, so those features have no rows.
