// Live code snippets: read code from files in the repository instead of
// copying it into the thesis.
//
// Paths are resolved relative to *this* file, so always pass root-absolute
// paths ("/tools/rewriter/runtime/src/cbox.rs"). The project root is the
// repository root, because final_project.typ lives there.
//
// ---------------------------------------------------------------------------
// Selecting part of a file
// ---------------------------------------------------------------------------
//   name: "header_free"  -> the lines between the markers
//                            THESIS-SNIPPET: header_free
//                            THESIS-SNIPPET-END: header_free
//   lines: 5               -> a single line (1-based, inclusive)
//   lines: (5, 12)         -> one range; `none` as end means "until end of file"
//   lines: ((5, 12), (20, 24))
//                          -> several ranges, joined by an elision line
//   (neither)              -> the whole file
//
// ---------------------------------------------------------------------------
// Markers in source files
// ---------------------------------------------------------------------------
// Markers live in comments of any syntax (`//`, `/* */`, `#`, `;`), directly
// after the comment opener: `// THESIS-SNIPPET: x` is a marker, while a
// sentence that mentions THESIS-SNIPPET halfway is not. A colon is always
// followed by a snippet name (letters, digits, `_`, `-`); any text after the
// marker and name is a free-form note for readers of that file.
//
//   // THESIS-SNIPPET: header_free
//   // THESIS-SNIPPET-END: header_free
//       Delimit a snippet. Snippets may nest and overlap.
//
//   // THESIS-SNIPPET-HIDE left out of the thesis figure for brevity
//   // THESIS-SNIPPET-HIDE-END
//       Hide the enclosed lines from every snippet that includes them.
//
//   // THESIS-SNIPPET-HIDE: header_free debugging aid, not part of the design
//   // THESIS-SNIPPET-HIDE-END
//       Hide the enclosed lines only from the snippet `header_free`.
//
//   poison(ptr); // THESIS-SNIPPET-HIDE-LINE left out for brevity
//   poison(ptr); // THESIS-SNIPPET-HIDE-LINE: header_free only hidden there
//       Hide just this line (from every snippet, or only from the named one).
//       Where the line is not hidden, it is shown without the marker comment.
//
// Marker lines themselves are never shown. Hidden lines are replaced by a
// single elision line (`elide`), indented like the first hidden line, unless
// `elide-hidden: false`; consecutive hidden lines share one elision. Hide
// blocks cannot nest.
//
// Example:
//   #snippet("/tools/rewriter/runtime/src/allocation.rs", name: "header_free")
//   #snippet("/benchmarks/c-code/lil/lil.c", lines: (120, 127))

#let _lang-by-extension = (
	"c": "c",
	"h": "c",
	"cpp": "cpp",
	"rs": "rs",
	"sh": "sh",
	"s": "yasm",
	"asm": "yasm",
	"toml": "toml",
	"py": "python",
	"typ": "typ",
)

#let _infer-lang(path) = {
	let file = path.split("/").last()
	if not file.contains(".") { return none }
	_lang-by-extension.at(lower(file.split(".").last()), default: none)
}

#let _name-pattern = "[A-Za-z0-9_-]+"
// A marker must directly follow a comment opener, so a comment that merely
// mentions THESIS-SNIPPET in a sentence is not mistaken for one.
#let _marker-regex = regex(
	"(?://|/\*|#|;)[ \t]*THESIS-SNIPPET(-HIDE-LINE|-HIDE-END|-HIDE|-END)?(?::[ \t]*("
		+ _name-pattern
		+ "))?",
)
#let _marker-kinds = (
	"": "start",
	"-END": "end",
	"-HIDE": "hide",
	"-HIDE-END": "hide-end",
	"-HIDE-LINE": "hide-line",
)

// `none` for an ordinary line, otherwise (kind: ..., name: ...), where kind
// is one of "start", "end", "hide", "hide-end", "hide-line" and name may be
// `none`.
#let _parse-marker(line) = {
	let m = line.match(_marker-regex)
	if m == none { return none }
	let suffix = m.captures.at(0)
	(kind: _marker-kinds.at(if suffix == none { "" } else { suffix }), name: m.captures.at(1))
}

// The code of a line with a trailing THESIS-SNIPPET-HIDE-LINE comment, with
// that comment removed. Comment openers are tried from most to least
// specific, so `#[attr] // marker` and `x = 1; // marker` both work.
#let _strip-marker-comment(line) = {
	let before = line.slice(0, line.position("THESIS-SNIPPET"))
	for opener in ("//", "/*", "#", ";") {
		let found = before.matches(opener)
		if found.len() > 0 { return before.slice(0, found.last().start).trim(at: end) }
	}
	before.trim(at: end)
}

#let _fail(path, message) = panic("snippet (" + path + "): " + message)

// For every line, the hide block it lies in: `none` when not hidden,
// otherwise (name: ...) with the hide block's name (possibly `none`).
// Also checks that hide blocks are well-formed.
#let _hide-state(markers, snippet-names, path) = {
	let state = ()
	let open = none // (line: ..., name: ...) of the open hide block
	for (i, marker) in markers.enumerate() {
		let kind = if marker == none { none } else { marker.kind }
		if kind == "hide" {
			if open != none {
				_fail(
					path,
					"THESIS-SNIPPET-HIDE on line " + str(i + 1) + " opens inside the hide block of line "
						+ str(open.line + 1) + "; hide blocks cannot nest",
				)
			}
			if marker.name != none and marker.name not in snippet-names {
				_fail(
					path,
					"THESIS-SNIPPET-HIDE on line " + str(i + 1) + " names snippet `" + marker.name
						+ "`, which does not exist in this file",
				)
			}
			open = (line: i, name: marker.name)
			state.push(none)
		} else if kind == "hide-end" {
			if open == none {
				_fail(path, "THESIS-SNIPPET-HIDE-END on line " + str(i + 1) + " has no matching THESIS-SNIPPET-HIDE")
			}
			open = none
			state.push(none)
		} else if kind == "hide-line" {
			if marker.name != none and marker.name not in snippet-names {
				_fail(
					path,
					"THESIS-SNIPPET-HIDE-LINE on line " + str(i + 1) + " names snippet `" + marker.name
						+ "`, which does not exist in this file",
				)
			}
			// Inside a hide block, the block decides.
			state.push(if open == none { (name: marker.name) } else { (name: open.name) })
		} else {
			state.push(if open == none { none } else { (name: open.name) })
		}
	}
	if open != none {
		_fail(path, "THESIS-SNIPPET-HIDE on line " + str(open.line + 1) + " is never closed")
	}
	state
}

// The 0-based [start, end) line range inside the markers of snippet `name`.
#let _named-range(markers, name, path) = {
	let find(kind) = range(markers.len()).filter(i => (
		markers.at(i) != none and markers.at(i).kind == kind and markers.at(i).name == name
	))
	let starts = find("start")
	let ends = find("end")
	if starts.len() == 0 { _fail(path, "snippet `" + name + "` not found") }
	if starts.len() > 1 or ends.len() > 1 { _fail(path, "snippet `" + name + "` is defined more than once") }
	if ends.len() == 0 { _fail(path, "snippet `" + name + "` has no THESIS-SNIPPET-END") }
	let (start, end) = (starts.first(), ends.first())
	if end < start { _fail(path, "THESIS-SNIPPET-END of `" + name + "` comes before its start") }
	(start + 1, end)
}

// `lines:` as an array of 0-based [start, end) ranges.
#let _line-ranges(spec, count, path) = {
	let ranges = if type(spec) == int {
		((spec, spec),)
	} else if type(spec) == array and spec.len() == 2 and type(spec.first()) == int {
		(spec,)
	} else { spec }
	ranges.map(((start, end)) => {
		let end = if end == none { count } else { end }
		if start < 1 or end > count or end < start {
			_fail(path, "invalid line range " + str(start) + "-" + str(end) + " (file has " + str(count) + " lines)")
		}
		(start - 1, end)
	})
}

#let _is-blank(line) = type(line) == str and line.trim() == ""
#let _indent(line) = line.match(regex("^[ \t]*")).text

// A segment is an array of items: a string is a shown line, a dictionary
// (gap: line) is an elision whose indentation is taken from `line`.
#let _segment(lines, markers, hidden, from, to, name, elide-hidden) = {
	let items = ()
	let in-gap = false
	for i in range(from, to) {
		let marker = markers.at(i)
		let line = lines.at(i)
		if marker != none {
			if marker.kind != "hide-line" { continue }
			line = _strip-marker-comment(line)
			// A HIDE-LINE marker on a line of its own hides nothing.
			if line.trim() == "" { continue }
		}
		let hide = hidden.at(i)
		if hide != none and (hide.name == none or hide.name == name) {
			if elide-hidden and not in-gap { items.push((gap: line)) }
			in-gap = true
		} else {
			items.push(line)
			in-gap = false
		}
	}
	// Drop blank lines at the edges of the segment.
	while items.len() > 0 and _is-blank(items.first()) { items = items.slice(1) }
	while items.len() > 0 and _is-blank(items.last()) { items = items.slice(0, -1) }
	items
}

// The raw text of a snippet, without turning it into content. Useful for
// tests, or for passing the text to something other than `raw`.
#let snippet-text(
	path,
	name: none,
	lines: none,
	elide: "...",
	elide-hidden: true,
	dedent: true,
) = {
	if name != none and lines != none { _fail(path, "give either `name` or `lines`, not both") }
	if name != none and name.match(regex("^" + _name-pattern + "$")) == none {
		_fail(path, "`" + name + "` is not a valid snippet name")
	}

	let all = read(path).replace("\r\n", "\n").split("\n")
	// A file ending in a newline would otherwise get an extra empty "line".
	if all.len() > 0 and all.last() == "" { all = all.slice(0, -1) }

	let markers = all.map(_parse-marker)
	for (i, marker) in markers.enumerate() {
		if marker != none and marker.kind in ("start", "end") and marker.name == none {
			_fail(path, "marker on line " + str(i + 1) + " needs a name, e.g. `THESIS-SNIPPET: my_name`")
		}
	}
	let snippet-names = markers.filter(m => m != none and m.kind == "start").map(m => m.name)
	let hidden = _hide-state(markers, snippet-names, path)

	let ranges = if name != none {
		(_named-range(markers, name, path),)
	} else if lines != none {
		_line-ranges(lines, all.len(), path)
	} else {
		((0, all.len()),)
	}
	let segments = ranges.map(((from, to)) => _segment(all, markers, hidden, from, to, name, elide-hidden))

	// Join the segments with an elision, indented like the line after it,
	// and merge elisions that end up next to each other.
	let items = ()
	for (i, segment) in segments.enumerate() {
		if i > 0 and segment.len() > 0 {
			let first = segment.first()
			items.push((gap: if type(first) == str { first } else { first.gap }))
		}
		items += segment
	}
	let merged = ()
	for item in items {
		if type(item) == dictionary and merged.len() > 0 and type(merged.last()) == dictionary { continue }
		merged.push(item)
	}

	// Remove the longest whitespace prefix shared by all non-blank lines.
	let shown = merged.filter(l => type(l) == str and not _is-blank(l))
	let prefix = if dedent and shown.len() > 0 { _indent(shown.first()) } else { "" }
	if dedent {
		for line in shown {
			while not line.starts-with(prefix) { prefix = prefix.slice(0, prefix.len() - 1) }
		}
	}
	let strip(line) = if line.starts-with(prefix) { line.slice(prefix.len()) } else { line.trim(at: start) }

	merged
		.map(item => {
			if type(item) == dictionary {
				let indent = _indent(item.gap)
				let indent = if indent.starts-with(prefix) { indent.slice(prefix.len()) } else { "" }
				indent + elide
			} else if _is-blank(item) { "" } else { strip(item) }
		})
		.join("\n", default: "")
}

// A code block read live from a file. See the top of this file for how to
// select part of a file. `lang` is inferred from the file extension unless
// given; pass `lang: none` for plain text.
#let snippet(
	path,
	name: none,
	lines: none,
	lang: auto,
	elide: "...",
	elide-hidden: true,
	dedent: true,
) = {
	let lang = if lang == auto { _infer-lang(path) } else { lang }
	raw(
		snippet-text(
			path,
			name: name,
			lines: lines,
			elide: elide,
			elide-hidden: elide-hidden,
			dedent: dedent,
		),
		lang: lang,
		block: true,
	)
}
