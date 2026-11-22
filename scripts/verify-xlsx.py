#!/usr/bin/env python3
"""Open a Batch Search report with an independent reader and check its shape.

The Rust tests prove the workbook is written; this script proves it is a
workbook another program can read. It is deliberately read-only: it never
rewrites the file it is given.

Usage:
    python3 scripts/verify-xlsx.py core/target/tmp/zeidex-batch-report.xlsx

Exit status is 0 when every check passes, 1 otherwise. The checks mirror the
contract in assets/docs/batch-search.md: four sheets, a frozen header, an
autofilter on the results, percent and datetime formats, Arabic text preserved,
and an unsearchable entry kept out of the Missing sheet.
"""

from __future__ import annotations

import sys
from pathlib import Path

try:
    from openpyxl import load_workbook
except ImportError:  # pragma: no cover - the message is the point
    sys.exit("openpyxl is required: python3 -m pip install openpyxl")


def fail(message: str) -> None:
    print(f"FAIL: {message}")
    sys.exit(1)


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} <report.xlsx>")

    path = Path(sys.argv[1])
    if not path.is_file():
        fail(f"{path} does not exist")

    workbook = load_workbook(path, read_only=False, data_only=False)
    expected = ["Summary", "Results", "Missing", "Multiple Matches"]
    if workbook.sheetnames != expected:
        fail(f"sheets are {workbook.sheetnames}, expected {expected}")

    summary = workbook["Summary"]
    results = workbook["Results"]
    missing = workbook["Missing"]
    multiple = workbook["Multiple Matches"]

    if results.freeze_panes != "B2":
        fail(f"Results freeze panes are {results.freeze_panes!r}, expected 'B2'")
    if results.auto_filter.ref is None:
        fail("Results has no autofilter")
    if not str(results.auto_filter.ref).startswith("A1:"):
        fail(f"Results autofilter starts at {results.auto_filter.ref!r}, expected A1")

    # A real report has more than the header row, and the header row is bold.
    if results.max_row < 2:
        fail("Results has no data rows")
    if not results["A1"].font.bold:
        fail("Results header is not bold")

    percent_cells = [
        cell
        for row in results.iter_rows(min_row=2, max_col=4)
        for cell in row
        if cell.number_format == "0%"
    ]
    if not percent_cells:
        fail("no 0% match-percentage cells in Results")

    datetime_cells = [
        cell
        for row in results.iter_rows(min_row=2)
        for cell in row
        if cell.number_format == "yyyy-mm-dd hh:mm"
    ]
    if not datetime_cells:
        fail("no yyyy-mm-dd hh:mm cells in Results")

    # Arabic must survive the round trip; the fixture has a fully Arabic name.
    arabic = any(
        isinstance(cell.value, str) and "\u0600" <= cell.value[0] <= "\u06ff"
        for row in results.iter_rows(min_row=2)
        for cell in row
        if cell.value
    )
    if not arabic:
        fail("no Arabic text found in Results")

    # The unsearchable (tatweel-only) fixture must be an Error row in Results
    # and must not be listed as a missing file.
    statuses = [str(cell.value) for cell in results["B"][1:] if cell.value]
    if not any("error" in status.lower() for status in statuses):
        fail(f"no Error row in Results (statuses: {statuses})")

    missing_names = [str(cell.value) for cell in missing["A"][1:] if cell.value]
    if any("\u0640" in name for name in missing_names):
        fail("the unsearchable entry leaked into the Missing sheet")

    # The fixture has report.pdf in two folders, so the sheet must have rows.
    if multiple.max_row < 2:
        fail("Multiple Matches is empty, but the fixture has a duplicate name")

    # A truncated candidate list has to say so; otherwise the sheet reads as the
    # complete answer.
    headers = [str(cell.value) for cell in multiple[1] if cell.value]
    if "Notes" not in headers:
        fail(f"Multiple Matches has no Notes column (headers: {headers})")

    # The summary chart must plot the mutually exclusive outcomes. `Found` is the
    # sum of the two match rows, so a series covering it would report more
    # outcomes than there were requests.
    charts = summary._charts
    if not charts:
        fail("Summary has no chart")
    values = [str(ref) for chart in charts for series in chart.series for ref in (series.val.numRef.f,)]
    if not any(ref.endswith("$B$10") for ref in values):
        fail(f"the summary chart does not reach the outcome rows: {values}")
    if any(ref.endswith("$B$6") for ref in values):
        fail(f"the summary chart plots the double-counted 'Found' row: {values}")

    print(f"OK {path}")
    print(f"  sheets: {workbook.sheetnames}")
    print(f"  results: {results.max_row - 1} rows, filter {results.auto_filter.ref}")
    print(f"  missing: {missing.max_row - 1} rows")
    print(f"  multiple: {multiple.max_row - 1} rows")
    print(f"  chart series: {values}")


if __name__ == "__main__":
    main()
