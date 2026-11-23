#!/usr/bin/env python3
"""Aggregates the per-benchmark results of the pipeline into the JSON files in results/, which the
thesis reads. Needs only the Python standard library.

    python3 evaluation/evaluate.py [--analyzed DIR] [--rewritten DIR] [--out DIR]

Defaults (relative to the repository root): work/analyzed, work/rewritten, results.

Output. All values are counts; percentages and rounding are left to the thesis.
  pointer_sources.json    named pointer sources per benchmark     thesis: named_pointer_source_distribution
  usage_types.json        how array sources are used              thesis: effective_usage_distribution
  rewrite_outcomes.json   rewrite applied per usage type          thesis: figure_rewrite_outcomes
  safety.json             safety category per benchmark           thesis: figure_safety_improvement
  extra/*.json            further statistics, not used in the thesis

Every table has the shape {"columns": [...], "rows": [{...}, ...], "total": {...}}, with the rows in
the order the thesis shows them.
"""

import argparse
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

if sys.version_info < (3, 9):
    sys.exit("Error: evaluate.py needs Python 3.9 or newer")

ROOT = Path(__file__).resolve().parent.parent


# --- Loading ----------------------------------------------------------------------------------

def benchmarks(folder: Path) -> list[Path]:
    return sorted(p for p in folder.iterdir() if p.is_dir())


def load(bench: Path, filename: str):
    """analysis_results/<filename> of one benchmark, or None if it does not exist."""
    path = bench / "analysis_results" / filename
    return json.loads(path.read_text()) if path.exists() else None


def load_records(folder: Path, filename: str) -> list[dict]:
    """The records of <filename> of every benchmark, each with a "benchmark" field added."""
    records = []
    for bench in benchmarks(folder):
        for record in load(bench, filename) or []:
            records.append({**record, "benchmark": bench.name})
    return records


def table(columns: list[str], rows: list[dict], total_columns: list[str]) -> dict:
    total = {c: sum(r[c] for r in rows) for c in total_columns}
    return {"columns": columns, "rows": rows, "total": total}


# --- Named pointer sources (thesis: named_pointer_source_distribution) ------------------------

# C2Rust redeclares extern C functions such as malloc in every file that calls them, and every
# declaration becomes its own pointer source. They are the same C function, so they count once
# per benchmark.
EXTERN_ALLOC_SOURCE_NAMES = {"malloc", "calloc", "realloc", "realloc_old_pointer", "free", "strdup", "strndup"}


def pointer_sources(analyzed: Path) -> dict:
    rows = []
    for bench in benchmarks(analyzed):
        sources = load(bench, "pointer_sources.json")
        if sources is None:
            continue
        seen_extern = set()
        counts = Counter()
        for s in sources:
            if s["name"] in EXTERN_ALLOC_SOURCE_NAMES:
                if s["name"] in seen_extern:
                    continue
                seen_extern.add(s["name"])
            counts["total"] += 1
            if not s["is_fat"]:
                continue
            counts["array"] += 1
            if not s["is_owned"]:
                counts["non_owned_array"] += 1
            elif s["is_output_param"]:
                # Owned output parameters are left out of the rewrite, so they are not counted as owned
                counts["owned_output_param_array"] += 1
            else:
                counts["owned_array"] += 1
        rows.append({"benchmark": bench.name, **{c: counts[c] for c in POINTER_SOURCE_COUNTS}})
    rows.sort(key=lambda r: (-r["total"], r["benchmark"]))
    return table(["benchmark", *POINTER_SOURCE_COUNTS], rows, POINTER_SOURCE_COUNTS)


POINTER_SOURCE_COUNTS = ["total", "array", "owned_array", "owned_output_param_array", "non_owned_array"]


# --- Usage types (thesis: effective_usage_distribution) ---------------------------------------

USAGE_COUNTS = ["owned_array", "owned_output_param_array", "non_owned_array"]

# The rewriter still calls the assignment category `StoreElsewhere`; the thesis calls it
# `Assignment`, as it covers values assigned into a source as well as out of it.
USAGE_TYPE_NAMES = {"StoreElsewhere": "Assignment"}


def usage_type_name(raw: str) -> str:
    return USAGE_TYPE_NAMES.get(raw, raw)


def usage_types(analyzed: Path) -> dict:
    counts = defaultdict(Counter)
    for u in load_records(analyzed, "array_usages.json"):
        if not u["source_is_fat"]:
            continue  # array_usages.json also contains usages of non-array pointers
        if not u["source_is_owned"]:
            kind = "non_owned_array"
        elif u["source_is_output_param"]:
            kind = "owned_output_param_array"
        else:
            kind = "owned_array"
        counts[usage_type_name(u["usage_type"])][kind] += 1
    rows = [{"usage_type": t, **{c: n[c] for c in USAGE_COUNTS}} for t, n in counts.items()]
    rows.sort(key=lambda r: (*(-r[c] for c in USAGE_COUNTS), r["usage_type"]))
    return table(["usage_type", *USAGE_COUNTS], rows, USAGE_COUNTS)


# --- Rewrite outcomes (thesis: figure_rewrite_outcomes) ---------------------------------------

def rewrite_outcomes(outcomes: list[dict]) -> dict:
    counts = defaultdict(Counter)
    for o in outcomes:
        counts[usage_type_name(o["usage_type"])][o["outcome"]] += 1
    names = sorted({o["outcome"] for o in outcomes})
    rows = [{"usage_type": t, **{n: c[n] for n in names}, "total": sum(c.values())} for t, c in counts.items()]
    rows.sort(key=lambda r: (-r["total"], r["usage_type"]))
    return table(["usage_type", *names, "total"], rows, [*names, "total"])


# --- Safety per benchmark (thesis: figure_safety_improvement) ---------------------------------

# How much safer each rewrite outcome (RewriteOutcome in the rewriter's src/rewrite_planner.rs)
# makes an interaction with an owned array.
SAFETY_GROUPS = {
    "BoundsCheckedElement": "bounds_checked",
    "BoundsCheckedPointer": "bounds_checked",
    "MoveBox": "stays_cbox",
    "FromRaw": "stays_cbox",
    "Deallocate": "stays_cbox",
    "Reallocate": "stays_cbox",
    "AsPtr": "leaves_as_raw",
    "IntoRaw": "leaves_as_raw",
    "ReallocateIntoRaw": "leaves_as_raw",
    "RawReference": "leaves_as_raw",
    "Unchanged": "unchanged",
}
SAFETY_COUNTS = ["bounds_checked", "stays_cbox", "leaves_as_raw", "unchanged"]


def safety(outcomes: list[dict]) -> dict:
    unknown = sorted({o["outcome"] for o in outcomes} - SAFETY_GROUPS.keys())
    if unknown:
        sys.exit(f"Error: rewrite outcomes without a safety group: {unknown}; add them to SAFETY_GROUPS")
    counts = defaultdict(Counter)
    promoted_by_source_type = Counter()
    for o in outcomes:
        counts[o["benchmark"]][SAFETY_GROUPS[o["outcome"]]] += 1
        if o["promoted"]:
            # A borrow that the rewriter turned into an ownership transfer (into_raw instead of as_ptr)
            counts[o["benchmark"]]["promoted"] += 1
            promoted_by_source_type[o["source_type"]] += 1
    rows = [
        {"benchmark": b, **{c: n[c] for c in SAFETY_COUNTS}, "promoted": n["promoted"],
         "total": sum(n[c] for c in SAFETY_COUNTS)}
        for b, n in counts.items()
    ]
    rows.sort(key=lambda r: (-r["total"], r["benchmark"]))
    result = table(["benchmark", *SAFETY_COUNTS, "promoted", "total"], rows, [*SAFETY_COUNTS, "promoted", "total"])
    result["promoted_by_source_type"] = dict(sorted(promoted_by_source_type.items()))
    return result


# --- Extra: Crown's own statistics (not used in the thesis) -----------------------------------

CROWN_ARRAY_STATS = [
    "sources_total", "sources_thin", "sources_thin_owning", "sources_thin_transient",
    "sources_arr", "sources_arr_owning", "sources_arr_transient",
    "usages_total", "usages_thin", "usages_thin_owning", "usages_thin_transient",
    "usages_arr", "usages_arr_owning", "usages_arr_transient",
]


def crown_array_stats(analyzed: Path) -> dict:
    """Thin and array pointers per benchmark, as counted by Crown's array analysis (array_stats.json)."""
    rows = []
    for bench in benchmarks(analyzed):
        stats = load(bench, "array_stats.json")
        if stats is not None:
            rows.append({"benchmark": bench.name, **{c: stats[c] for c in CROWN_ARRAY_STATS}})
    return table(["benchmark", *CROWN_ARRAY_STATS], rows, CROWN_ARRAY_STATS)


def crown_pointers(analyzed: Path) -> tuple[list[dict], list[dict]]:
    """One record per named pointer (function variables) and per struct field that Crown tracks.

    Crown's ownership and fatness are lists with one entry per level of indirection: index 0 is the
    pointer itself, deeper indices what it points to (for `T **p`: `p`, then `*p`).
    """
    variables, fields = [], []
    for bench in benchmarks(analyzed):
        own = load(bench, "ownership.json")
        if own is None:
            continue
        fat = load(bench, "fatness.json") or {}
        out = load(bench, "output_params.json") or {}

        def record(own_levels, fat_levels, **extra):
            depth = max(len(own_levels), len(fat_levels))
            if depth == 0:
                return None  # tracked, but without any resolved level (e.g. a non-pointer local)
            return {"benchmark": bench.name, "depth": depth,
                    "outer_owned": bool(own_levels) and own_levels[0] == "Owning",
                    "outer_fat": bool(fat_levels) and fat_levels[0] == "Arr",
                    "arr_levels": sum(1 for f in fat_levels if f == "Arr"), **extra}

        for function, vars_own in own.get("fn_data", {}).items():
            vars_fat = fat.get("fn_data", {}).get(function, {})
            out_params = set(out.get("fn_data", {}).get(function, []))
            for var, levels in vars_own.items():
                r = record(levels, vars_fat.get(var, []), is_output_param=var in out_params)
                if r:
                    variables.append(r)
        for struct, fields_own in own.get("struct_data", {}).items():
            fields_fat = fat.get("struct_data", {}).get(struct, {})
            for field, levels in fields_own.items():
                r = record(levels, fields_fat.get(field, []))
                if r:
                    fields.append(r)
    return variables, fields


def nesting_shape(p: dict) -> str:
    if p["depth"] == 1:
        return "array" if p["outer_fat"] else "thin"
    if p["arr_levels"] >= 2:
        return "array of arrays"
    if p["arr_levels"] == 1:
        return "mixed"  # pointer to an array, or array of pointers
    return "nested thin"


def owned_counts(pointers: list[dict], key) -> dict:
    counts = defaultdict(Counter)
    for p in pointers:
        counts[key(p)]["owned" if p["outer_owned"] else "not_owned"] += 1
    rows = [{"group": g, "owned": c["owned"], "not_owned": c["not_owned"], "total": c["owned"] + c["not_owned"]}
            for g, c in counts.items()]
    rows.sort(key=lambda r: (-r["total"], r["group"]))
    return table(["group", "owned", "not_owned", "total"], rows, ["owned", "not_owned", "total"])


def crown_pointer_shapes(analyzed: Path) -> dict:
    """How Crown classifies named pointers and struct fields: by fatness and output parameter, and
    by nesting shape, each split into owned and not owned."""
    variables, fields = crown_pointers(analyzed)
    fat_owned = [p for p in variables if p["outer_fat"] and p["outer_owned"]]
    return {
        "variables_by_fatness_and_output_param": owned_counts(
            variables, lambda p: f"{'array' if p['outer_fat'] else 'thin'}, "
                                 f"{'output param' if p['is_output_param'] else 'not output param'}"),
        "variables_by_nesting_shape": owned_counts(variables, nesting_shape),
        "struct_fields_by_nesting_shape": owned_counts(fields, nesting_shape),
        # What leaving owned array output parameters unrewritten costs
        "output_param_impact": {
            "tracked_pointers": len(variables),
            "array_pointers": sum(1 for p in variables if p["outer_fat"]),
            "owned_array_pointers": len(fat_owned),
            "owned_array_output_params": sum(1 for p in fat_owned if p["is_output_param"]),
            "not_owned_array_output_params": sum(
                1 for p in variables if p["outer_fat"] and not p["outer_owned"] and p["is_output_param"]),
        },
    }


# --- Output ---------------------------------------------------------------------------------

def print_table(title: str, t: dict) -> None:
    columns = t["columns"]
    lines = [[str(r.get(c, "")) for c in columns] for r in t["rows"]]
    lines.append(["TOTAL", *(str(t["total"].get(c, "")) for c in columns[1:])])
    widths = [max(len(c), *(len(line[i]) for line in lines)) for i, c in enumerate(columns)]
    print(f"\n=== {title} ===")
    print("  ".join(c.ljust(w) for c, w in zip(columns, widths)))
    for line in lines:
        print("  ".join([line[0].ljust(widths[0]), *(v.rjust(w) for v, w in zip(line[1:], widths[1:]))]))


def write(out: Path, name: str, data: dict) -> None:
    path = out / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--analyzed", type=Path, default=ROOT / "work" / "analyzed")
    parser.add_argument("--rewritten", type=Path, default=ROOT / "work" / "rewritten")
    parser.add_argument("--out", type=Path, default=ROOT / "results")
    args = parser.parse_args()
    for folder in (args.analyzed, args.rewritten):
        if not folder.is_dir():
            sys.exit(f"Error: '{folder}' does not exist; run the pipeline first")

    outcomes = load_records(args.rewritten, "rewrite_outcomes.json")
    tables = {
        "pointer_sources.json": ("Named pointer sources per benchmark", pointer_sources(args.analyzed)),
        "usage_types.json": ("Effective usage types of array sources", usage_types(args.analyzed)),
        "rewrite_outcomes.json": ("Rewrite outcome per usage type (owned arrays)", rewrite_outcomes(outcomes)),
        "safety.json": ("Safety of the rewritten interactions per benchmark", safety(outcomes)),
        "extra/crown_array_stats.json": ("Crown: thin and array pointers per benchmark", crown_array_stats(args.analyzed)),
    }
    for name, (title, t) in tables.items():
        write(args.out, name, t)
        print_table(title, t)
    write(args.out, "extra/crown_pointer_shapes.json", crown_pointer_shapes(args.analyzed))
    print(f"\nWritten to {args.out}/")


if __name__ == "__main__":
    main()
