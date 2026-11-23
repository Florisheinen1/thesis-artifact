#!/usr/bin/env python3
"""
Aggregate pointer_sources.json and array_usages.json (written by
`rewriter analyze`, see analyze_project() in src/main.rs) across a set of
benchmarks into two summary tables:

  1. Per benchmark: total pointer sources, array (fat) pointer sources,
     owned array pointer sources, non-owned array pointer sources.
  2. Across all benchmarks: effective usage type variant, amount of owned
     array usages, amount of non-owned array usages.

Each table is printed once as plain text and once as a Typst #table(..).

Usage:
    python plot.py <benchmarks_dir>

Example:
    python plot.py ../input
    python plot.py ../output
"""

import json
import sys
from pathlib import Path

import pandas as pd


def to_typst(table: pd.DataFrame) -> str:
    columns = [""] + list(table.columns)
    rows = [[idx] + [str(v) for v in row] for idx, row in table.iterrows()]

    def fmt_row(cells):
        return "\t" + " ".join(f"[{c}]," for c in cells)

    col_count = len(columns)
    lines = [
        "#table(",
        f"\tcolumns: {col_count},",
        "\ttable.header(",
        fmt_row(columns),
        "\t),",
    ] + [fmt_row(row) for row in rows] + [
        ")",
    ]
    return "\n".join(lines)


def load_json_records(benchmarks_dir: Path, filename: str) -> pd.DataFrame:
    records = []

    for bench in sorted(benchmarks_dir.iterdir()):
        if not bench.is_dir():
            continue

        path = bench / "analysis_results" / filename
        if not path.exists():
            continue

        with path.open() as f:
            data = json.load(f)

        # The records themselves carry no project/benchmark field - each
        # file already lives under one project's own analysis_results/, so
        # we key off the containing directory name instead.
        for row in data:
            row["benchmark"] = bench.name
            records.append(row)

    return pd.DataFrame(records) if records else pd.DataFrame()


# Names of the extern C allocator/deallocator pointer sources emitted by
# get_pointer_sources_of_extern_c_functions() (src/pointer_source.rs). C2Rust
# redeclares `extern "C" { fn malloc(...); }` etc. in every file that calls
# them, so one file-local `extern` declaration becomes one NamedPointerSource
# each - a benchmark that calls malloc from 10 files gets 10 "malloc" rows.
# They're the same C function for counting purposes, so collapse them to one
# row per benchmark.
EXTERN_ALLOC_SOURCE_NAMES = {"malloc", "calloc", "realloc", "realloc_old_pointer", "free"}


def dedupe_extern_alloc_sources(df: pd.DataFrame) -> pd.DataFrame:
    is_extern_alloc = df["name"].isin(EXTERN_ALLOC_SOURCE_NAMES)
    deduped_extern = df[is_extern_alloc].drop_duplicates(subset=["benchmark", "name"])
    return pd.concat([df[~is_extern_alloc], deduped_extern], ignore_index=True)


def source_summary_table(df: pd.DataFrame) -> pd.DataFrame:
    rows = []
    for bench, g in df.groupby("benchmark"):
        arr = g[g["is_fat"]]
        rows.append(
            {
                "benchmark": bench,
                "total_pointer_sources": len(g),
                "array_pointer_sources": len(arr),
                "owned_array_pointer_sources": int(arr["is_owned"].sum()),
                "non_owned_array_pointer_sources": int((~arr["is_owned"]).sum()),
            }
        )

    t = (
        pd.DataFrame(rows)
        .set_index("benchmark")
        .sort_values("total_pointer_sources", ascending=False)
    )

    total_row = pd.DataFrame(
        [[t[c].sum() for c in t.columns]],
        columns=t.columns,
        index=pd.Index(["TOTAL"], name=t.index.name),
    )
    return pd.concat([t, total_row])


def usage_summary_table(df: pd.DataFrame) -> pd.DataFrame:
    all_types = sorted(df["usage_type"].unique())
    owned = df[df["source_is_owned"]].groupby("usage_type").size()
    non_owned = df[~df["source_is_owned"]].groupby("usage_type").size()

    t = (
        pd.DataFrame(
            {"amount_owned_array": owned, "amount_non_owned_array": non_owned},
            index=all_types,
        )
        .fillna(0)
        .astype(int)
        .rename_axis("effective_usage_type")
        .sort_values("amount_owned_array", ascending=False)
    )

    total_row = pd.DataFrame(
        [[t[c].sum() for c in t.columns]],
        columns=t.columns,
        index=pd.Index(["TOTAL"], name=t.index.name),
    )
    return pd.concat([t, total_row])


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    benchmarks_dir = Path(sys.argv[1])
    if not benchmarks_dir.is_dir():
        print(f"Not a directory: {benchmarks_dir}", file=sys.stderr)
        sys.exit(1)

    pd.set_option("display.max_columns", None)
    pd.set_option("display.width", 160)

    sources_df = load_json_records(benchmarks_dir, "pointer_sources.json")
    if sources_df.empty:
        print(f"No pointer_sources.json files found under {benchmarks_dir}", file=sys.stderr)
        sys.exit(1)
    sources_df = dedupe_extern_alloc_sources(sources_df)

    source_table = source_summary_table(sources_df)
    print("=== Pointer sources per benchmark ===")
    print(source_table.to_string())
    print()
    print(to_typst(source_table))

    usages_df = load_json_records(benchmarks_dir, "array_usages.json")
    if usages_df.empty:
        print(f"\nNo array_usages.json files found under {benchmarks_dir}", file=sys.stderr)
        sys.exit(0)
    # array_usages.json now also carries usages of non-array (non-fat) pointer
    # sources; this table is about array usage types specifically.
    usages_df = usages_df[usages_df["source_is_fat"]]

    print()
    usage_table = usage_summary_table(usages_df)
    print("=== Effective array usage types (owned vs. non-owned) ===")
    print(usage_table.to_string())
    print()
    print(to_typst(usage_table))


if __name__ == "__main__":
    main()
