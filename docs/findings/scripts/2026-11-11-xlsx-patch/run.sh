#!/bin/sh
# Re-runs the 2026-11-11 xlsx patching check. Usage: run.sh OUTDIR (an empty scratch folder)
set -e
here=$(cd "$(dirname "$0")" && pwd)
in="$here/../../../../crates/finder-core/tests/data/sheet.xlsx"
out=$1
cd "$out"
python3 -I "$here/patch.py" "$in" plain.xlsx                        # cells only
python3 -I "$here/patch.py" "$in" calc.xlsx --calc                  # + fullCalcOnLoad="1"
python3 -I "$here/patch.py" "$in" dropped.xlsx --calc --drop-cached # + formulas' last values removed
soffice --headless --convert-to csv --outdir default plain.xlsx calc.xlsx dropped.xlsx >/dev/null 2>&1
# The same, with LibreOffice set to "always recalculate" Excel 2007+ files, in a throwaway profile.
mkdir -p profile/user && cp "$here/always-recalc.xcu" profile/user/registrymodifications.xcu
soffice -env:UserInstallation="file://$PWD/profile" --headless --convert-to csv --outdir always plain.xlsx >/dev/null 2>&1
for f in default/*.csv always/*.csv; do echo "== $f"; cat "$f"; done
echo "== original"; soffice --headless --convert-to csv --outdir orig "$in" >/dev/null 2>&1; cat orig/*.csv
echo "== zip entries that differ from the original (plain.xlsx, calc.xlsx)"
python3 -I - "$in" plain.xlsx calc.xlsx <<'PY'
import sys, zipfile
a = zipfile.ZipFile(sys.argv[1])
for other in sys.argv[2:]:
    b = zipfile.ZipFile(other)
    print(other, [n for n in a.namelist() if a.read(n) != b.read(n)], "of", len(a.namelist()))
PY
