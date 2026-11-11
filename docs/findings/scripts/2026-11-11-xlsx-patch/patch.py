"""Patch cell values inside an xlsx without touching anything else, the way plan option A would.

Usage: python3 -I patch.py IN.xlsx OUT.xlsx [--calc] [--drop-cached]
  Changes sheet1: A2 -> inline string "Plums", C2 -> 2 (D2 = B2*C2 depends on it),
  D3's formula -> the plain value 99, E2 -> date serial 46334 (2026-11-08).
  --calc also sets fullCalcOnLoad="1" on <calcPr> in xl/workbook.xml.
  --drop-cached also removes the last calculated value (<v>) of every formula cell left in sheet1.
Every other zip entry is copied unchanged.
"""
import re
import sys
import zipfile

src, dst = sys.argv[1], sys.argv[2]
calc = "--calc" in sys.argv
drop_cached = "--drop-cached" in sys.argv

CELLS = {
    # ref: new <c> body (attributes keep r and s; t changes as needed)
    "A2": ('t="inlineStr"', "<is><t>Plums</t></is>"),
    "C2": ('t="n"', "<v>2</v>"),
    "D3": ('t="n"', "<v>99</v>"),
    "E2": ('t="n"', "<v>46334</v>"),
}


def patch_sheet(xml: str) -> str:
    for ref, (t, body) in CELLS.items():
        pat = re.compile(r'<c r="%s"( s="\d+")?[^>]*?(/>|>.*?</c>)' % ref)
        m = pat.search(xml)
        assert m, ref
        style = m.group(1) or ""
        xml = xml[: m.start()] + f'<c r="{ref}"{style} {t}>{body}</c>' + xml[m.end():]
    if drop_cached:
        xml = re.sub(r"(<f[^>]*>[^<]*</f>)<v>[^<]*</v>", r"\1", xml)
    return xml


with zipfile.ZipFile(src) as zin, zipfile.ZipFile(dst, "w") as zout:
    for info in zin.infolist():
        data = zin.read(info.filename)
        if info.filename == "xl/worksheets/sheet1.xml":
            data = patch_sheet(data.decode()).encode()
        elif info.filename == "xl/workbook.xml" and calc:
            data = re.sub(rb"<calcPr", b'<calcPr fullCalcOnLoad="1"', data, count=1)
        zout.writestr(info, data)
