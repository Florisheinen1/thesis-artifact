// Statistics produced by the pipeline, so that tables and numbers in the thesis come from the
// data instead of being copied by hand.
//
// The JSON files in /results are written by pipeline/4_results.sh (evaluation/evaluate.py) and
// committed, so the thesis compiles without running the pipeline. Paths are root-absolute, as in
// snippets.typ: the project root is the repository root, because final_project.typ lives there.
//
// ---------------------------------------------------------------------------
// The data
// ---------------------------------------------------------------------------
// Every table has the shape (columns: (..), rows: ((..), ..), total: (..)), with the rows in the
// order the thesis shows them, and only counts (no percentages: use `pct`). The first column
// labels the rows.
//
//   name                variable   contents                                 rows labeled by
//   "pointer_sources"   sources    named pointer sources per benchmark      benchmark
//   "usage_types"       usages     effective usage types of array sources   usage_type
//   "rewrite_outcomes"  outcomes   rewrite applied per usage type           usage_type
//   "safety"            safety     safety category per benchmark            benchmark
//
// Fields, e.g. `sources.total.owned_array` or `row(safety, "benchmark", "lodepng").total`; see the
// JSON files for all column names.
//
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
//   pct(part, whole)              "56%", rounded to whole percents (digits: n for decimals)
//   row(data, key, value)         the row whose `key` column equals `value`
//   result-table(name, columns)   a table of the rows plus a TOTAL row; see below
//   stats                         the derived numbers used in the running text; see below
//
// Example:
//   #result-table("pointer_sources", ("total", ("owned_array", [Owned arrays])))
//   Only #stats.owned-arrays of the #stats.arrays arrays are owned.

#let sources = json("/results/pointer_sources.json")
#let usages = json("/results/usage_types.json")
#let outcomes = json("/results/rewrite_outcomes.json")
#let safety = json("/results/safety.json")

#let _tables = (
	pointer_sources: sources,
	usage_types: usages,
	rewrite_outcomes: outcomes,
	safety: safety,
)

#let pct(part, whole, digits: 0) = {
	let value = calc.round(part / whole * 100, digits: digits)
	(if digits == 0 { str(int(value)) } else { str(value) }) + "%"
}

#let row(data, key, value) = {
	let found = data.rows.find(r => r.at(key) == value)
	assert(found != none, message: "no row with " + key + " = " + value)
	found
}

// "owned_output_param_array" -> "Owned output param array"
#let _title(id) = {
	let words = id.replace("_", " ")
	upper(words.first()) + words.slice(words.first().len())
}

// A table of one of the results, with one row per data row, followed by a TOTAL row.
//
//   name      the table, e.g. "pointer_sources" (see the list above)
//   columns   the columns to show, in order, or `auto` for all of them. Each column is one of:
//               "owned_array"                                a column; title made from its name
//               ("owned_array", [Owned arrays])              a column with your own title
//               (r => pct(r.leaves_as_raw, r.total), [Raw])  computed from each row (and the
//                                                            TOTAL row), with its title
//             The row label column (e.g. "benchmark") comes first, unless you list it yourself
//             to give it a title or another position.
//   header-style   function applied to every title except the row label's, e.g. `vertical`
//   total-label    the label of the last row
//   hide-zero      leave cells that are 0 empty, so the non-zero values stand out
//   ..args         passed on to `table`, e.g. align
//
// An unknown table or column name is an error that lists the available names.
#let result-table(name, columns, header-style: none, total-label: [TOTAL], hide-zero: false, ..args) = {
	assert(
		name in _tables,
		message: "unknown results table \"" + name + "\"; available: " + _tables.keys().join(", "),
	)
	let data = _tables.at(name)
	let key = data.columns.first()

	let specs = (if columns == auto { data.columns } else { columns }).map(spec => {
		if type(spec) == str { return (spec, _title(spec)) }
		assert(
			type(spec) == array and spec.len() == 2,
			message: "a column is \"name\", (\"name\", [Title]) or (row => value, [Title]), got " + repr(spec),
		)
		spec
	})
	for (id, _) in specs {
		if type(id) == str {
			assert(
				id in data.columns,
				message: "table \"" + name + "\" has no column \"" + id + "\"; available: " + data.columns.join(", "),
			)
		}
	}
	if not specs.any(spec => spec.first() == key) { specs.insert(0, (key, _title(key))) }

	let header = specs.map(((id, title)) => if id == key or header-style == none { title } else { header-style(title) })
	let cells(r, label) = specs.map(((id, title)) => {
		if type(id) == function { id(r) } else if id == key { label } else {
			let value = r.at(id)
			if hide-zero and value == 0 { [] } else { str(value) }
		}
	})
	table(
		columns: specs.len(),
		..args,
		table.header(..header),
		..data.rows.map(r => cells(r, r.at(key))).flatten(),
		..cells(data.total, total-label),
	)
}

// ---------------------------------------------------------------------------
// Derived numbers used in the running text
// ---------------------------------------------------------------------------
// Use as #stats.owned-arrays, or #pct(stats.safer, stats.interactions) for a percentage.

#let _owned-usage(type) = row(usages, "usage_type", type).owned_array
#let _outcome(type, outcome) = row(outcomes, "usage_type", type).at(outcome)

// Share of interactions per benchmark that still hand out a raw pointer, lowest first
#let _raw-shares = (
	safety.rows
		.map(r => (benchmark: r.benchmark, share: r.leaves_as_raw / r.total))
		.sorted(key: r => r.share)
)

#let stats = {
	let s = (
		// Chapter "Array manifestation"
		pointer-sources: sources.total.total, // all named pointer sources (7534)
		arrays: sources.total.array, // of which arrays (3013)
		owned-arrays: sources.total.owned_array, // owned arrays, output parameters excluded (114)
		owned-usages: usages.total.owned_array, // usages of owned arrays (933)
		element-accesses: _owned-usage("DereferenceElement") + _owned-usage("ReferenceElement"), // (301)
		reallocations: _owned-usage("Deallocate") + _owned-usage("Reallocate"), // deallocations and reallocations (66)

		// Chapter "Evaluation"
		interactions: outcomes.total.total, // rewritten interactions with owned arrays (1002)
		bounds-checked: safety.total.bounds_checked, // (301)
		stays-cbox: safety.total.stays_cbox, // (256)
		leaves-as-raw: safety.total.leaves_as_raw, // (445)
		// raw pointers that are only inspected: null checks and address computations (110)
		inspect-only: _outcome("NullCheck", "AsPtr") + _outcome("AddressOfPointer", "AsPtr"),
		promoted: safety.total.promoted, // borrows turned into ownership transfers (65)
		promoted-fn-results: safety.promoted_by_source_type.at("FnResult", default: 0), // of which function results (64)
		lowest-raw-share: _raw-shares.first(), // (benchmark: "libtree", share: 0.27)
		highest-raw-share: _raw-shares.last(), // (benchmark: "urlparser", share: 0.85)
	)
	// assignments of a non-owned value into an owned array, rewritten but not usages of it (69)
	s.insert("assignments-into-owned", s.interactions - s.owned-usages)
	// safer beyond automatic deallocation: bounds checked or still a CBox (557)
	s.insert("safer", s.bounds-checked + s.stays-cbox)
	// raw pointers handed to code that can access the array without bounds checks (335)
	s.insert("raw-access", s.leaves-as-raw - s.inspect-only)
	s
}
