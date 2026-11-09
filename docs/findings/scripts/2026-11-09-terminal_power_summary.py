#!/usr/bin/env python3
"""Summarise runs.jsonl from 2026-11-09-terminal_power.py into markdown tables.

Usage: python3 -I 2026-11-09-terminal_power_summary.py runs.jsonl

Desktop (Xorg + gnome-shell), whole-iGPU busy and RAPL watts are given as the difference from the same
round's baseline (no test window). Each cell is the median over rounds, with the min..max range.
"""
import json, statistics, sys

BATTERY_WH = 2.7 * 15.2  # charge_full 2.7 Ah x 15.2 V design voltage = 41 Wh

rows = [json.loads(l) for l in open(sys.argv[1])]
meta = [r for r in rows if r.get("meta")]
runs = [r for r in rows if not r.get("meta")]
base = {r["round"]: r for r in runs if r["name"] == "baseline"}


def metrics(r):
    b = base[r["round"]]
    c, g = r["cpu_pct"], r["gpu_pct"]
    desk = lambda d: d["xorg"] + d["gnome_shell"]
    m = {
        "term_cpu": c.get("term", 0),
        "tb_cpu": c.get("tb_client", 0) + c.get("tb_server", 0),
        "desk_cpu": desk(c) - desk(b["cpu_pct"]),
        "term_gpu": g.get("term", 0),
        "desk_gpu": desk(g) - desk(b["gpu_pct"]),
        "igpu_busy": r["igpu_busy_pct"] - b["igpu_busy_pct"],
    }
    m["cpu_total"] = m["term_cpu"] + m["tb_cpu"] + m["desk_cpu"]
    for k in ("pkg", "uncore", "psys"):
        if k in r["watts"] and k in b["watts"]:
            m[f"{k}_w"] = r["watts"][k] - b["watts"][k]
    return m


def cell(vals, digits=1):
    med = statistics.median(vals)
    return f"{med:.{digits}f} ({min(vals):.{digits}f}..{max(vals):.{digits}f})"


names = []
for r in runs:
    if r["name"] != "baseline" and r["name"] not in names:
        names.append(r["name"])

bad = [(r["name"], r["round"]) for r in runs if r.get("workload_missing") or r.get("focused") is False
       or r.get("dgpu") not in (None, "suspended") or r.get("tb_server_killed")]
print("Meta:", json.dumps(meta[0]) if meta else None)
print("Flagged runs (missing workload, unfocused, dGPU awake, tb server force-killed):", bad or "none")
print("Baselines:", {k: {"pkg_w": v["watts"].get("pkg"), "psys_w": v["watts"].get("psys"),
                         "igpu_busy": v["igpu_busy_pct"],
                         "desk_cpu": round(v["cpu_pct"]["xorg"] + v["cpu_pct"]["gnome_shell"], 1)}
                     for k, v in sorted(base.items())})
print()

cols = [("term_cpu", "terminal CPU %", 1), ("tb_cpu", "tb CPU %", 1), ("desk_cpu", "Xorg+shell CPU % (Δ)", 1),
        ("cpu_total", "total CPU %", 1), ("term_gpu", "terminal iGPU %", 1), ("desk_gpu", "Xorg+shell iGPU % (Δ)", 1),
        ("igpu_busy", "iGPU busy % (Δ)", 1), ("pkg_w", "package W (Δ)", 2), ("uncore_w", "iGPU W (Δ)", 2),
        ("psys_w", "psys W (Δ)", 2)]
print("| setup | " + " | ".join(c[1] for c in cols) + " | battery %/h (est.) |")
print("|---" * (len(cols) + 2) + "|")
for n in names:
    ms = [metrics(r) for r in runs if r["name"] == n]
    out = []
    for k, _, d in cols:
        vals = [m[k] for m in ms if k in m]
        out.append(cell(vals, d) if vals else "n/a")
    pk = [m["pkg_w"] for m in ms if "pkg_w" in m]
    est = f"{100 * statistics.median(pk) / BATTERY_WH:.1f}" if pk else "n/a"
    print(f"| {n} | " + " | ".join(out) + f" | {est} |")

print()
print("Grid sizes:", sorted({(r["term"], r.get("grid")) for r in runs if r.get("grid")}))
