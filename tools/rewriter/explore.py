#!/usr/bin/env python3
"""
Exploratory analysis of C-to-Rust array data.

Generate the CSV files first:
    cargo run --release -- /path/to/benchmarks/

Then run this script from the repo root:
    python analysis/explore.py
"""

import os

import pandas as pd

DATA_DIR = os.path.dirname(__file__)
ARRAYS_CSV = os.path.join(DATA_DIR, "analysis", "arrays.csv")
USAGES_CSV = os.path.join(DATA_DIR, "analysis", "usages.csv")

# ── Load ──────────────────────────────────────────────────────────────────────

arrays = pd.read_csv(ARRAYS_CSV)
usages = pd.read_csv(USAGES_CSV)

# Merge: one row per usage, all array columns available as suffixes _usage / _def
# For most analyses you'll work with `arrays` and `usages` separately and join as needed.
merged = usages.merge(arrays, on="array_id", how="left", suffixes=("_usage", "_def"))

print(f"Arrays:  {len(arrays):>6,}")
print(f"Usages:  {len(usages):>6,}")
print(f"Merged:  {len(merged):>6,}")
print()

# ── Distributions ─────────────────────────────────────────────────────────────

print("=== Arrays per project ===")
print(arrays.groupby("project").size().sort_values(ascending=False).to_string())
print()

print("=== Ownership type ===")
print(arrays["ownership_type"].value_counts().to_string())
print()

print("=== Memory type ===")
print(arrays["memory_type"].value_counts().to_string())
print()

print("=== Origin kind ===")
print(arrays["origin_kind"].value_counts().to_string())
print()

print("=== Usage type ===")
print(usages["usage_type"].value_counts().to_string())
print()

# ── Cross-cuts ────────────────────────────────────────────────────────────────

print("=== Usage types by ownership ===")
cross = merged.groupby(["ownership_type", "usage_type"]).size().unstack(fill_value=0)
print(cross.to_string())
print()

print("=== Usage types by memory type ===")
cross2 = merged.groupby(["memory_type", "usage_type"]).size().unstack(fill_value=0)
print(cross2.to_string())
print()

# ── Usages per array ──────────────────────────────────────────────────────────

usage_counts = usages.groupby("array_id").size().rename("n_usages")
arrays_with_counts = arrays.join(usage_counts, on="array_id").fillna({"n_usages": 0})
arrays_with_counts["n_usages"] = arrays_with_counts["n_usages"].astype(int)

print("=== Top 10 most-used arrays ===")
top = arrays_with_counts.nlargest(10, "n_usages")[
    ["project", "name", "item_path", "ownership_type", "memory_type", "n_usages"]
]
print(top.to_string(index=False))
print()

print("=== Arrays with zero usages ===")
zero = arrays_with_counts[arrays_with_counts["n_usages"] == 0]
print(f"  {len(zero)} arrays ({100 * len(zero) / max(len(arrays), 1):.1f}%)")
print()

# ── Scratch space ─────────────────────────────────────────────────────────────
# Add your own analyses below. Some useful starting points:
#
# Filter to owned heap arrays only:
#   owned_heap = arrays[(arrays.ownership_type == "Owned") & (arrays.memory_type == "Heap")]
#
# All allocations across projects:
#   allocs = usages[usages.usage_type == "SetNewAllocation"]
#
# Per-project ownership breakdown:
#   arrays.groupby(["project", "ownership_type"]).size().unstack(fill_value=0)
#
# Arrays that are deallocated at least once:
#   freed_ids = usages[usages.usage_type == "Deallocate"]["array_id"].unique()
#   freed_arrays = arrays[arrays.array_id.isin(freed_ids)]
