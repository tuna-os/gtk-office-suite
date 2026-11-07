# DPA return print fitting reduction (#1330)

The `tables/print-fit-page` fixture isolates the page fitting settings of
the DPA return's Contents sheet. It uses generated text, four wide columns,
landscape A4, `fitToPage="1"` and a stored print scale of 62%. It has no
hyperlinks, hidden sheets, hidden rows, merges or custom colours.

The source workbook's `xl/worksheets/sheet8.xml` contains these settings;
`xl/workbook.xml` names that sheet Contents and selects it with
`activeTab="7"`. The source is the OGL-UK-3.0 workbook linked from #1330.

In the issue's CI run 40077970330 (`render-real-tables` artifact), the
Contents heading fits across LibreOffice's landscape page, while both
Tables tiers draw it beyond the right edge of the captured grid. The
reported 1.69 scale ratio is consistent with this print-fitting mismatch.
This identifies a candidate cause, not proof that it is the only defect:
the workbook also contains hyperlinks and hidden rows.

Reproduce the isolated case with:

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/print-fit-page in render-lab-out/report.html.
```

The reduction deliberately leaves the real-document baseline unchanged.
Remaining work is to reconcile the editor's grid view with the fitted
print reference, render the reduction and the real corpus, and update
`baseline-real.json` only from measured results. #1330 remains open until
both real-document tiers are green without regressing other documents.
