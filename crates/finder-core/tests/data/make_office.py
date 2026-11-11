"""Builds the Office test files with LibreOffice, so they're what a real program writes.

    /usr/bin/python3 make_office.py      # run by make.sh; needs LibreOffice and its Python bridge (uno)

- sheet.xlsx, sheet.ods, sheet.xls: two sheets, "Budget" (text, numbers, a formula, a date) and "Notes".
- office.docx: office.html (headings, bold and italic, lists, a table) saved as Word.
- deck.pptx: three slides: a title slide, a slide with bullets on two levels, and one with a text box.
- locked.xlsx, locked.docx: password-protected (password "secret").
"""

import datetime
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

import uno
from com.sun.star.beans import PropertyValue

HERE = Path(__file__).resolve().parent


def prop(name, value):
    p = PropertyValue()
    p.Name = name
    p.Value = value
    return p


def url(path):
    return uno.systemPathToFileUrl(str(path))


def start():
    profile = tempfile.mkdtemp(prefix="tos-office-")
    pipe = f"tosfixtures{os.getpid()}"
    proc = subprocess.Popen(
        [
            "soffice",
            "--headless",
            "--invisible",
            "--norestore",
            f"-env:UserInstallation={url(profile)}",
            f"--accept=pipe,name={pipe};urp;",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local)
    for _ in range(150):
        try:
            ctx = resolver.resolve(f"uno:pipe,name={pipe};urp;StarOffice.ComponentContext")
            desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
            return proc, profile, desktop
        except Exception:
            time.sleep(0.2)
    proc.kill()
    raise SystemExit("LibreOffice didn't start")


def save(doc, path, filt, password=None):
    props = [prop("FilterName", filt), prop("Overwrite", True)]
    if password:
        props.append(prop("Password", password))
    doc.storeToURL(url(path), tuple(props))


def spreadsheet(desktop):
    doc = desktop.loadComponentFromURL("private:factory/scalc", "_blank", 0, (prop("Hidden", True),))
    sheets = doc.Sheets
    budget = sheets.getByIndex(0)
    budget.Name = "Budget"
    sheets.insertNewByName("Notes", 1)
    locale = uno.createUnoStruct("com.sun.star.lang.Locale")
    formats = doc.NumberFormats
    iso = formats.queryKey("YYYY-MM-DD", locale, False)
    if iso == -1:
        iso = formats.addNew("YYYY-MM-DD", locale)
    rows = [
        ["Item", "Qty", "Price", "Total", "Bought"],
        ["Apples", 3, 0.5, "=B2*C2", datetime.date(2026, 11, 7)],
        ["Pears", 10, 0.25, "=B3*C3", datetime.date(2026, 1, 2)],
    ]
    for r, row in enumerate(rows):
        for c, value in enumerate(row):
            cell = budget.getCellByPosition(c, r)
            if isinstance(value, datetime.date):
                cell.Value = (value - datetime.date(1899, 12, 30)).days
                cell.NumberFormat = iso
            elif isinstance(value, str) and value.startswith("="):
                cell.Formula = value
            elif isinstance(value, str):
                cell.String = value
            else:
                cell.Value = value
    sheets.getByIndex(1).getCellByPosition(0, 0).String = "Second sheet"
    doc.calculateAll()
    save(doc, HERE / "sheet.xlsx", "Calc MS Excel 2007 XML")
    save(doc, HERE / "sheet.ods", "calc8")
    save(doc, HERE / "sheet.xls", "MS Excel 97")
    save(doc, HERE / "locked.xlsx", "Calc MS Excel 2007 XML", "secret")
    doc.close(True)


def word(desktop):
    doc = desktop.loadComponentFromURL(
        url(HERE / "office.html"), "_blank", 0, (prop("Hidden", True), prop("FilterName", "HTML (StarWriter)"))
    )
    save(doc, HERE / "office.docx", "MS Word 2007 XML")
    save(doc, HERE / "locked.docx", "MS Word 2007 XML", "secret")
    doc.close(True)


def placeholder(page, kind):
    for i in range(page.Count):
        shape = page.getByIndex(i)
        if shape.ShapeType == f"com.sun.star.presentation.{kind}":
            return shape
    raise SystemExit(f"no {kind} on the slide")


def bullets(shape, items):
    """Fills an outline placeholder with (text, level) paragraphs."""
    text = shape.Text
    text.String = "\n".join(t for t, _ in items)
    paragraphs = text.createEnumeration()
    for _, level in items:
        paragraphs.nextElement().NumberingLevel = level


def presentation(desktop):
    doc = desktop.loadComponentFromURL("private:factory/simpress", "_blank", 0, (prop("Hidden", True),))
    pages = doc.DrawPages
    # Layouts: 0 is a title slide, 1 a title and content.
    first = pages.getByIndex(0)
    first.Layout = 0
    placeholder(first, "TitleTextShape").String = "Quarterly review"
    placeholder(first, "SubtitleShape").String = "Prepared by the team"

    second = pages.insertNewByIndex(0)
    second.Layout = 1
    placeholder(second, "TitleTextShape").String = "Results"
    bullets(placeholder(second, "OutlinerShape"), [("Revenue up", 0), ("Mostly in Europe", 1), ("Costs flat", 0)])

    third = pages.insertNewByIndex(1)
    third.Layout = 1
    placeholder(third, "TitleTextShape").String = "Next steps"
    bullets(placeholder(third, "OutlinerShape"), [("Hire two people", 0)])
    box = doc.createInstance("com.sun.star.drawing.TextShape")
    size = uno.createUnoStruct("com.sun.star.awt.Size")
    size.Width, size.Height = 8000, 2000
    point = uno.createUnoStruct("com.sun.star.awt.Point")
    point.X, point.Y = 2000, 15000
    third.add(box)
    box.Size = size
    box.Position = point
    box.String = "Questions?"
    save(doc, HERE / "deck.pptx", "Impress MS PowerPoint 2007 XML")
    doc.close(True)


def main():
    proc, profile, desktop = start()
    try:
        spreadsheet(desktop)
        word(desktop)
        presentation(desktop)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass
        proc.wait(timeout=30)
        shutil.rmtree(profile, ignore_errors=True)


if __name__ == "__main__":
    main()
