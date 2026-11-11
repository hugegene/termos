"""Makes the spreadsheets for docs/findings/2026-11-11-calamine.md.

    python3 -I make_xlsx.py DIR

Writes DIR/big.xlsx (100,000 rows x 20 columns, plus a second sheet) and DIR/small.xlsx (200 rows x 20). Each
row mixes text (some repeated, so shared strings), whole and decimal numbers, a date and a formula.
"""

import datetime
import sys
from pathlib import Path

from openpyxl import Workbook


def make(path: Path, rows: int) -> None:
    wb = Workbook(write_only=True)
    ws = wb.create_sheet("Data")
    ws.append([f"col {c}" for c in range(20)])
    start = datetime.date(2020, 1, 1)
    for r in range(1, rows):
        row = []
        for c in range(20):
            kind = c % 5
            if kind == 0:
                row.append(f"name {r % 500}")
            elif kind == 1:
                row.append(r * 7 + c)
            elif kind == 2:
                row.append((r * 31 + c) / 8)
            elif kind == 3:
                row.append(start + datetime.timedelta(days=r % 3000))
            else:
                row.append(f"=B{r + 1}*2")
        ws.append(row)
    notes = wb.create_sheet("Notes")
    notes.append(["second sheet"])
    wb.save(path)


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    make(out / "small.xlsx", 200)
    make(out / "big.xlsx", 100_000)


if __name__ == "__main__":
    main()
