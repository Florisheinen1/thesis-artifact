#!/usr/bin/env bash
# Checks that every code example shown in the thesis behaves as claimed:
#   c/*.c                must compile as strict C99 (warnings are allowed:
#                        some examples demonstrate undefined behavior on
#                        purpose)
#   rust/src/bin/*.rs    must compile and run without failing, or, if the
#                        file has `CHECK-ARGS:` / `CHECK-EXIT-CODE:` lines,
#                        exit with that code when run with those arguments
#                        (e.g. 101 for a program that is meant to panic)
#   rust/compile-fail/*  must fail to compile with exactly the errors listed
#                        in their `EXPECTED-COMPILE-ERROR:` lines: an error
#                        code (E0382) or a lint name (unconditional_panic)
#   generated/           must be what ./generate.sh produces now; otherwise
#                        an example changed (or the compiler did) and
#                        ./generate.sh needs to be rerun
#   line ranges          the thesis shows some files that cannot get snippet
#                        markers (benchmarks) by line number; each listed line
#                        must still contain the expected text
set -uo pipefail

# file (relative to the repository root)  line  text the line must contain
pinned_lines=(
	"benchmarks/c-code/lil/lil.c            123   static char* strclone(const char* s)"
	"benchmarks/c-code/lil/lil.c            130   }"
	"benchmarks/rust-code/lil/src/lil.rs    211   unsafe extern \"C\" fn strclone("
	"benchmarks/rust-code/lil/src/lil.rs    219   }"
)

cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

failures=0
pass() { printf '  ok    %s\n' "$1"; }
fail() { printf '  FAIL  %s: %s\n' "$1" "$2"; failures=$((failures + 1)); }

echo "C examples (must compile as C99)"
for file in c/*.c; do
	if gcc -std=c99 -pedantic-errors -Wall -c "$file" -o "$tmp/out.o" 2> "$tmp/stderr"; then
		pass "$file"
	else
		fail "$file" "does not compile"
		sed 's/^/        /' "$tmp/stderr"
	fi
done

echo "Rust examples (must compile and run)"
if cargo build --quiet --offline --manifest-path rust/Cargo.toml --bins 2> "$tmp/stderr"; then
	for file in rust/src/bin/*.rs; do
		name=$(basename "$file" .rs)
		read -r -a run_args <<< "$(grep -oP 'CHECK-ARGS:\s*\K.*' "$file" || true)"
		expected_exit=$(grep -oP 'CHECK-EXIT-CODE:\s*\K[0-9]+' "$file" || echo 0)
		rust/target/debug/"$name" "${run_args[@]}" > /dev/null 2> "$tmp/stderr"
		actual_exit=$?
		if [ "$actual_exit" -eq "$expected_exit" ]; then
			pass "$file"
		else
			fail "$file" "exited with $actual_exit, expected $expected_exit"
			sed 's/^/        /' "$tmp/stderr"
		fi
	done
else
	fail "rust/" "cargo build failed"
	sed 's/^/        /' "$tmp/stderr"
fi

echo "Rust examples (must fail to compile)"
for file in rust/compile-fail/*.rs; do
	expected=$(grep -oP 'EXPECTED-COMPILE-ERROR:\s*\K\S+' "$file" | sort -u)
	if [ -z "$expected" ]; then
		fail "$file" "declares no EXPECTED-COMPILE-ERROR"
		continue
	fi
	# A full build: some lints (unconditional_panic) only run during codegen.
	if rustc --edition 2024 -o "$tmp/out" "$file" 2> "$tmp/stderr"; then
		fail "$file" "compiles, but should not"
		continue
	fi

	expected_codes=$(grep -E '^E[0-9]{4}$' <<< "$expected" || true)
	expected_lints=$(grep -vE '^E[0-9]{4}$' <<< "$expected" || true)
	actual_codes=$(grep -oP '^error\[\KE[0-9]{4}(?=\])' "$tmp/stderr" | sort -u)

	problem=""
	if [ "$actual_codes" != "$expected_codes" ]; then
		problem="expected error codes [$(echo $expected_codes)], got [$(echo $actual_codes)]"
	fi
	for lint in $expected_lints; do
		grep -q "#\[deny($lint)\]" "$tmp/stderr" || problem="$problem; lint $lint did not fire"
	done
	if [ -z "$problem" ]; then
		pass "$file"
	else
		fail "$file" "${problem#; }"
		sed 's/^/        /' "$tmp/stderr"
	fi
done

# Removes the parts of generated files that differ per run or system, as
# declared in their "Varies per run or system" header lines.
normalise() {
	local file regexes regex
	for file in "$1"/*; do
		regexes=$(grep -oP 'Varies per run or system, ignored by check.sh: /\K.*(?=/$)' "$file" || true)
		while IFS= read -r regex; do
			[ -n "$regex" ] && sed -E -i "s|$regex||g" "$file"
		done <<< "$regexes"
	done
}

echo "Generated output (must be up to date)"
if ./generate.sh "$tmp/generated" > /dev/null 2> "$tmp/stderr"; then
	cp -r generated "$tmp/committed"
	normalise "$tmp/committed"
	normalise "$tmp/generated"
	if diff -r "$tmp/committed" "$tmp/generated" > "$tmp/diff"; then
		pass "generated/"
	else
		fail "generated/" "outdated, rerun ./generate.sh"
		sed 's/^/        /' "$tmp/diff"
	fi
else
	fail "generate.sh" "failed"
	sed 's/^/        /' "$tmp/stderr"
fi

echo "Line ranges (must still point at the code the thesis means)"
for pin in "${pinned_lines[@]}"; do
	read -r file line text <<< "$pin"
	actual=$(sed -n "${line}p" "../../$file")
	if [[ $actual == *"$text"* ]]; then
		pass "$file:$line"
	else
		fail "$file:$line" "expected '$text', found '$actual'"
	fi
done

if [ "$failures" -gt 0 ]; then
	echo "$failures example(s) failed"
	exit 1
fi
echo "All examples behave as expected"
