
#import "thesis/template.typ": *

#import "thesis/snippets.typ": snippet

#import "thesis/results.typ": pct, result-table

#import "@preview/cetz:0.4.1": canvas, draw

#show: template

#let vertical(body) = rotate(-90deg, reflow: true, body)

#titlepage(
  title: "Safe Array Translation for C2Rust",
  author: "Floris Heinen",
  studentNumber: "s2299909",
  study: "Master Computer Science",
  committee: (
    "dr.ir. V. Zaytsev",
    "dr.ir. T.S. van Ede",
  ),
  date: "November 2026",
)

#basic_header(outline())

#pagebreak()

= Introduction
For decades, embedded systems, cloud infrastructure, and other high-performance critical applications have been written in C. C is old, it has been in active use since the early 1970s, and its age shows in the codebases it has produced. Lehman's law of software evolution tells us that programs either grow in complexity over time or become legacy systems @lehman_evolution. Even still, the language has consistently ranked in the top five of the Tiobe index since 1990 @tiobe_index. This has resulted in the world having accumulated a massive body of complex and legacy C code.

What makes this particularly concerning is that C is also deeply unsafe. The language was designed to give programmers maximum control over hardware, and safety checks were left as the programmer's responsibility. The result is that projects can contain bugs like out-of-bounds memory accesses, use-after-free errors, and double frees. These are not just possible but common. Over 70% of the severe security bugs found in the Chromium Project are caused by memory-related unsafety @chromium_vulnerabilities. High-profile vulnerabilities like Heartbleed in OpenSSL @heartbleed_analysis, EternalBlue in the Windows operating system @liu_eternal_blue, and StageFright in Android @akcay_androids_2016 were all rooted in memory unsafety, and together allowed attackers to steal secrets, execute arbitrary code, and encrypt victims' data. These are not hypothetical risks; they are documented catastrophes in production systems.

#figure(
  ```c
  int tls1_process_heartbeat(SSL *s) {
  	unsigned char *p = &s->s3->rrec.data[0], *pl;
  	unsigned int payload;
  	unsigned int padding = 16;

  	n2s(p, payload); // length read straight from attacker's request, never validated
  	pl = p;

  	buffer = OPENSSL_malloc(1 + 2 + payload + padding);
  	bp = buffer;

  	*bp++ = TLS1_HB_RESPONSE;

  	memcpy(bp, pl, payload); // Copies 'payload' bytes regardless of the record's real size
  }
  ```,
  caption: "Code snippet of the OpenSSL implementation that allowed Heartbleed. The unchecked payload length is assumed to be correct, and that many bytes is copied from. If the actual payload length is smaller, bytes from neighboring allocations get copied with memcpy instead. Less relevant lines have been left out for brevity, and comments added for clarity.",
  placement: auto,
)<figure_heartbleed_snippet>

Migrating existing C codebases to another safer language would seem like the obvious response. The reality is more complicated. Lehman also observes that usually migration neither improves a project's complexity nor its legacy nature, contrary to popular belief. These problems travel with the code @lehman_evolution. Manual rewrites of millions of lines are laborious and reintroduce logical bugs that years of debugging had already eliminated. Incremental migration, rewriting module by module, results in mixed-language projects that combine the unsafety of C with the overhead of foreign function interface (FFI) boundaries and an entirely new class of interoperability bugs @yang_multi_language_issues. Neither approach is attractive.

There is, however, one reason migration to a safe language may still be worth it despite these costs: safety can be a pass/fail transition. Unlike the reduction of complexity, a complete migration to a safe language can guarantee entire categories of bugs are eradicated once the new language accepts the translated code. A correctly rewritten program simply cannot produce a vulnerability like Heartbleed. For this reason, organizations and governmental bodies such as the CISA @CISA, Google @google_safety and Microsoft @microsoft_safety created a push towards these safer languages, making this not just a technical question but a current and urgent one. The code responsible for Heartbleed is shown in @figure_heartbleed_snippet.

The problem is that most memory-safe languages introduce runtime overhead, through garbage collection and reference counting for example. That makes them unsuitable for the performance-critical domain where C lives. Rust fills this gap. It offers performance nearly equal to C @okawa_rust_performance while guaranteeing safety through the borrow checker, a compile-time system that enforces strict ownership rules and eliminates most severe memory safety bugs without any runtime cost @rust_safety. Migration to Rust is therefore the migration worth attempting.

The question is how to perform that migration at scale. Three broad strategies exist. Writing the new codebase from scratch is impractical for large projects. Incremental rewriting, as noted above, creates long-lived mixed-language systems with their own problems. The most appealing option is an automated translation tool, often considered a "silver bullet" that rewrites the entire codebase at once, preserving the original logic while producing safe Rust.

Several tools approach this goal. LLM-based translators such as EvoC2Rust @wang_evoc2rust and type-migrating approaches @hong_type_migrating produce idiomatic Rust, at the cost of translational correctness. On the other end of the spectrum, C2Rust @C2Rust translates C syntactically to Rust, preserving program semantics exactly, but only produces code that is almost entirely unsafe. While neither approach is perfect, the latter can serve as an excellent starting ground for further improvement to put the output into the safe Rust constructs. Such examples of tools are Laertes, @emre_translating and Crown @zhang_crown, which infer ownership and replace raw pointers with safe references, while Nopcrat @hong_nopcrat and GenC2Rust @wu_genc2rust make the resulting code more idiomatic by eliminating output parameters and inferring generics. Together these tools progressively close the gap between C2Rust output and natively written Rust.

Yet all of these tools, which prioritize preserving program correctness over idiomatic output, share a blind spot: they explicitly exclude arrays from their scope. Converting C style arrays to Rust idiomatic arrays comes with a group of complexities. In C, an array is typically represented as a raw pointer. This means the array's length is not automatically stored alongside it, automatic bounds checking does not exist, and memory management is manual. When C2Rust translates such a pointer, the result is an unidiomatic raw pointer in Rust too, which acts similar to C's pointer without the benefits Rust could offer. An extensive analysis is required in order to figure out the right metadata of the arrays, and find a suitable, safe and idiomatic alternative in Rust, which none of the existing tools perform. This has left most arrays unsafe in existing automated migration.

This thesis addresses that gap. Rather than designing a new complete and sound ownership analysis aimed at arrays, we make a deliberate choice to build on Crown's existing ownership and fatness analysis, accepting that it is neither complete nor sound. Our goal is not to solve the migration of arrays completely, but to reduce the unsafe surface area of translated programs. Every array that no longer has to be managed through raw pointers is one less source of memory bugs, and makes subsequent manual refactoring less error-prone. Arrays whose ownership cannot be established are deliberately left untouched.

This leads to the following main research question:

#block(inset: (left: 1em))[
  *How can Crown's ownership analysis be used to automatically rewrite owned heap arrays in C2Rust output into safe Rust, while preserving the behavior of the program?*
]

We divide this question into four sub-questions:

#block(inset: (left: 1em))[
  *RQ1*: How do C arrays appear in C2Rust output, and how many of them can be identified as owned heap arrays with Crown's analysis? \
  *RQ2*: What safe Rust type can represent an owned C heap array, while keeping the memory representation of the original raw pointer? \
  *RQ3*: How can C2Rust output be rewritten automatically to use that type, and under which assumptions is the rewrite correct? \
  *RQ4*: To what extent does the rewrite reduce the unsafe surface area of the translated programs, while preserving their behavior?
]

In answering these questions, we make two contributions: (1) an empirical analysis of how C arrays manifest in C2Rust translated code, characterizing the types and ownership patterns that appear in practice, which answers RQ1; and (2) a safe array type, `CBox`, together with a tool that automatically rewrites owned heap arrays to use it, driven by Crown's ownership and fatness analysis, which answers RQ2 and RQ3. We evaluate the rewriter on 20 benchmarks to answer RQ4, and show that all 114 owned arrays with their 1002 interactions are rewritten, of which 301 interactions become bounds checked, while all benchmarks still compile.

The remainder of this thesis is structured as follows. @chapter_background provides background on code migration, and on unsafety in C and Rust. @chapter_state_of_the_art discusses the tools this work builds on. @chapter_array_manifestation presents the empirical array analysis, answering RQ1. @chapter_safe_array_type introduces the safe array type, answering RQ2. @chapter_rewrite_methodology describes the rewrite methodology, and @chapter_correctness states under which assumptions it is correct, together answering RQ3. @chapter_evaluation evaluates the rewriter, answering RQ4, followed by its limitations in @chapter_limitations. @chapter_conclusion concludes.

#pagebreak()

= Background <chapter_background>

== Code migrations
When migrating code from one language to another, not every source language construct has a direct equivalent in the target language. Terekhov @terekhov_realities_of_conversion distinguishes three cases:
1. a construct can translate natively (both languages express it the same way),
2. it can be simulated (the target language can approximate the behavior using different constructs),
3. or the construct is simply absent in the target language.

Simulated language constructs come with significant downsides. Getting a simulated construct to behave exactly like in the other language is difficult, as it often contains unexpected side-effects. Additionally, simulating constructs leads to boilercode repeated throughout a codebase, resulting in less maintainable code. After such translations are finished, extra work is needed to clean up the code and make it more idiomatic again.

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*Shared native language constructs*],
      colspan: 2,
    ),
    table.cell(
      align: center,
      [*C code*],
    ),
    table.cell(
      align: center,
      [*Rust code*],
    ),
    snippet("/thesis/examples/c/while_loop.c", name: "while_loop"),
    snippet("/thesis/examples/rust/src/bin/while_loop.rs", name: "while_loop"),
  ),
  caption: "Both C and Rust natively support the use of `while` loops",
)

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*Simulated language constructs*],
      colspan: 2,
    ),
    table.cell(
      align: center,
      [*C code*],
    ),
    table.cell(
      align: center,
      [*Rust code*],
    ),
    snippet("/thesis/examples/c/c_int.c", name: "c_int"),
    snippet("/thesis/examples/rust/src/bin/c_int.rs", name: "c_int"),
  ),
  caption: "Data types in C are differently defined than in Rust. The libc library simulates these types.",
)

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*Absent language constructs*],
      colspan: 2,
    ),
    table.cell(
      align: center,
      [*C code*],
    ),
    table.cell(
      align: center,
      [*Rust code*],
    ),
    snippet("/thesis/examples/c/variable_length_array.c", name: "variable_length_array"),
    ```rust
    // Not possible: stack allocation size must be known at compile time
    ```,
  ),
  caption: "While C allows variably sized arrays on the stack, it is impossible to create them in (unsafe) Rust.",
)

== Unsafety
When talking about (un)safety in programming languages, many different definitions and categories exist. One such category, memory safety, is about how a programming language prevents the running program from corrupting its memory. The disadvantage of corrupt memory is that it prevents the ability to check if memory has been corrupted, as the code that is supposed to check for this might have been affected too. Another category, type safety, protects the programmer from misinterpreting one type as another. If there is no guard against such accidental misinterpretations, a numerical value can be mistakenly interpreted as a character, for example. It is impossible to check for these accidents afterwards, as the underlying memory is exactly the same. These two examples highlight a common ground: The inability to detect something went wrong after a mistake happened. This shortcoming prevents programmers from creating a failsafe in case something does go wrong. Additionally, unsafety allows programs to continue running even after unsafe behavior happened, possibly resulting in even worse situations. In this work, we use a lack of protection against these irrecoverable errors as the definition of general unsafety.

== Unsafety in C
The C language is regarded as a very unsafe language. This is by design, as C trades safety for programmer control and performance. The result is a language that allows for accidentally accessing memory that it does not own, a language that allows for interpreting any piece of memory as any type, but a language with compilers that can optimize code extremely well. Wrong usage of the language is therefore also left as undefined behavior, instead of forcing compilers to insert runtime checks that prevent this. This unfortunately can result in unexpected behavior from compilers. Take for example @undefined_behavior_optimized_away. Here, because dereferencing a nullpointer is undefined behavior and is assumed to never happen, the compiler can assume the pointer cannot be null. That means the test for checking if `array` is a nullpointer is redundant, and is optimized away. This results in a program that does not check for null, even though it is explicitly written by the programmer. Another example is accessing variables of an array in C in @accessing_out_of_bounds. A programmer can accidentally access a piece of memory that is not within the allocated memory of the array, which is undefined behavior. Due to the lack of safety checks, the program could continue to run normally, or it could crash. In both scenarios, it is difficult to figure out what piece of code was at fault.

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*C code*],
    ),
    table.cell(
      align: center,
      [*Compiled assembly*],
    ),
    snippet("/thesis/examples/c/null_check_after_dereference.c", name: "null_check_after_dereference"),
    snippet("/thesis/examples/generated/null_check_after_dereference.s", name: "assembly"),
    snippet("/thesis/examples/c/null_check_before_dereference.c", name: "null_check_before_dereference"),
    snippet("/thesis/examples/generated/null_check_before_dereference.s", name: "assembly"),
  ),
  caption: "Example of how undefined behavior can result in unexpected behavior with compiler optimizations turned on. Because C assumes dereferenced values will never be done on nullpointers, testing against nullpointers after dereferencing is useless, and therefore optimized out. This can be seen in the assembly, where the jump expression is left out depending on the order of the nullpointer check and the dereference.",
  placement: auto,
) <undefined_behavior_optimized_away>

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*C code*],
    ),
    table.cell(
      align: center,
      [*Rust equivalent*],
    ),
    snippet("/thesis/examples/c/out_of_bounds.c", name: "out_of_bounds"),
    snippet("/thesis/examples/rust/src/bin/out_of_bounds.rs", name: "out_of_bounds"),
    table.cell(
      colspan: 2,
      align: center,
      [*Terminal output*],
    ),
    snippet("/thesis/examples/generated/out_of_bounds_c.txt", name: "run", lang: "sh"),
    snippet("/thesis/examples/generated/out_of_bounds_rust.txt", name: "run", lang: "sh"),
  ),
  caption: "Accessing elements outside arrays in C and Rust, where C compiles with undefined behavior and Rust simply panics.",
  placement: auto,
) <accessing_out_of_bounds>

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*C code*],
    ),
    table.cell(
      align: center,
      [*Safe Rust alternative*],
    ),
    snippet("/thesis/examples/c/double_free.c", name: "double_free"),
    snippet("/thesis/examples/rust/compile-fail/double_free.rs", name: "double_free"),
  ),
  caption: "Deallocating the same data twice in C and in Rust. C compiles the double free with undefined behavior, while Rust's ownership rules prevent the compilation of this code. In this case, some C compilers do emit a warning about a double free, but in more complex scenarios, such warnings are not guaranteed.",
  placement: auto,
) <deallocating_twice>

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*Aliasing pointers*],
    ),
    table.cell(
      align: center,
      [*Non-aliasing pointers*],
    ),
    snippet("/thesis/examples/rust/src/bin/aliasing.rs", name: "aliasing"),
    snippet("/thesis/examples/rust/src/bin/aliasing.rs", name: "non_aliasing"),
    table.cell(
      colspan: 2,
      align: center,
      [*Compiled assembly*],
    ),
    snippet("/thesis/examples/generated/aliasing_foo.s", name: "assembly"),
    snippet("/thesis/examples/generated/non_aliasing_foo.s", name: "assembly"),
  ),
  caption: "Two Rust code snippets that show a function that takes possibly aliasing or non-aliasing pointers. The compiled assembly shows how the non-aliasing function can be better optimized through a hardcoded return value.",
  placement: auto,
) <aliasing_optimization>


== Safety in Rust
Rust is designed to provide both safety and performance. When a runtime safety check fails, the program panics rather than continuing in an undefined state, as shown in @accessing_out_of_bounds. However, relying on too many runtime checks hurts performance, so Rust instead moves as many checks as possible to compile time. Doing so requires the compiler to guarantee that every reachable program state is safe, which in turn requires a set of strict rules. One such rule is that raw pointers cannot be dereferenced in safe code, since their validity is not guaranteed. Other rules come from the borrow checker, which enforces two properties: every piece of data has a single owner, as shown in @ownership_uniqueness, and at any point data may be borrowed either by many immutable references or by exactly one mutable reference, but not both, as shown in @mutable_mutual_exclusivity.

#figure(
  snippet("/thesis/examples/rust/compile-fail/ownership_uniqueness.rs", name: "ownership_uniqueness"),
  caption: "Example of how each piece of data must have a single owner. When ownership is moved, previously made references are invalidated.",
) <ownership_uniqueness>

#figure(
  snippet("/thesis/examples/rust/compile-fail/mutable_exclusivity.rs", name: "mutable_exclusivity"),
  caption: "Example of the rule that a single mutable reference and many immutable references are mutually exclusive.",
) <mutable_mutual_exclusivity>

These two properties enable several further guarantees. Firstly, they make it possible to determine at compile time when an object should be deallocated, as illustrated in @deallocating_twice. The double free scenario cannot be compiled in safe Rust. Secondly, because the compiler can guarantee the absence of pointer aliasing due to the borrow checker rules, it does not need to insert runtime checks to account for it, allowing for more aggressive optimizations, as shown in @aliasing_optimization. Thirdly, because only one thread may hold a mutable reference to any piece of data at a time, data races are prevented.

== Unsafety in Rust
While this strictness guarantees correctness, it is also overly restrictive: some programming paradigms that are known to be safe cannot be expressed in safe Rust at all. A circular linked list, for instance, has no single, well-defined owner for each node, even though such a structure can be implemented correctly. Similarly, low-level work such as directly manipulating hardware is fundamentally impossible under the borrow checker's rules, yet is sometimes required.

To accommodate these cases, Rust provides an unsafe context, marked by the `unsafe` keyword, in which some of the language's safety rules are relaxed. Within an unsafe block, it becomes possible to dereference raw pointers and to call functions that are themselves marked unsafe, as shown in @rust_unsafety. Because actions like dereferencing a raw pointer could go wrong, it is up to the developer to ensure it is only done in correct ways. Using `unsafe` is therefore a contract the developer makes with the compiler, which promises the compiler that all the unsafe code is correct, even though the compiler cannot verify this itself. Important to note here is that the borrow checker is not disabled in unsafe blocks. This means that if a safe mutable reference and a raw mutable pointer to the same object are both active inside an unsafe block, the assumption that the mutable reference is the unique pointer to that object is broken, which is undefined behavior.

#figure(
  snippet("/thesis/examples/rust/src/bin/raw_pointer_dereference.rs", name: "raw_pointer_dereference"),
  caption: "Using unsafe to dereference a raw pointer. Without the unsafe context, dereferencing is not allowed in safe Rust.",
) <rust_unsafety>

== Unsafety abstraction in Rust
This safety contract, however, is difficult to uphold consistently, especially in larger teams: it is easy, for example, to forget that a function cannot be called with a null pointer, or to forget to check if a given index is still within bounds. To manage this, Rust allows unsafe code to be encapsulated behind a safe interface. A function whose correctness depends on conditions the compiler cannot check should itself be marked `unsafe`, placing the burden of satisfying those conditions on its caller. Conversely, a function that guarantees safety for every way it can possibly be called may hide its internal use of unsafe code behind a safe signature, hiding a layer of unsafety from its users.

Such an abstraction is called sound if it remains safe under every possible use. Verifying the soundness of safety abstractions must be done by the developers, as the compiler cannot do this automatically. @unsafe_soundness shows two functions that perform the same operation under the same precondition, yet only one of them correctly marks itself as unsafe. The other function is an unsound abstraction, since calling it incorrectly cannot be prevented by the compiler. The Rust standard library itself contains a substantial amount of unsafe code, but through extensive review and collaboration this code is considered both safe and sound, allowing all other users to build on the standard library without fear of causing undefined behavior.

#figure(
  table(
    align: left,
    columns: (1fr, 1fr),
    table.cell(
      align: center,
      [*Unsound safe function*],
    ),
    table.cell(
      align: center,
      [*Sound unsafe function*],
    ),
    snippet("/thesis/examples/rust/src/bin/soundness.rs", name: "unsound"),
    snippet("/thesis/examples/rust/src/bin/soundness.rs", name: "sound"),
  ),
  caption: "Two functions using unsafe Rust, both having the precondition that `ptr` must not be a nullpointer, as that would cause undefined behavior due to the dereference. Only the right function warns callers that there is a safety requirement that needs to be adhered to that the function itself does not guarantee.",
) <unsafe_soundness>

== Rust (array) pointers
Pointers in Rust come in several forms, each carrying different guarantees. A raw pointer of any sized object is the direct equivalent of a C pointer: it simply contains the address of the data it points to, without any further guarantees. A `Box<T>` is Rust's safe alternative for owning heap-allocated data. Lastly, a (mutable) reference represents a borrow of data, governed by the aliasing rules described above.

To represent C-style arrays, Rust offers two alternatives. A Rust array has a length that is known at compile time and encoded in its type, so a pointer to it remains a "thin pointer", just like a raw pointer. A slice, by contrast, does not encode its length in its type, but instead dynamically stores the length of the slice alongside the pointer, resulting in a fat pointer. Because this length is in both cases always available, either at compile time for arrays or at runtime for slices, Rust can perform bounds checks that prevent out-of-bounds accesses, as shown in @accessing_out_of_bounds.

When an array or slice is allocated on the heap, it can be placed inside a `Box`. If the boxed value has a compile-time-known size, such as a Rust array, the resulting `Box` remains a thin pointer. If the boxed value is a slice, however, its length is stored alongside the pointer inside the `Box` itself, making the `Box` a fat pointer as well.

#pagebreak()

= State of the art <chapter_state_of_the_art>
This section contains the current state of the art related to C to Rust translation. First, covering the works that directly influence this research. Second, related works that attempt to solve the same problem differently, or a related problem.

== C2Rust
C2Rust @C2Rust is an industrial tool that translates C99-compliant C to Rust while preserving functional correctness through syntactic rewrites. It bridges the differences between the two languages through a combination of FFI, native Rust constructs, and simulation. C functions are called through FFI bindings provided by the libc crate, and C types that differ from Rust's are mapped using libc's type definitions. Constructs with no direct Rust equivalent are simulated: variable-length arrays, which Rust does not support, are replaced with heap-allocated arrays, and C's text-based macros, which have no equivalent in Rust's macro system, are fully expanded before translation. Because C2Rust operates at the syntactic level and C organizes code into modules, the output follows the same module structure. However, unlike C, Rust does not allow duplicate definitions across modules, so some additional manual refactoring is typically needed to make the translated project compile.

In C, references, heap objects, and arrays are all represented as thin pointers. C2Rust's output reflects this directly, as all those values become `*mut T` raw pointers. To avoid incorrectly restricting any pointer, C2Rust conservatively marks all of them as mutable, which is correct but hides whether a pointer is actually written to. Arrays get the same treatment: instead of using a fat pointer to a slice, which would carry the array's length alongside its address, C2Rust always emits a thin pointer to the first element, with subsequent elements accessed through pointer arithmetic. This means no length information is preserved for any array in C2Rust's output.

The translated result, while semantically correct, is entirely wrapped in `unsafe` blocks, even in code that would not require it. Together with the large number of raw pointers and other C idioms carried over verbatim, the output is therefore highly unidiomatic, as illustrated in @c2rust_example. Nevertheless, because C2Rust faithfully preserves the semantics of the original program in valid Rust syntax, its output is an ideal starting point for downstream tools to improve safety and idiomaticity without having to re-derive the program's logic from scratch.

#figure(
  table(
    columns: (auto, 1fr),
    align: (left),
    table.cell(
      align: center,
      [*C code*],
    ),
    table.cell(
      align: center,
      [*C2Rust output*],
    ),
    snippet("/benchmarks/c-code/lil/lil.c", lines: (123, 130)),
    snippet("/benchmarks/rust-code/lil/src/lil.rs", lines: (211, 219)),
  ),
  caption: "Example of how C2Rust translates C syntactically, resulting in equally unsafe Rust. Code snippet taken from lil benchmark",
) <c2rust_example>

== Laertes <laertes_work>

Emre et al. @emre_translating were the first to work on improving the output of C2Rust. They began by analyzing the causes of unsafety in C2Rust translations, categorizing them to identify which were most worth targeting. The most prominent cause turned out to be raw pointers. As part of this analysis, they further classified raw pointers by the reason they appear. Pointers that are involved in pointer arithmetic (`PtrArith`), likely represent arrays, since elements in arrays are accessed through pointer arithmetic as well. They note that such pointers could in principle be rewritten as safe slices, but left this out of their scope.

For the remaining raw pointers, they built a tool called Laertes that takes a compiler-guided approach. Laertes begins by optimistically rewriting all raw pointer uses into safe references, not taking into account yet mutability, aliasing and lifetimes. It then repeatedly attempts to compile the result, and for each compilation error either tries a less permissive alternative, such as making a reference mutable, or reverting a safe reference back into a raw pointer. Because the Rust compiler can prove safety properties about lifetime and mutability, any version that successfully compiles is guaranteed to be safe in that regard. A key finding from their evaluation is that most functions are unsafe due to multiple causes simultaneously, meaning that addressing a single cause of unsafety yields limited overall improvement.

== Aliasing limits <aliasing_work>

In a follow-up paper, Emre et al. @emre_aliasing investigate why the Laertes approach succeeds on only a fraction of pointers, and how far it can be pushed. Their first contribution is a technique called _pseudo-safety_, which removes all causes of unsafety other than ownership and lifetime from a program. This means they replaced for example pointer arithmetic with substitutes that preserve the static aliasing and lifetime relationships the Rust typechecker reasons about, but not the program's runtime behavior. While this does change the semantics of the program, meaning the change is not a valid translation, it does allow Laertes to give more meaningful results, as now all remaining pointers are directly eligible for the analysis.

Applied to their benchmark suite, Laertes can still only rewrite 12% of the remaining pointers. Laertes models the Rust typechecker as an equality-based, Steensgaard-style analysis, which groups all pointers that flow into each other into type equivalence classes. Because of that, when even a single pointer is inherently unsafe, that entire class of pointers now must also be kept unsafe. This propagation of unsafety ends up being severe for larger projects. In the worst case, a handful of inherently unsafe pointers is enough to force 85% of all pointers in the program to be marked unsafe, even though the other pointers do not necessitate unsafe behavior.

== Crown <section_crown>

Zhang et al. @zhang_crown take a fundamentally different approach from Laertes in @laertes_work. Rather than using the Rust compiler as an oracle and iteratively rewriting pointers until compilation succeeds, Crown performs a static ownership analysis on C2Rust output directly. It infers three type qualifiers for each pointer: *Ownership* (whether this pointer owns its data), *Mutability* (whether this data is ever written to), and *Fatness* (whether the pointer points to an array). The ownership analysis is the most extensive. Determining who owns an object typically requires knowing which pointers alias, which is expensive to do precisely. Crown avoids this by introducing a strengthening assumption called _ownership monotonicity_, which states that along any access path, ownership can only decrease. For example, consider a pointer `p` to a struct with a field `next`. If `p->next` is owning, then `p` must be owning as well. This approach is more restrictive than the borrow checker, disallowing some safe Rust patterns, but it does help in scaling to larger real-world codebases without needing aliasing information.
Because Crown does not group pointers into type equivalence classes, it avoids the propagation of unsafety that limits Laertes.

The ownership analysis is used to generate a system of ownership constraints, derived from assignment statements, known C library functions, and the ruleset of ownership monotonicity. Solving these constraints is shown to be NP-complete, and is done through a SAT solver. If no solution exists, the affected pointers are left as raw, indicating either a memory leak or an ownership scheme that violates ownership monotonicity.

For non-array pointers where a solution is found, owning pointers are retyped to `Option<Box<T>>`. The `Option` type here is used for null pointer values, as `Box` has the invariant that the pointer must always be valid. One exception is output parameters, where function parameters are mutable pointers to which the function's result is written. While these variables are non-owning, the fields reachable through them may be owning. Crown detects these exceptions, and rewrites these function parameters as a mutable reference `&mut T` instead. Possible type errors are detected during translation, and are reverted in such cases.

Crown generates ownership constraints only for heap-allocated objects. A pointer that could point to either a heap object or a stack array depending on control flow receives no constraints, so the SAT solver assigns it as non-owning, which in turn leaves the pointer as raw. Although the ownership and fatness analyses are correctly applied to arrays, Crown does not generate the length metadata required to produce a safe Rust slice type. Array pointers are therefore kept as raw in Crown's output, even if they are known to be owning. Besides arrays, Crown also does not handle union pointers, function pointers, or pointers involving non-standard allocators.

Evaluated on 20 benchmarks, Crown achieves a median raw pointer declaration reduction of 37% and a raw pointer use reduction of 62%, reaching 100% on several data structure benchmarks such as avl, bst, buffer, and ht. It outperforms Laertes on the large majority of programs. This thesis extends Crown at exactly the gap it leaves: using the ownership and fatness information Crown's analysis already computes to rewrite arrays into safe Rust constructs.

== Complementary tools
Next to the ownership, aliasing and lifetime related issues in translating, partially solved by Laertes and Crown, there are more points of friction from unidiomatic translation outputs.

The use of output parameters is idiomatic in C, but not in Rust, which opts for using a more rich return type. For this, Hong et al. also introduced Nopcrat @hong_nopcrat, a tool that rewrites functions with output parameters into functions that return the result. By doing so, the resulting code becomes more easy to comprehend, less error prone and more idiomatic, making use of the `Result<T, E>` and `Option<T>` types. This is done through a static analysis that detects which parameters are used as output values.

Another unidiomatic translation artifact is the use of unions. Around 18% of unsafe functions involve C unions. Unions are often tied to a related tag to indicate wich type of data has been stored in the union, but updating this tag is error prone. Urcrat @hong_unions tries to solve this by rewriting unions to the Rust idiomatic tagged enums.

Forcrat @hong_forcrat is a tool to automatically replace the I/O API of libc with the Rust std API through static analysis.

#pagebreak()

= Array representation in C2Rust output <chapter_array_manifestation>
In order to rewrite C style arrays to a safe Rust alternative, we first need to understand how arrays manifest in C to Rust translated code, answering RQ1. Our first contribution is therefore an empirical analysis on the prevalence and characteristics of arrays in C2Rust output. C2Rust produces raw pointers as `*mut T` to represent C's heap arrays, which is a pointer to the address of the first element of the array. The same syntax is used for regular pointers that point to individual pieces of memory. Due to the ambiguity in representation, it is difficult to determine which pointers are arrays. We can reuse the Crown tool to figure this out.

== Reusing the Crown analysis
Crown determines which raw pointers are arrays, based on array-specific syntax, such as pointer arithmetic. It additionally calculates for every pointer whether it owns the data it points to, and its mutability. Together, Crown exports them as JSON files, organized under the name of each pointer and grouped by the function or struct it belongs to. We extended this analysis framework for our own use by exporting further information that Crown already computes internally but does not itself export, such as the ownership, mutability, and fatness attributes of function return types.

// TODO: Reference artifact

== Analysis framework

=== Named pointer sources
All information Crown exports is linked to the name of a pointer within a function or struct. Using such a name in a line of code yields the value that currently belongs to it, which gives rise to a definition we rely on throughout this chapter: a *named pointer source* is a place in the code that introduces a pointer value under a name, such that using that name elsewhere in the code retrieves that same value. We distinguish four kinds of named pointer sources, illustrated in @types_of_named_pointer_sources:
- function parameters, which become a source the moment the function is entered
- let bindings, which become a source the moment they are initialized
- struct fields, which are a source everywhere the containing struct's field is reachable
- function return values, which are a source at every call site of that function.

// TODO: Make sure this list of 4 things does not split and wrap to next page.

#figure(
  table(
    columns: 4,
    table.header([Struct field], [Parameter], [Let-binding], [Return value]),
    snippet("/thesis/examples/rust/src/bin/named_pointer_sources.rs", name: "struct_field"),
    snippet("/thesis/examples/rust/src/bin/named_pointer_sources.rs", name: "parameter"),
    snippet("/thesis/examples/rust/src/bin/named_pointer_sources.rs", name: "let_binding"),
    snippet("/thesis/examples/rust/src/bin/named_pointer_sources.rs", name: "return_value"),
  ),
  caption: [The four kinds of named pointer sources. In each snippet, `ptr` is the name under which the pointer value is introduced, and that name can then be used elsewhere to retrieve the same value.],
)<types_of_named_pointer_sources>

=== Named pointer interaction
For every named pointer source, we query all of its occurrences in the codebase using the Rust Analyzer. We call these occurrences _named pointer interactions_. Interactions come in two kinds, distinguished by their directness. A *direct interaction* is one where the name of the pointer source itself introduces the value into the code statement, for example `some_function(my_ptr_src);`. An *indirect interaction*, by contrast, is one where another pointer value is stored or moved into the named source rather than read from it. For example, a let binding such as `let my_ptr_src = ...;` uses a different value to initialize the source that is being interacted with.

=== Usage classification <section_usage_classification>
For each named pointer interaction, we classify how the value is used by examining its surrounding syntax. Not all surrounding syntax is informative, however. Parentheses, for example, neither change the underlying data nor depend on ownership, so we first need to find the *effective pointer*. This effective value and corresponding type will actually undergo the usage. This is done by walking up the syntax tree until we reach a node that is not one of the following: a parenthesis, a cast, or a flow-sensitive expression such as a tail expression or an if/if-else expression. The parent syntax node of the effective pointer is what we classify.

By collecting the parent syntax nodes of every effective pointer found across our twenty benchmarks, we distinguish 14 unique *effective usage types*:
- NullCheck (method call or binary expression)
- Deallocate (function call)
- Reallocate (function call)
- Assignment (assignment or initialization)
- ReferenceRawArray (reference)
- DereferenceElement (method call followed by dereference)
- ReferenceElement (method call)
- AddressOfPointer (method call)
- ComparePointer (binary expression)
- OffsetFrom (method call)
- PassToFunction (function call)
- Returned (return expression)
- Unused (no parent expression)
- Unclassified (not supported)

Some of these categories follow immediately from the parent syntax node alone, such as `Returned`. Others share the same kind of parent node and must be disambiguated further. For example, deallocating a pointer is itself just passing the source to the `free` function. Therefore, we explicitly treat `free` as an exception to distinguish `Deallocate` from the more general `PassToFunction`. These special cases are chosen purely for the convenience of the later rewriting process. `Unused` covers the remaining case where the effective pointer's parent is already the entire statement, which happens, for example, when a function that returns a pointer is called purely for its side effect and its return value is discarded. Finally, `Assignment` is a broad category covering any assignment or initialization of the classified value, such as storing the value into a variable, a static, a struct field, or a container such as ```rs let x = [.., my_ptr_value, ..];```. This covers both directions: the value of a named pointer source being assigned elsewhere, which is a direct interaction, and another value being assigned into the named pointer source, which is an indirect interaction.

The `Unclassified` category is used when syntax is found which we do not support in our classifier. This includes pointer-to-integer bit arithmetic, such as alignment checks, and calls through a function cast to a function-pointer type. These are uncommon, only occur in the brotli benchmark for non-owning arrays, and therefore do not affect the rewriter, which only handles owned arrays.

== Implementation
The entire analysis is implemented in Rust, built on top of the Rust Analyzer project rather than the compiler internals Crown itself depends on. We chose the Rust Analyzer for its architectural consistency with the rest of our tooling, its documentation, and the fact that it does not require a nightly Rust toolchain, which keeps the overall setup simpler. To collect all named pointer sources, we walk the syntax of each file and record every name for which Crown's analysis already holds information. Crown's analysis, however, does not cover the result types of C's FFI bindings, so we hardcoded named pointer source definitions for the relevant C functions ourselves. For example, `malloc` returns a raw owning pointer, and `free` takes one. To find the effective pointer of a given interaction, we implemented a recursive function that ascends the syntax tree until it reaches a node that is not a parenthesis, a cast, or a code block tail; the classification function then inspects the direct syntax parent of that effective pointer.

== Limitations
Neither Crown's analysis nor our own is complete. Several C idioms produce code that is inherently difficult to analyze statically, which means Crown's analysis can end up producing incorrect or no information for some pointers, and for much the same reason, our own analysis will not find every interaction of a given named pointer source.

@idiomatic_array_usage_in_c illustrates the idiomatic end of this spectrum: a pointer to a struct that itself contains an array is passed to a function, which assigns a new value to one of that array's elements. Here Crown correctly determines that the local variable `arr` represents the struct's `y` field, and, because `arr` is indexed, that `y` must therefore be an array. In a less idiomatic project such as @less_idiomatic_array_usage_in_c, the struct pointer is instead cast to a different type before the field is accessed through a hardcoded byte offset. This hardcoded offset is valid C on a given platform, since the struct's layout, padding included, is fixed by the platform's ABI, but the lack of the struct field access syntax makes analyses difficult. Crown can still recognize `arr` itself as an array from the indexing operation, but can no longer attribute that fatness back to the `y` field. Push the same idiom one step further, as in @even_less_idiomatic_array_usage_in_c, and even that link to `arr` disappears: once the function performs no array-specific operation on the pointer at all, Crown has no signal left to infer that it is an array.

Our own analysis inherits a similar shape of limitation. For a given named pointer source, the Rust Analyzer finds most of its named pointer interactions, but an interaction like the one in @even_less_idiomatic_array_usage_in_c is invisible to it for the same reason it is invisible to Crown. This is not limited to struct fields: it also affects function result types reached through a function pointer, since the Rust Analyzer cannot determine which concrete function a given function pointer refers to, particularly when it may point to different functions at different points during execution.

#figure(
  snippet("/thesis/examples/c/struct_field_idiomatic.c", name: "struct_field_idiomatic"),
  caption: [Accessing an array stored in a struct field through regular field access. Because `arr` is read from `my_struct->y` and then indexed, Crown infers both that `arr` is an array and that the field `y` is an array.],
  placement: auto,
) <idiomatic_array_usage_in_c>

#figure(
  snippet("/thesis/examples/c/struct_field_byte_offset.c", name: "struct_field_byte_offset"),
  caption: [The same access as in @idiomatic_array_usage_in_c, but through a hardcoded byte offset: `y` is read 8 bytes past the start of the struct (a 4-byte int followed by 4 bytes of padding on a 64-bit platform). Crown still infers that `arr` is an array from the indexing, but can no longer link it to the field `y`.],
  placement: auto,
) <less_idiomatic_array_usage_in_c>

#figure(
  snippet("/thesis/examples/c/struct_field_free_only.c", name: "struct_field_free_only"),
  caption: [The same byte-offset access as in @less_idiomatic_array_usage_in_c, but `arr` is only deallocated instead of indexed. With no array-specific operation and no recognizable link to the field `y`, Crown has nothing left to infer from that `arr` is an array.],
  placement: auto,
) <even_less_idiomatic_array_usage_in_c>

== Experimental setup
We evaluate our analysis on the same 20 benchmarks Crown itself uses: real-world C programs and libraries, already translated with C2Rust, ranging from mathematics libraries to a game binary, and so covering a reasonably varied set of C codebases. For each benchmark, we first run two of Crown's own scripts. The first script is a preprocessing script that deduplicates code, undoes some of C2Rust's more verbose desugaring, and makes a few pointer idioms explicit to ease further analysis. The second script examines every pointer in the project to produce its ownership, mutability, and fatness information. We then run our own analysis on top of this, writing its results into the same analysis-results folder Crown itself uses.

== Results

=== Ownership and array distribution

@named_pointer_source_distribution counts all named pointer sources across the benchmark suite. C's FFI functions are, by construction, defined as a source per file in each project, so for readability we collapse them into a single source per project in the table. A further overlap arises between direct and indirect named pointer interactions: an assignment such as `my_src_a = my_src_b` gives `my_src_a` an indirect interaction and `my_src_b` a direct one, but since both describe the same piece of code, we count it once, as a single occurrence of `Assignment`. The non-owned array column combines everything Crown marks as either not owned or transient.

The avl, bst, and quadtree benchmarks report no array pointers at all, which is unsurprising given that these are tree-structured data types operating exclusively on pointers to individual nodes rather than arrays. At the other extreme, tulipindicators, a library for computing financial indicators over time-series data, reports mostly array sources, since its entire data model consists of float arrays flowing through signal-processing functions. For that same reason, very few of those arrays are owned, since ownership lies with whichever code uses tulipindicators as a dependency rather than with the library itself. Brotli follows a similar pattern: as a compression codec, it takes input and output buffers from its caller, operates on them, and returns without ever allocating them itself. Both libraries are structurally transient, as they are designed not to own the data they touch. Benchmarks that are expected to manage their own storage show higher ownership rates instead: buffer maintains its own internal byte array, and ht owns its bucket array.

Beyond this difference in library design, C code routinely creates short-lived interior pointers through expressions such as `ptr + offset` or `buf->data + i` to scan or partition an existing allocation, which accounts for a significant share of the non-owned array sources. Crown's conservative analysis may also misclassify some genuinely owned arrays as non-owned, though this is unlikely to explain a large fraction of the gap.

#figure(
  result-table("pointer_sources", (
    ("benchmark", [benchmark]),
    ("total", [total]),
    ("array", [array]),
    ("owned_array", [owned array]),
    ("non_owned_array", [non-owned array]),
  )),
  caption: [
    Named pointer sources per benchmark, split by Crown's classification. _Array_ counts sources Crown marks as fat. _Owned array_ and _non-owned array_ split those by ownership, where non-owned includes both non-owning and transient pointers. C FFI declarations are counted once per benchmark. The 12 owned array sources that are output parameters are excluded from both ownership columns, as they are not rewritten (@section_scope_limitation).
  ],
)<named_pointer_source_distribution>

=== Usage patterns <section_usage_patterns>

@effective_usage_distribution shows how often each effective usage type occurs. The 114 owned array sources are used 933 times in total. Two of the three most frequent usage types among the owned arrays, `Assignment` and `PassToFunction`, are not array-specific at all and resemble general pointer usage. They also do not introduce unsafety, but simply transfer the existing safety contract to another location in the code. The `DereferenceElement` and `ReferenceElement` categories both target specific elements inside the array and together make up around 32% of all owned-array usages. Code classified this way relies on unchecked raw offsets from the base pointer, and is a significant source of unsafety in its own right. Major real-world vulnerabilities like Heartbleed @heartbleed_analysis were possible precisely because of a lack of bounds checking, as shown in @figure_heartbleed_snippet, making this a valuable target for rewriting. Among non-owned arrays, `DereferenceElement` is the single most common array-specific usage type.

Deallocation should, in principle, only ever occur on owned pointers, yet the analysis still records some non-owning arrays being deallocated. This can happen when Crown is unable to prove ownership, typically because the code violates the monotonicity rule Crown requires to mark a pointer as owned: Crown observes the deallocation but, unable to prove ownership, leaves the pointer classified as non-owning regardless. The same explanation applies to the handful of `Reallocate` usages on non-owned arrays.

The `Unused` category never occurs for owned arrays, which is expected given how few ways it can arise at all. The first possibility is a statement that consists of nothing but a bare value, such as `my_variable;`. This is a no-op, and therefore rare. The other possibility is a function call made purely for its side effects, whose returned pointer is never stored anywhere and is therefore classified as `Unused`. This is harmless for a non-owning pointer, but if the returned pointer were owning, discarding its result this way would leak memory, which is exactly why it is not expected to occur for owned arrays.

#figure(
  result-table("usage_types", (
    ("usage_type", [effective usage type]),
    ("owned_array", [owned array]),
    ("non_owned_array", [non-owned array]),
  )),
  caption: [Number of named pointer interactions per effective usage type, summed over all 20 benchmarks, for owned and non-owned array sources. An assignment between two named sources is counted only once. Interactions of the 12 owned array sources that are output parameters are excluded, as these sources are not rewritten (@section_scope_limitation).],
)<effective_usage_distribution>

== Summary
In answer to RQ1: C2Rust represents heap arrays as plain raw pointers, indistinguishable in type from pointers to single values. Using Crown's fatness analysis, 3013 of 7534 named pointer sources are arrays, but only 114 of those can be identified as owned. These owned arrays are used 933 times in total. Most of these usages, such as `Assignment` and `PassToFunction`, only move the pointer around, while 301 access elements through unchecked offsets (`DereferenceElement`, `ReferenceElement`), and 66 deallocate or reallocate it. Moreover, as discussed in the limitations, not every interaction is visible to the analysis, and some pointers are incorrectly marked as owned and vice-versa. Together with 69 interactions in which another value flows into an owned array, which are counted under that incoming value in @effective_usage_distribution, the rewriter must handle 1002 interactions in total (@chapter_evaluation).

These observations shape the remainder of this thesis. A safe replacement must take over deallocation, check element accesses, and remain usable by code the rewriter cannot identify. @chapter_safe_array_type turns these into the requirements for such a type.

#pagebreak()

= Safe array type <chapter_safe_array_type>

// Komt wel over, maar als je met malloc speelt, praat je over controle over alle allocate paden in het programma
// Mag meer expliciet, geen externe libraries assumed in "world"
// Zijn nog steeds assumptions
//
//

To rewrite the owned arrays found in @chapter_array_manifestation, we first need a safe type to rewrite them into. This chapter answers RQ2.

Such a type must satisfy at least two requirements. It should have:
1. *Automatic deallocation.*
  Whereas stack memory can be "leaked" without consequence, heap allocations must be deallocated. Safe rust types that own allocations automatically do this once their lifetime ends, such as the `Box` type. An additional requirement here is that the deallocating is done with the same allocator that was used to obtain the pointer. Safe types in Rust use the Rust Global Allocator. In contrast, translated C code still makes use of the C allocator, through FFI bindings to `malloc` and others. Mixing these is undefined behavior, meaning a pointer obtained with C's `malloc` cannot be deallocated with Rust's dealloc function, or vice-versa. The safe array type must therefore not only ensure the allocation is automatically deallocated, but also ensure it is done with the same allocator as the one used to allocate the memory in the first place.
2. *Representation equivalence*
  Given the earlier example in @even_less_idiomatic_array_usage_in_c, and the difficulty of detecting struct-field usage in code like it, code that the rewriter cannot see must keep working on our new type as if it were still the original raw pointer. This requires the new type to have not only the same size and alignment as the original raw pointer, but every value it holds must also be bit-for-bit the pointer value the original program would hold: the address of the first element for a live array, and all zeroes for null. Equivalently, reinterpreting (transmuting) the new type as the raw pointer type yields exactly the original pointer.

These two requirements also hold for safe regular pointers. The primary use of arrays however is to store and retrieve elements inside of it. This is done with indexing. Accessing an element that does not lie within the bounds of the array is undefined behavior. Therefore, an additional third requirement for safe arrays is needed:

3. *Bounds checking*
  Because unchecked indexing allows for undefined behavior, runtime checks should ensure the accessed element exists and lies within the bounds of the array.

// #figure(
// 	[Rust divides types in either the Sized or Unsized category, indicating whether the total size of required memory of the type is known at compile time. A struct with only Sized fields is Sized, whereas a slice such as `&[i8]` is Unsized, as it is not yet known how many `i8` elements exist in the slice. Unsized types are required to be accompanied with an additional size value on the stack: &[i8] therefore consists of a pointer and a size. `Box<i8>` consists of only the pointer to the i8. `Box<[i8]>` is Sized, but consists of the pointer to the first element, and has the length of the slice on the stack.],
// 	caption: "Rust Sizedness"
// )// TODO: Nice figure explaining Sizedness?

// TODO: Should I include definition of unsized types in Rust?

== Existing options
While many types exist that solve some of these requirements, no existing type covers all three simultaneously. In this section, we go over the upsides and downsides of each option, and why they are unsuitable for arrays. In the examples, `T` represents the type of the element that is contained inside the array.
- *`Box<[T]>`*
  Reusing the `Box` results in automatic deallocation, as well as bounds checking when elements are accessed. Unfortunately, the slice type `[T]` is Unsized. This means that the `Box` must also store the length of the slice next to its pointer. This fails our representation equivalence requirement.
- *`Box<Box<[T]>>`*
  While the slice is Unsized, a `Box<[T]>` itself is in fact Sized. This means that creating indirection through another `Box` solves both the automatic deallocation and the bounds checking. Unfortunately, although the resulting stored size and layout of the outer box is exactly the same as a raw pointer, the actual stored address does not represent the first element of the array.
- *`ThinBox<[T]>`*
  A `ThinBox` is similar to a `Box`, except that instead of storing an Unsized type's size next to its pointer, it stores that size on the heap, in a header directly before the data itself. The stored pointer points at the first element, with the length at a negative offset, so its representation is equivalent to a raw pointer, and it provides bounds checking. Its limitations lie elsewhere. A `ThinBox` can only be created from a value, as there is no way to construct it from a raw pointer or convert it back into one. This makes it impossible to take over an allocation obtained from `malloc`, or to pass ownership back to code that calls `free` or `realloc`. Additionally, `ThinBox` is still an unstable feature, only available on nightly versions of Rust.
- *`Box<T>`*
  Crown's rewrite tactic is to use the already existing `Box<T>`. To reuse the same approach, we could make the `Box` point to only the first element of the array. And each time we need to access one of the other elements in the array, we convert the box to a raw pointer first, and continue with its usage. This exactly solves the representation equivalence and automatic deallocation. This approach still misses the bounds checking.
- *`Box<T>` with prefixed header*
  Instead, we could try to, similar to the `ThinBox`, store the length of the slice in the allocated memory, before the address of the pointer. If we can ensure each allocation made in the program "reserves" a few bytes before the obtained pointer, we can use this prefixed header to calculate whether the indexed element actually lies inside the allocation. Since `Box` always deallocates through Rust's Global Allocator, this requires overriding the Global Allocator itself to reserve the header, which imposes the header on every allocation in the program, including those made internally by Rust's standard library. In theory, this solves our three requirements.
  Unfortunately, this is still not an ideal solution due to pointer provenance. Besides an address, a pointer carries provenance: which memory it is allowed to access. A `Box<T>` claims unique access to exactly `size_of::<T>()` bytes, and any pointer derived from it inherits that narrowed range. Reading the header before it, or any element after the first, therefore accesses memory outside of what the `Box` may access, even though that memory still lies inside the allocation. Pointer provenance is still actively being developed and may change over time, but for now it means we cannot guarantee this solution will work.
// TODO: cite Stacked Borrows (Jung et al., POPL 2020) and Tree Borrows

#let yes = text(fill: rgb("#1a7f37"), weight: "bold")[✓]
#let no = text(fill: rgb("#cf222e"), weight: "bold")[✗]
#let maybe = text(fill: rgb("#bf8700"), weight: "bold")[?]

#figure(
  table(
    align: (left, center, center, center, center),
    columns: (1.2fr, 1fr, 1fr, 1fr, 1.3fr),
    [Candidate], [Auto-dealloc.], [Repr. equiv.], [Bounds check], [Adopts raw alloc.],
    [`*mut T`], no, yes, no, yes,
    [`Box<T>`], yes, yes, no, yes,
    [`Box<T>` + header], yes, yes, maybe, yes,
    [`Box<[T]>`], yes, no, yes, yes,
    [`Box<Box<[T]>>`], yes, no, yes, yes,
    [`ThinBox<[T]>`], yes, yes, yes, no,
    [`CBox<[T]>`], yes, yes, yes, yes,
  ),
  caption: [Each considered type for arrays in Rust. #yes marks a satisfied property, #no an unsatisfied one. The last column indicates whether the type can take ownership of an existing raw allocation, and give it back. The #maybe marks bounds checking that relies on reading memory outside of the `Box`'s provenance, which is only sound under some aliasing models.],
)

== The new CBox type
We therefore introduce a new type, which we call `CBox`. It follows the same design as `ThinBox`, storing the length in a header directly before the first element, but addresses its shortcomings: it can take ownership of raw allocations and give them back, supports lengths only known at runtime, and places its header at a fixed offset regardless of the element type. Because `CBox` stores a raw pointer rather than a `Box`, it does not narrow the pointer's provenance, avoiding the problem of the prefixed-header `Box`. This new type solves all three of our requirements.

=== Representation

Rust's `NonNull<..>` is a pointer type guaranteed never to be null. To still allow for null pointers in our own type, we wrap it in `Option<NonNull<..>>`. Wrapping a type in an `Option` would normally require extra memory to record which variant, `Some` or `None` in this case, is active. Because `NonNull` is guaranteed to never be bitwise all zeroes, Rust can use that all-zero bit pattern itself to represent the `None` variant. This null-pointer optimization (NPO) is what lets `Option<NonNull<..>>` remain exactly the same size as a raw pointer. The advantage of this pattern over a simple internal raw pointer is that it becomes syntactically more clear when a pointer is null or actually contains a valid value.

This `Option<NonNull<..>>` is then wrapped in a parameterized struct so that we can separate the underlying type from the parameter type, as seen in @cbox_struct_definition. If we defined the internal pointer as `Option<NonNull<T>>`, with `T` as the parameterized type of CBox itself, then the internal pointer would still contain a size value if `T` is Unsized, for example with `CBox<[f32]>`, resulting in `Option<NonNull<[f32]>>`. That defeats our purpose of creating a type that is guaranteed to be the same size as a raw pointer. Instead, we hardcode the internal pointer type with `u8`, representing a single byte. Since the internal pointer is only stored, and cast to the right element type whenever it is used, this choice does not restrict which types a `CBox` can point to.

The `?Sized` parameter specification indicates that both Sized and Unsized types are allowed in the CBox, such as `f32` or `[f32]` respectively.

To ensure the wrapping in the struct does not influence the size, we can mark it `transparent`, which tells the Rust compiler that the struct should not take up more space in memory than the internal data.

The `_marker` field is zero-sized and does not affect layout. It gives the unused type parameter `T` a use, which is required by the compiler, and through `*mut T` makes `CBox` invariant in `T` and neither `Send` nor `Sync`, matching the raw pointer it replaces.

Because every level is a thin pointer, representation equivalence would also holds for nested arrays. A C array of strings, `char **` can correctly be represented by `CBox<[CBox<[u8]>]>`: both the outer pointer and each of its elements take up exactly as much space as their raw counterparts, so indexing the outer array reaches the same addresses as before. With `Box<[Box<[u8]>]>`, each element would be a fat pointer twice the size of a raw pointer, so every element after the first would lie at a different offset than raw code expects. Note however that in this work, we exclude nested arrays from our scope, as explained further in @section_scope_limitation.

#figure(
  snippet("/tools/rewriter/runtime/src/cbox.rs", name: "cbox_struct"),
  caption: "The struct definition of CBox. The internal pointer is stored as a pointer to bytes rather than T, to ensure 'ptr' remains thin",
)<cbox_struct_definition>

=== Allocation header <padding_allocations_with_header>
Allocation functions now need to reserve and initialize an extra space of memory before each created allocation. For this, we defined the following functions, each mimicking the original C allocation functions. For a `CBox` to be able to take ownership of any allocation, every allocation in the program must go through these functions. How the rewriter manages this is described in @rerouting_allocation_calls.

The header stores the size of the allocation in bytes as a `usize`, a type large enough to represent the size of any allocation. The header does not take up only the 8 bytes of a `usize`, however, but the maximum alignment any primitive type requires on the platform, `MAX_ALIGN`, which is likely 16 bytes. This ensures that the data following the header remains correctly aligned for any element type, and that the header is always at the same fixed offset before the data pointer, regardless of the type the pointer is viewed as.

*`header_malloc`*, shown in @malloc_implementation, mimics the C's `malloc` function by taking a specified size in amount of requested bytes, and returning a void pointer to an address that has the requested amount of bytes left that can be used. The `layout_with_header` is a utility function that creates a Rust `Layout` type that describes the requested bytes in addition to the header, while ensuring it is properly aligned. The new layout is then allocated with `global_alloc_zeroed` from the Rust Global allocator. C's `malloc` does not zero its memory. In C, reading uninitialized memory yields an indeterminate value, but Rust takes this one step further. A reference must point to a valid value, even if it is never read, and an uninitialized integer is not a valid value. Since the `get` method of our `CBox` hands out references to elements, we therefore take the precaution to already zero the newly allocated memory. Every type that C2Rust emits, such as integers, floats, raw pointers and optional function pointers, has a valid representation of all zeroes, and so do Crown's `Option<Box<T>>` and a null `CBox`, meaning our new allocation always holds valid elements if interpreted as an array. This will not change the observable behavior, as a correct C program never reads uninitialized memory. After a quick check for null, the allocation size is stored, and the data pointer is calculated and returned.

#figure(
  snippet("/tools/rewriter/runtime/src/allocation.rs", name: "header_malloc"),
  caption: "The 'malloc' implementation that uses an allocation layout that leaves space for the allocation header. The pointer to the first data byte is returned, allowing future code to access the `size` value at the negative offset. Here, `global_alloc_zeroed` allocates zeroed memory through the Global Allocator in Rust. The function is generic over the integer type of `size`, so it accepts whichever integer type the translated code passes.",
)<malloc_implementation>

Similar to `header_malloc`, `header_free` in @free_implementation takes any pointer that originates from a prefixed allocation pointer, and deallocates it using `global_dealloc` from the Rust Global allocator. For this, we need to reconstruct the original `Layout` that was used to create the allocation, so we fetch the size stored in the header with `get_header`, which returns a pointer to the header by subtracting `MAX_ALIGN` bytes from the given data pointer. We then create the same layout as in the `header_malloc`, and proceed to deallocate the result.

#figure(
  snippet("/tools/rewriter/runtime/src/allocation.rs", name: "header_free"),
  caption: "The 'free' implementation that subtracts the header size in order to get the allocation's base pointer, which in turn can be used to safely deallocate it with Rust's Global Allocator. This code is simplified.",
)<free_implementation>

Next to `header_malloc` and `header_free`, we also implement four other functions to ensure each allocation contains this header: `calloc`, `realloc`, `strdup` and `strndup`. These are implemented in a similar fashion as `malloc` and `free`, but are left out for brevity. Realloc however needs to manually zero the new otherwise undefined bytes, for the same reason we also zero newly made allocations in `header_malloc`.

All of these functions follow C99, which defines `malloc`, `calloc`, `realloc` and `free`, and C23 for `strdup` and `strndup`, which C99 does not define. Where these standards leave a choice to the implementation, we make the following choices. Every allocation that cannot be made returns a null pointer, including requests that are too large to describe as a Rust `Layout`, and negative sizes, which C converts to a very large `size_t`. A failing `realloc` leaves the original allocation intact. `malloc(0)` returns a unique pointer that can be freed, as the header makes every allocation non-empty. `realloc` with a size of zero frees the allocation and returns a null pointer. As neither standard requires it, the functions make no guarantee about the value of `errno`. Finally, as described above, every allocation is zeroed, where C leaves its contents indeterminate.

=== Ownership and deallocation
In contrast to `Box`, a `CBox` only owns its own allocation. It models a `malloc`'d block of memory, and only releases its allocation just like `free` would. This means the elements inside the `CBox` are not dropped automatically when the `CBox` itself is. If we were to make CBox drop its elements as well, just like Rust's Box does, we would have to guarantee each element uniquely owns its data. We cannot make this guarantee, because Crown's analysis assumes all elements inside an array to have the same ownership type. If the first element contained an owning pointer, and the second element a copied pointer to the same allocation, the second would incorrectly be considered owning as well. This means we cannot automatically deallocate the elements inside a CBox.
The automatic deallocation trait called `Drop` is implemented on the struct as shown in @cbox_drop_implementation. It takes the internally stored pointer, and directly calls our custom `header_free` function on it.

#figure(
  snippet("/tools/rewriter/runtime/src/cbox.rs", name: "drop"),
  caption: "The `Drop` implementation of CBox, where only the CBox's own allocation is freed.",
) <cbox_drop_implementation>

=== Bounds checking <section_bounds_checking>
For our third requirement of the safe array type, the index methods with bounds checking are implemented on `CBox<[T]>` types. Two types of index methods are implemented, with varying levels of strictness. The most lenient is the `get_ptr` method, which calculates a pointer at the given index offset. In both C and Rust, to prevent undefined behavior, the resulting pointer after the offset must lie inside the allocation, or be one address past the allocation. This means the returned pointer may be an invalid pointer, and cannot be dereferenced. This is a common C idiom, where a pointer is iterated over an array, and only after incrementing the pointer until it is outside of the allocation, the check to see if it still lies within the allocation is made, and the dereferencing is omitted. To support the bounds checking, the header before the internal CBox pointer is consulted, and the maximum index is calculated that could be requested. If the requested index turns out to be past the maximum index, or the `CBox` is null, `None` is returned instead of a pointer.


#figure(
  snippet("/tools/rewriter/runtime/src/cbox.rs", name: "get_ptr"),
  caption: "The get_ptr method for CBox<[T]> types.",
)

The second, more strict indexing method is the `get` method which is more Rust idiomatic. This returns a safe reference to one of the elements of the array, similar to what `Box<[T]>` implements. In addition to the requirements of the `get_ptr` method, the index can no longer lie one past the end of the allocation, and must point to a valid memory representation of the element type. Its mutable counterpart, `get_mut`, returns a mutable reference instead. While a reference returned by `get` is alive, no raw pointer may modify the same element, and while a reference returned by `get_mut` is alive, no raw pointer may access it at all. Our rewriter only uses the reference within the statement that creates it, and C2Rust output contains no other references to these elements, so this requirement is always met.

#figure(
  snippet("/tools/rewriter/runtime/src/cbox.rs", name: "get"),
  caption: "The get indexing method for CBox<[T]> types. ",
)

Both methods return an `Option`, leaving the decision of how to handle an out-of-bounds access to the caller.

=== Scope limitations <section_scope_limitation>
The `CBox` approach only works on base allocation pointers. This excludes pointers to stack arrays as well as pointers to non-first elements inside of heap arrays. Crown marks pointers "owned" when its value flows from allocation functions or into deallocation functions. Otherwise, the "owned" value can propagate through code such as assignments or function parameters. Pointers obtained in other ways, like taking the address of a stack array, are therefore explicitly constrained to be non-owning. Owning pointers therefore always hold a value that points to the base of an allocation. This allows us to consider all owning pointers to be suitable for our CBox retyping. Due to the unsoundness of Crown's analysis, this is an assumption rather than a guarantee, which we discuss further in @chapter_limitations

While Crown does rewrite non-array output parameters, that is no longer possible for the CBox approach. Output parameters do not have the ownership guarantee anymore from Crowns analysis, meaning that stack array pointers may be passed to output parameters which do not have an allocation header. For this reason, output parameters are left outside of our scope.

Furthermore, this work does not support nested arrays. The Crown analysis cannot distinguish ownership of individual elements of the same array. In addition, indexing such as `my_arr[3]` gets translated to `*my_arr.offset(3)`, which is considered a borrow of the `my_arr` pointer in Crowns analysis, rather than an access. This also means that elements of owned arrays are structurally marked transient, rather than owned. This is visible in the benchmarks, as none of the owned arrays have owned array elements. The fallback of this limitation in scope is safe, as now any internal array remains a raw pointer, and will still be deallocated with a call to `free`. This is why our rewriter explicitly ignores nested arrays.

== Summary
In answer to RQ2: no existing Rust type satisfies all three requirements of automatic deallocation, representation equivalence and bounds checking. We therefore introduced, `CBox<[T]>`, which holds exactly the same bits as the raw pointer it replaces, including null. Its length is stored in a header at a fixed offset before the first element. When a `CBox` is dropped, it automatically frees its own allocation, though not the elements inside it. Its `get` and `get_ptr` methods use the header to perform bounds checking.

// TODO: Explicitly define 'base pointer', and consistenly use later on?

The replacing of pointers with a `CBox` is safe under two conditions. First, every pointer that is used to construct a CBox must be pointing directly behind a header. Second, the pointer must own its allocation. Only truly owning pointers are guaranteed to fit both these requirements, which @chapter_correctness states as assumption A1.

#pagebreak()

= Rewrite methodology <chapter_rewrite_methodology>

With the safe `CBox` type in place, this chapter answers RQ3: How can C2Rust output be rewritten automatically to use this type? The rewriting process contains multiple independent steps. The first step is to reroute each call to the C FFI allocation methods to our own implementations with a header, which is explained in @rerouting_allocation_calls. After that is done, all owned arrays are retyped as `CBox<[T]>`, with `T` being the type of the elements in the array, seen in @owned_array_retyping. Then, all pointer interactions that reference these rewritten arrays must be rewritten such that the interaction itself matches the same type as the surrounding code in @context_rewriter. Finally, in addition to the context switching, some special cases are modified to improve the safety of the usage itself rather than just the typing, which is discussed in @targeted_usage_rewriting.

== Rerouting allocation calls <rerouting_allocation_calls>
The first requirement for replacing a pointer with `CBox` is that it must be pointing one address after the allocations header. Because we do not verify statically or during runtime which pointers were allocated with a header, we instead simply ensure that every allocation made is done with our custom allocation functions. Rather than finding every call site individually, we find each file's declarations of these C allocator functions, and swap them for our allocator functions aliased to the same name, shown in @figure_allocation_call_rerouting. This way, even function pointers to C's allocation functions, which are untraceable with the Rust Analyzer, end up creating allocations with the prefixed header. An additional benefit of this approach is that this itself is also a step towards migration away from FFI allocation calls.

#figure(
  table(
    columns: (auto, 1fr),
    [*input*], [*output*],
    snippet("/thesis/examples/rewriter/allocation_rerouting/src/main.rs"),
    snippet("/thesis/examples/generated/allocation_rerouting.rs", name: "output"),
  ),
  caption: "Example of allocation call rerouting: a small input project, before and after running Crown and our rewriter on it.",
) <figure_allocation_call_rerouting>

// TODO: Talk about the fact CBox provides spatial memory safety (in or outside of allocation), and not type safety: Can easily cast to different types

== Owned array retyping <owned_array_retyping>
Each owned array pointer's type is rewritten to have the type of the `CBox<[T]>`, with `T` being the type of the elements inside the array. The elements type is parsed syntactically from the pointer type, either by directly unwrapping the pointer, or by asking the Rust Analyzer to infer the actual type in case a type alias is used. The new array type is only rewritten if the original type is explicitly mentioned. For example, in let-bindings, the type of the new variable may be omitted if it can be inferred from the initializer value. In that case, we leave the type omitted as well.

== Context rewriting <context_rewriter>
Once arrays have been retyped, their interactions need to be modified as well. If the type of the pointer expression changed, the surrounding code might expect a different type, as seen in @example_of_rewritten_allocation. To solve this, we adopt Crown's context-based rewriting, and adjust it slightly.

#figure(
  table(
    columns: (1fr, 1fr),
    [*Before*], [*After*],
    ```rs
    // Type error: Cannot implicitly cast raw pointer from malloc to a CBox
    let new_arr: CBox<[u8]> = malloc(20);
    ```,
    ```rs
    let new_arr: CBox<[u8]> = CBox::from_raw(malloc(20));
    ```,
  ),
  caption: "Code snippets of before applying context-switching rewrites and after. ",
) <example_of_rewritten_allocation>

Each pointer expression with a modified type has an inner context, which represents the type of the pointer value itself, and an outer context, which represents the type the surrounding code expects. In this work we only focus on the `Box` and the `Raw` context, but in contrast to Crown already distinguish between owning `Raw` and non-owning `Raw`. With these contexts, we can define a transition table, as shown in @context_transition_table.

#figure(
  table(
    columns: (1fr, 1fr, 1fr, 1fr),
    [*inner context*], table.cell([*outer context*], colspan: 3),
    [], [*Box*], [*Raw(owned)*], [*Raw(non-owned)*],
    [*Box*], [`p.take()`], [`p.into_raw()`], [`p.as_ptr()`],
    [*Raw(owned)*], [`CBox::from_raw(p)`], [`p`], [`p`],
    [*Raw(non-owned)*], [-], [-], [`p`],
  ),
  caption: [Rewrite applied to a pointer value `p` with the inner context (rows) when used where the outer context (columns) is expected. '`p`' means no rewrite is needed, and '`-`' marks transitions that cannot occur, as values flowing into an owned array are always treated as owning.],
) <context_transition_table>

In @example_of_rewritten_allocation, where `new_arr` is rewritten to have a `CBox` type, the variable is initialized with a new allocation obtained from malloc. In this example, the pointer expression is the call to malloc: `malloc(20)`, which returns a raw pointer. This cannot be implicitly turned into a `CBox`. The inner context is therefore `Raw(owned)`, whereas the outer context is a `CBox`. The solution here is to use the `from_raw` method, which takes ownership of a raw pointer, and builds a new `CBox` from it.

The transition from a non-owning raw pointer to a `CBox` should not exist, because under Crown's ownership rules, ownership can only be transferred from an owning pointer, and a non-owning value can never flow into an owning one. Crown's results are however not always consistent, and we do encounter non-owning values being assigned to owned arrays. In that case, we trust the ownership of the receiving array: since it will eventually be deallocated, the value it receives must be the base of an allocation, and the non-owning label of the incoming value is the inaccurate one. We therefore treat every value that flows into an owned array as owning, so a non-owning raw pointer never needs to be converted into either a `CBox` or an owning raw pointer.

The advantage of already distinguishing between owning and non-owning raw contexts allows us to define two different rewrite rules for the conversion of Box contexts to raw pointers. When a temporary raw pointer is needed, which may happen when a Boxed array is passed to a function that needs to read from an array, we only copy the raw internal pointer of the CBox. This keeps the CBox in charge of the lifetime of the allocation. When a function expects an owning raw pointer, the `into_raw()` is used instead, such that the raw pointer now holds the ownership of the allocation. To prevent CBox from still deallocating the pointer when the box goes out of scope, the internal pointer is replaced with the `None` option. Their implementation can be found in @pointer_getting_implementations.

#figure(
  snippet("/tools/rewriter/runtime/src/cbox.rs", name: "as_ptr_into_raw"),
  caption: "The method implementations of as_ptr, which returns a non-owning raw pointer, and into_raw, which returns the owning pointer and leaves None behind",
) <pointer_getting_implementations>

Copying the internal pointer with `as_ptr()` is only correct as long as the CBox outlives the raw pointer. Due to the ownership results we obtain from Crown, this does not always hold. For example, in @figure_borrowed_function_result, the result of `strdup` is marked as owning, so `strdup` is rewritten to return a `CBox`. The variable `str` that receives this result is however marked as non-owning, even though it is freed later on. According to @context_transition_table, `.as_ptr()` should be used here, even though the obtained `CBox` is only a temporary value and will be dropped after the assignment, which leaves `str` with a dangling pointer. The same occurs when an owning variable is returned from a function whose return value is marked as non-owning. The variable is deallocated when the function exits, so only a dangling pointer is returned. In both cases, the `CBox` is not used anymore after the conversion, so we can safely convert them to raw with `.into_raw()` instead, leaving the `None` variant behind. This is hardcoded into our rewriter for these exceptions where we know the `CBox` is not used after the conversion. We report how often this exception applies in @chapter_evaluation.

Crown's own rewriter converts a Box into a non-owning raw pointer in the same way, without such an exception. It does not encounter this problem, because every inconsistent result we encountered concerns an array pointer, which Crown analyses but never rewrites. Our rewriter is therefore the first to depend on Crown's ownership results for array pointers, and so the first to expose their inconsistencies.

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(align: center, [*C code*]),
    table.cell(align: center, [*Naively rewritten with `as_ptr()`*]),
    ```c
    bool url_is_ssh(
    	char *str
    ) {
    	str = strdup(str);
    	if (strcmp(str, "ssh") == 0
    		|| strcmp(str, "git") == 0)
    	{
    		free(str);
    		return true;
    	}
    	return false;
    }
    ```,
    ```rs
    fn url_is_ssh(
    	mut str: *mut c_char
    ) -> bool {
    	str = (strdup(str)).as_ptr();
    	if strcmp(str, "ssh") == 0
    		|| strcmp(str, "git") == 0
    	{
    		free(str as *mut c_void);
    		return true;
    	}
    	return false;
    }
    ```,
  ),
  caption: [Checking whether a URL's protocol is SSH in the urlparser benchmark. Crown marks the result of `strdup` as owning, so `strdup` now returns a `CBox`, but marks `str` as non-owning, so the rewriter only borrows its pointer with `as_ptr()`. The temporary `CBox` is however dropped at the end of the statement, which deallocates the copy immediately: both `strcmp` calls read freed memory, and `free` deallocates it a second time. The code is simplified for brevity.],
) <figure_borrowed_function_result>

This work explicitly leaves out Crown's `Mut` and `Const` pointer kinds, which Crown rewrites to the safe references `Option<&mut T>` and `Option<&T>`. `Const` is used for non-owning pointers that are only read from. `Mut` is used for output parameters, which are function parameters that Crown's ownership analysis marks as owning, but that only give the function temporary and unique access to data owned by the caller. Output parameters are a common C idiom, where the output of a function is stored inside one of its parameters. Although we could reuse the same concept to rewrite safe output parameters, for example with the `Option<&mut [T]>` type, we cannot guarantee that retyping is valid. Because Crown cannot guarantee output parameters are owning because of allocation related usage, there is a chance that pointers of stack arrays are passed to these parameters. And because stack arrays are not created using allocation functions, they do not contain the header with the size of the array. This could become possible by applying the same strategy as in @rerouting_allocation_calls to stack arrays, rerouting each stack array creation such that it also contains a header before the resulting pointer. However, this approach is not suitable for arrays in struct fields, as there it is impossible to increase the size the array takes up. Therefore, we leave the idea of rewriting output parameters outside of our scope, and focus on rewrites whose correctness we can argue in @chapter_correctness.

In addition to rewriting interactions to make the context of a pointer expression match the outer context, the applied pointer casts also need to be taken into account. Unlike raw pointers, the CBox type cannot be casted with the `as` keyword. That is why we need an additional condition in our rewriter, which is based on the type of context switch that happens. If our effective pointer is of the Box context, the casts inside the effective pointer are no longer possible. If the target context is a Box as well, we can make use of the `.cast()` method, which only changes the parameterized type of our CBox while leaving the internal data exactly the same. On the other hand, if the target context is a raw pointer, we can simply move the cast syntax after the context-switch code, such that the casts are applied after the CBox has turned into a raw pointer. When the effective pointer contains casts but has one of the Raw contexts, we just keep the casts inside the rewritten part, to ensure the correctly typed raw pointer is either turned into a CBox or raw pointer.

#figure(
  table(
    columns: (auto, 1fr),
    align: (left + horizon, left),
    table.cell(colspan: 2)[
      *Original:* ```rs ptr as *const i32```, where `ptr` points to an array of `u32`
    ],
    [*Context switch*], [*Rewritten with cast*],
    [Raw to Box],
    ```rs
    CBox::from_raw(ptr as *const i32)
    ```,
    [Box to Raw],
    ```rs
    ptr.into_raw() as *const i32
    ```,
    [Box to Box],
    ```rs
    ptr.cast::<[i32]>()
    ```,
  ),
  caption: [Rewriting the same pointer cast for each context switch. A cast on a raw pointer stays inside or after the conversion, whereas a cast between two `CBox`es becomes a call to `cast`, as Rust's `as` cannot cast between `CBox` types.],
) <cast_rewrites>

== Targeted usage rewriting <targeted_usage_rewriting>
The previous steps ensure the safe type is correctly used, which results in improved overall safety with regards to allocation and deallocation. However, using a CBox can provide more benefits than just turning it into a raw pointer. Therefore, for specific categories of the usage classification in @section_usage_classification, we define additional rewrite rules.

=== Deallocation
Because a CBox deallocates its allocation once it goes out of scope, it may seem sufficient to replace each free call with `()`, in the same way Crown does. This relies on every CBox eventually being dropped by Rust. That does not hold for a CBox stored in a struct field, an example of which is shown in @figure_struct_field_deallocation. Here, the array field of the struct, and then the struct itself are freed with a raw free call. Replacing the first `free` call with `()` would leak its memory.

#figure(
  table(
    columns: (1fr, 1fr),
    align: (left),
    table.cell(align: center, [*C code*]),
    // table.cell(align: center, [*Rewritten with `()`*]),
    table.cell(align: center, [*Translated + rewritten*]),
    ```c
    struct heman_image_s {
    	HEMAN_FLOAT* data;
    	...
    };

    void heman_image_destroy(
    	heman_image* img
    ) {
    	free(img->data);
    	free(img);
    }
    ```,
    ```rs
    struct heman_image_s {
    	data: CBox<[c_float]>,
    	...
    }

    fn heman_image_destroy(
    	img: *mut heman_image
    ) {
    	drop(((*img).data).take());
    	free(img as *mut c_void);
    }
    ```,
  ),
  caption: [Deallocating an image in the heman benchmark. The pixel array `data` is rewritten to a CBox, while the image struct itself is not an array and remains a raw pointer. Replacing the first `free` with `()` would leak the pixel array, as freeing the `img` pointer only deallocates the memory of the struct itself. With the `.take()`, we first extract the ownership out of the struct, such that it can correctly be deallocated. The code is simplified for brevity.],
) <figure_struct_field_deallocation>

We therefore rewrite every deallocation of a CBox to `drop(my_cbox.take())`. The `take()` method moves the allocation out of the CBox and leaves a null CBox in its place, after which `drop` immediately deallocates it. This deallocates at exactly the same moment as the original program would have, as argued in @section_correctness_argument. Unlike `drop(my_cbox)`, which would move out of the variable and disallow any further use of it, this works on every kind of place, including struct fields behind raw pointers. Unlike a bare `my_cbox.take()`, the rewrite has type `()`, just like the original free call, so it remains valid where the call is used as an expression.

The remaining difference with the original program is that the pointer is null after the deallocation, rather than dangling. It may seem this changes program behavior, as testing against the deallocated pointer without dereferencing it would now instead be testing against a nullpointer. However, according to the C99 standard, simply using the value of a pointer after it has been deallocated is undefined behavior. The rewrite would therefore only alter behavior of programs that already contain undefined behavior, on which our claim makes no statement (@section_correctness_claim).

=== Reallocation
Reallocating a CBox must be done through the custom `CBox::from_realloc` method. This is due to the mutable borrow nature of `realloc`, as the original pointer should remain valid and usable if reallocation fails, or should be considered invalid and unusable if reallocation has succeeded. This split makes it impossible to pass a raw pointer obtained from a CBox to the realloc function, as `into_raw()` by default leaves behind a nullpointer in the CBox even if realloc fails, and `.as_ptr()` does not take the ownership even if realloc succeeds. The `from_realloc` method of CBox therefore is implemented as seen in @realloc_implementation.

#figure(
  snippet("/tools/rewriter/runtime/src/cbox.rs", name: "from_realloc", elide-hidden: false),
  caption: "Implementation of the CBox from_realloc method. Takes a mutable `self`, such that the original pointer can be set to null in case the reallocation succeeds, or in case reallocating to zero bytes freed the allocation.",
) <realloc_implementation>

=== Indexing
Syntax that fetches (a pointer to) an element inside the array can also benefit more from the CBox type. Instead of calculating a raw offset of a raw pointer without bounds checking, the safe alternative of `.get` and `.get_ptr` can be used. These two functions check if the requested element or pointer actually lie within the allocation, by consulting the length stored in the allocation header. Both return an `Option`, so `.unwrap()` is called each time they are used in a rewrite. This immediately causes a panic if undefined behavior would have occurred in the original code.

#figure(
  snippet("/thesis/examples/rust/src/bin/indexing.rs", name: "raw_offset"),
  caption: "Two cases where the .offset method is called. The first only gets a pointer to the sixth element in the array, whereas the second actually dereferences it.",
)
#figure(
  snippet("/thesis/examples/rust/src/bin/indexing.rs", name: "cbox_get"),
  caption: "Two snippets showing the safe alternative of indexing. The first line gets a raw pointer to the 6th element, and the second line gets a safe reference. Both immediately unwrap the result, so that the code will panic if the element lies out of bounds for example.",
)

/*
#figure(
	canvas({
		import draw: *

		// ---------- tunable constants: change these to retune the whole figure ----------
		let field-w = 1     // width of a ptr/len cell
		let field-h = 0.5   // height of a ptr/len cell
		let data-w = 3       // width of a "data: [T]" box
		let data-h = 1        // height of a "data: [T]" box
		let stack-x = 0         // x of stack-side cells
		let inner-x = 3           // x of the heap-allocated inner fat pointer
		let data-x = 6              // x of the final "data" box
		let title-gap = 0.5           // space between a cell's top and its title above it
		let label-gap = 0.3             // space between a row's bottom and its byte-count label
		let row-gap = 0.6                 // space between one row's label and the next row's title

		// reusable pieces: `pos` is always a cell's bottom-left corner
		let field(pos, label, name) = {
			rect(pos, (rel: (field-w, field-h)), name: name)
			content(name + ".center", text(size: 8pt)[#label])
		}
		// A ptr cell at `ptr-pos`, with a len cell directly below it, out of the
		// way of the horizontal arrow that will leave from the ptr cell.
		let fat-pointer(ptr-pos, name) = {
			field(ptr-pos, [ptr], name + "-ptr")
			field((ptr-pos.at(0), ptr-pos.at(1) - field-h), [len], name + "-len")

		}
		// A data box, vertically centered on the ptr cell whose bottom-left y is `ptr-y`.
		let data-box(ptr-y, name) = {
			let y = ptr-y - (data-h - field-h) / 2
			rect((data-x, y), (rel: (data-w, data-h)), name: name)
			content(name + ".center", text(size: 8pt)[data: `[T]`])
		}
		let arrow(from, to) = line(from, to, mark: (end: ">"))

		// ---------- row 1: raw pointer ----------
		let r1-y = 6.0
		content((0.5, r1-y + field-h + title-gap), text(size: 8pt)[`*mut T`])
		field((stack-x, r1-y), [ptr], "r1-ptr")
		data-box(r1-y, "r1-data")
		arrow("r1-ptr.east", "r1-data.west")
		content((0.5, r1-y - label-gap), text(size: 7pt)[8 bytes])

		// ---------- row 2: Box<[T]> ----------
		let r2-y = r1-y - label-gap - row-gap - field-h - title-gap
		content((0.5, r2-y + field-h + title-gap), text(size: 8pt)[`Box<[T]>`])
		fat-pointer((stack-x, r2-y), "r2")
		data-box(r2-y, "r2-data")
		arrow("r2-ptr.east", "r2-data.west")
		content((0.5, r2-y - field-h - label-gap), text(size: 7pt)[16 bytes])

		// ---------- row 3: Box<Box<[T]>>, same trick for the heap-allocated inner fat pointer ----------
		let r3-y = r2-y - field-h - label-gap - row-gap - field-h - title-gap
		content((0.5, r3-y + field-h + title-gap), text(size: 8pt)[`Box<Box<[T]>>`])
		field((stack-x, r3-y), [ptr], "r3-ptr")
		content((3.5, r3-y + field-h + title-gap), text(size: 7pt)[`Box<[T]>`])
		content((3.5, r3-y - field-h -label-gap), text(size: 7pt)[16 bytes])
		fat-pointer((inner-x, r3-y), "r3-inner")
		data-box(r3-y, "r3-data")
		arrow("r3-ptr.east", "r3-inner-ptr.west")
		arrow("r3-inner-ptr.east", "r3-data.west")
		content((0.5, r3-y - label-gap), text(size: 7pt)[8 bytes])
	}),
	caption: [Memory layout of a raw pointer, a boxed slice, and a doubly-boxed slice. `Box<Box<[T]>>`'s outer pointer happens to match a raw pointer's size (8 bytes), but reaching the array data now takes two pointer hops instead of one: its bit pattern is not interchangeable with the original raw pointer, even though its size is.],
	placement: auto,
) <indirection_size_diagram>
*/

// TODO: This is still an interesting bit of information. I do want to put it somewhere
// #figure(
// 	[\*Note: The linux allocator already implements a header on every allocation, used by C and Rust on linux systems.]
// )


// == Results <section_results>
// All 114 eligible pointer sources and their 933 interactions are rewritten using our methodology. All benchmarks still compile. Five benchmarks, buffer, genann, quadtree, rgba and urlparser contain testing code. With a slight modification of their `Cargo.toml` file to define a binary entrypoint, and the uncommenting main functions, we could run these tests by running `cargo run`. The test suites of buffer, genann, quadtree and rgba pass. The test suite of urlparser however fails. This is due to a heap buffer overflow that is already present in its original C code, but only shows after the rewrite. It allocates a single-byte buffer and writes an entire string into it, as shown in @figure_buffer_overflow_urlparser. Note that this buffer overflow is not caught because our rewrite made it more safe, but likely because our `header_malloc` behaves slightly different. The original `malloc` sometimes allocates more bytes for the sake of bookkeeping, so that when the `fmt_url` is written into it, it coincidentally fills the spare bytes the allocation has, while `header_malloc` already allocates a few bytes extra for the pointer header, so the unused allocation end is reached earlier. Once the allocation calls increase their size, the benchmark's test suite runs successfully.
//
// #figure(
// 	```c
// static char *
// get_part (char *url, const char *format, int l) {
// 	...
// 	char *tmp = malloc(sizeof(char));
// 	...
// 	sscanf(fmt_url, format, tmp);
// 	...
// }
// 	```
// ) <figure_buffer_overflow_urlparser>
//
// In 65 conversions, 64 of a function result and 1 of a returned variable, the rewriter transferred ownership with `into_raw()` instead of borrowing with `.as_ptr()`, as described in @context_rewriter. Without this exception, the test suite of genann fails due to a use-after-free.
//
// Benchmarks that contain no owned arrays, and therefore provide no rewrite target for us, such as brotli, binn, quadtree, libcsv, rgba, avl, bst and robotfindskitten, do not get any retyping. However, our rewriter does still rewire the allocation calls to our custom header functions. The still-passing tests indicate that the rerouting worked successfully.

#pagebreak()

= Correctness <chapter_correctness>
The previous chapter described how the rewrite works. This chapter answers the second half of RQ3: under which assumptions is the rewrite correct? Because we deliberately build on Crown's analysis, which is neither sound nor complete, the rewrite cannot be correct unconditionally. Instead, we make the conditions under which it is correct explicit. We first fix our terminology and state precisely what we claim, then list the assumptions the claim depends on, and argue that each part of the rewrite preserves the program's behavior under those assumptions. We close by discussing how rigorous this argument is, which assumptions are known to be violated in practice, and how much of a program the rewrite covers.

== Definitions <section_correctness_definitions>
The word "sound" is used in two different ways in this thesis, and both differ from the property we claim for the rewrite, which we call _correctness_. We keep the three apart explicitly.

The first is the soundness of an _analysis_. Crown's ownership analysis would be sound if every pointer it marks as owning is, in every execution, the unique owner of its allocation, and complete if it marks every such pointer as owning. Crown is neither, and we do not attempt to make it so. Instead, the properties of Crown that the rewrite depends on are stated as assumptions in @section_assumptions.

The second is the soundness of a _safety abstraction_, as illustrated in @unsafe_soundness: a function with a safe signature is sound if no way of calling it can cause undefined behavior. This applies to the methods of `CBox`, and is a different property than the correctness of the rewrite.

The third, and the one this chapter is about, is the _correctness_ of the rewrite itself. The rewrite is correct if it preserves the observable behavior of every program it is applied to. We compare against the program C2Rust produced, not against the C source, and interpret its calls into the C library according to C99. Where C99 leaves a choice to the implementation, or leaves a value indeterminate, a program may show any of the permitted behaviors. The _observable behavior_ of an execution consists of its input and output operations, such as reading standard input and writing to standard output, standard error and files, together with its exit status and whether it terminates. A panic or an abort is observable as well, which matters because the rewrite introduces both, through bounds checks and through the allocation functions. Two differences are explicitly not observable. The first is memory usage, as every allocation grows by the size of its header. This also means an allocation may fail with a null pointer under memory pressure where the original would have succeeded. The second is the concrete addresses of allocations.

== Correctness claim <section_correctness_claim>
Let $P$ be a program, and $R(P)$ the program our rewriter produces from it. We claim the following.

#block(inset: (left: 1em))[
  *Claim.* If $P$ satisfies assumptions A1 to A8 of @section_assumptions, then for every input on which the original C program has no undefined behavior, $R(P)$ has no undefined behavior on that input either, and every observable behavior of $R(P)$ on that input is one that $P$ may also show on that input.
]

For a program whose observable behavior does not depend on implementation-defined or indeterminate behavior, this means $R(P)$ behaves exactly as $P$.

The rewriter cannot detect undefined behavior, so it does not refuse programs that contain it. On such inputs, the rewrite may change how the undefined behavior manifests, as the urlparser benchmark in @chapter_evaluation shows.

== Assumptions <section_assumptions>
The claim depends on the following assumptions. A1, A2, A5 and A6 concern the results of Crown's analysis, A3 and A7 concern how the program uses its allocations, and A4 and A8 concern the output of C2Rust.

/ A1: _Owned pointers are base pointers._ Every value held by a pointer that Crown marks as an owned array is either null, or the base pointer of a live allocation. @section_scope_limitation argues why Crown's constraints make this hold: only allocation functions produce owning values, while offsets, the addresses of stack objects and the results of unknown functions are constrained to be non-owning. @section_known_violations discusses the cases that escape this argument.
/ A2: _Ownership is complete._ No non-owning copy of a pointer to a `CBox`'s allocation is used after the `CBox` is dropped. In other words, every place that keeps an allocation alive is marked as owning by Crown. Without this assumption, the automatic deallocation of a `CBox` would leave such a copy dangling.
/ A3: _Closed world._ Every allocation that the program reads, writes, reallocates or deallocates is made by `malloc`, `calloc`, `realloc`, `strdup` or `strndup`, and deallocated by `free` or `realloc`, which are exactly the functions the rewriter reroutes to their header-based counterparts. Other allocating C library functions are outside our scope, as the memory they return must not be passed to `free` or `realloc`. Likewise, no code outside of the rewritten project may allocate memory that the project itself frees, or free memory that the project allocated. This excludes callers of public functions that hand over their own allocations, user-supplied allocators, and callbacks registered from outside of the project.
/ A4: _All-zero values are valid._ Every type stored in an allocation, which includes every type C2Rust emits, Crown's `Option<Box<T>>` and `CBox` itself, has a valid value when all of its bytes are zero. The header allocation functions of @padding_allocations_with_header rely on this, as they zero every allocation.
/ A5: _Ownership transfers are moves._ After an owning value is transferred out of an owning place, that place is not read again until it is assigned a new value. The rewrite leaves a null `CBox` behind in such a place, where the original program still holds the pointer.
/ A6: _Ownership is unique._ At any point in an execution, at most one owning place holds a given allocation. Otherwise, two `CBox`es would free the same allocation.
/ A7: _Deallocation is visible._ Every deallocation or reallocation of an allocation held by an owned array goes through an interaction that names its pointer source, and is therefore found and rewritten. A deallocation hidden behind, for example, a hardcoded byte offset would free an allocation that a `CBox` frees again later.
/ A8: _Faithful translation._ On every input on which the original C program has no undefined behavior under C99, the program $P$ that C2Rust produced from it has no undefined behavior either, and shows the same observable behavior. This lets us use the rules of C99 to reason about $P$: an operation that is undefined behavior in C, such as using the value of a freed pointer, does not happen in $P$ either, even where Rust itself would allow it.

== Argument for correctness <section_correctness_argument>
We argue, under the assumptions of the previous section, that the rewrite is correct. Most of the argument consists of showing that each individual change the rewriter applies is correct by itself. However, the rewrite also introduces behavior that is not visible in the code: Rust deallocates a `CBox` automatically. We therefore list all types of changes our rewriter applies, together with this automatic deallocation, and argue for the correctness of each.
- _Rerouted allocation functions (A3, A4)._ Each header allocation function either returns a null pointer in case the requested allocation is impossible, just like in C99, or it returns a pointer to a valid block of memory of the requested size. `header_free` and `header_realloc` deallocate exactly the same blocks as `free` and `realloc` would. The header lies before the returned pointer, so the program can only reach it through an out-of-bounds access, which is undefined behavior. The C99 standard leaves the contents of a new allocation indeterminate, which leaves all zeroes as a valid option, and by A4 all zeroes is a valid value for every type. Existing code can therefore only show behavior it could already show with the original allocation functions. By A3, every allocation the program uses goes through these functions.
- _Representation is preserved._ A `CBox<[T]>` has the same size and alignment as `*mut T`, and holds exactly the address of the first element of the array as well, or all zeroes in case of a null pointer. Code that is not rewritten, but reads or writes to a retyped place therefore observes the same value as before.
- _Retyping yields valid CBoxes (A1)._ Wrapping an owned value with `from_raw` yields a valid `CBox`. By A1, the value is either null, which gives a null `CBox`, or the base pointer of an allocation, which has a header because it was made by one of the header allocation functions.
- _Context conversions preserve values (A5)._ The `as_ptr()` method returns exactly the same bits as the original pointer expression, and leaves the `CBox` unchanged. The `into_raw()` and `take()` methods also return the same bits, but leave a null behind, while the original code would still have held the original pointer. Because of A5, the original code no longer reads this remaining value before assigning a new one, so leaving a null behind is a valid rewrite.
- _Automatic deallocation only frees unused memory (A2, A5, A6)._ Rust drops a `CBox` at the end of its scope, when its place is assigned a new value, and when the struct containing it is dropped. At that point, the `CBox` is either null, or holds an allocation the original program never uses again: no other owning place holds it (A6), it was not moved out (A5), and no non-owning copy of it is used afterwards (A2). The original program would have leaked that allocation, so freeing it only changes the memory usage, which is not observable.
- _Deallocation is preserved (A7, A8)._ The `drop(x.take())` frees the same allocation at the same point the original code would do using the `free` function. While the `free` function does not modify the pointer value itself, using that value afterwards is undefined behavior in C99, and by A8 the original program therefore does not use it. That is why setting it to null is a correct rewrite as well. By A7, every deallocation of a `CBox`'s allocation is found and rewritten in this way.
- _Reallocation is preserved (A7, A8)._ The header realloc function behaves just like the original `realloc`, combined with setting the original pointer to null on success, where using it after reallocating would already have been undefined behavior. If the reallocation fails, the original `CBox` is left unchanged and the result is null, just like in C99. Calling `realloc` with a new size of `0` bytes, which C99 leaves implementation-defined, simply deallocates the given pointer, leaving both null, which is one of the permitted behaviors. By A7, every reallocation is found and rewritten.
- _Bounds checks never fire (A1, A8)._ By A1, each `CBox` holds the base pointer of its allocation. Obtaining a pointer more than one element past the end of an allocation, or before its start, is undefined behavior in C99, and so is dereferencing the element one past the end. The `get` and `get_mut` methods only return `None` when the original code would dereference outside of the allocation, and `get_ptr` only when the original code would create a pointer outside of it. By A8, this does not happen in the original program, so the `unwrap()` that the rewriter adds never panics. Because the `CBox` holds a base pointer, a negative index always leaves the allocation, which is why passing the index as a `usize` is safe.
- _Casts preserve values._ Casts only change the way the pointer is interpreted, and the `.cast()` method therefore also only changes the type parameter of `CBox`. When `into_raw()` or `as_ptr()` is called, it still yields the same bits, but now under the right type.

Each piece of code in the original program is then either left unchanged, which keeps working because the representation is preserved, or changed into new code that has the same observable behavior. The panics and other changes in the introduced code that would alter the observable behavior do not occur under the given assumptions. We can therefore argue, informally but in a structured way, that our claim holds.

== Rigor and evidence <section_rigor>
The argument above is not a formal proof. Such a proof would require a complete formal semantics of unsafe Rust, which does not exist yet. In particular, the rules for when raw pointers and references may point to the same memory are still being developed, with Stacked Borrows @stacked_borrows and Tree Borrows @tree_borrows as the main proposals.

The argument and the experiments of @chapter_evaluation complement each other. The argument covers every input, but only if the assumptions hold. The test suites of the benchmarks only cover their own inputs, but they run real programs, which may violate the assumptions. That a rewritten benchmark compiles shows only that it is well-typed, and says nothing about its behavior.

// TODO: Verify against the actual rewriter output that (1) structs with CBox fields are never copied, and (2) references from get/get_mut never outlive their expression

== Known violations of the assumptions <section_known_violations>
The assumptions about Crown's results do not always hold. Two cases escape the argument for A1. Unlike taking the address of a variable, which Crown constrains to be non-owning, constants and integer-to-pointer casts receive no constraint on their own ownership, so they may still become owning through the constraints of the variables they flow into. Furthermore, Crown merges the ownership of a variable over the entire function, so a variable that owns an allocation at one point may hold a borrowed value at another.

Crown's results can also be inconsistent with each other, as in @figure_borrowed_function_result, where the result of `strdup` is owning but the variable that receives it is not. Following the context rules would violate A2, as the `CBox` would be dropped while the variable still uses its allocation. Our rewriter handles these cases with the `into_raw()` exception of @context_rewriter, which is correct under A5, as the temporary `CBox` is not used again after the conversion.

Finally, the urlparser benchmark contains undefined behavior on the inputs of its own test suite. We enlarge its undersized allocations, but other undefined behavior remains, so the claim does not apply to it. We discuss it in @chapter_evaluation.

== Completeness <section_completeness>
For completeness, we distinguish three levels:
- *Completeness with respect to Crown's results.* Every pointer source that Crown marks as an owned array, and that is neither an output parameter nor a nested array, is retyped to `CBox`, and every interaction with it that our analysis finds is rewritten. This is the level our rewriter aims for, and which we measure in @chapter_evaluation.
- *Completeness with respect to the program.* Every owned heap array of a project is retyped. Because Crown's analysis is incomplete, this level is neither achieved nor measurable: an owned array that Crown does not recognize as owned is left as a raw pointer.
- *Completeness of the interactions.* Every interaction with a retyped pointer source is found. As discussed in @chapter_array_manifestation, the Rust Analyzer cannot find interactions that do not mention the source by name, such as accesses through hardcoded byte offsets. Such an interaction keeps working on a `CBox` because its representation is preserved (@section_correctness_argument), unless it deallocates or reallocates the array, which A7 excludes.

Under the assumptions of @section_assumptions, incompleteness at any of these levels never makes the rewrite incorrect: an array that is not found is left as the raw pointer it was, and an interaction that is not found keeps working on the `CBox`, because its representation is preserved.

#pagebreak()

= Evaluation <chapter_evaluation>

This chapter answers RQ4: to what extent does the rewrite reduce the unsafe surface area of the translated programs, while preserving their behavior? We first measure how each interaction with an owned array is rewritten, and then whether the rewritten programs still compile and pass their test suites.

== Experimental setup
We run the rewriter on the same 20 benchmarks as in @chapter_array_manifestation, after Crown's preprocessing and analysis. The rewriter records, for every interaction it rewrites, which rewrite it applied. All tables in this chapter are generated from these records, using a script that runs the complete pipeline from a fresh copy of the benchmarks. This pipeline, together with the tools and benchmarks, is available as an artifact @thesis_artifact, and as a Docker image with everything installed and built @thesis_artifact_image.

Five benchmarks, buffer, genann, quadtree, rgba and urlparser, contain test code. To run these tests, we added a binary entry point to their `Cargo.toml` that calls their test function, such that `cargo run` executes the test suite. We run each test suite both before and after the rewrite.

== Rewrite coverage
The rewriter retypes all 114 owned array sources to `CBox`. Their 933 usages from @effective_usage_distribution, together with 69 interactions in which another value flows into an owned array, through assignment, initialization, parameter passing or a return, result in 1002 rewritten interactions. These interactions are counted as usages of the incoming value in @effective_usage_distribution, but still need to be rewritten, as the receiving array is now a `CBox`. All 1002 interactions are rewritten; none had to be left untouched.

@figure_rewrite_outcomes shows which rewrite each type of usage received. The element accesses, deallocations and reallocations each map onto a single safe counterpart, whereas the general usages, such as storing or passing a pointer, depend on the context they occur in.


#figure(
  result-table(
    "rewrite_outcomes",
    (
      ("usage_type", [Effective usage type]),
      "AsPtr",
      "BoundsCheckedElement",
      "BoundsCheckedPointer",
      "Deallocate",
      "FromRaw",
      "IntoRaw",
      "MoveBox",
      "RawReference",
      "Reallocate",
      "ReallocateIntoRaw",
      ("total", [total]),
    ),
    header-style: vertical,
    hide-zero: true,
    align: (col, row) => if row == 0 { bottom + center } else if col == 0 { left } else { center },
  ),
  caption: [Rewrite applied to each interaction with an owned array, per effective usage type. The columns name the applied rewrite, such as `AsPtr` for `p.as_ptr()` (@context_transition_table). Empty cells are zero.],
) <figure_rewrite_outcomes>

To quantify the gain in safety, we group these outcomes into three categories, shown per benchmark in @figure_safety_improvement:
- *Bounds checked* (301): element accesses now use `get` or `get_ptr`, which check the index against the allocation size, and panic instead of accessing memory outside of the allocation.
- *Stays a `CBox`* (256): the value remains managed by a `CBox`, through moves with `take()`, adopted allocations with `from_raw()`, deallocations and reallocations.
- *Leaves as raw pointer* (445): the value is converted back into a raw pointer with `as_ptr()` or `into_raw()`, typically to be passed to code that still expects a raw pointer.

#figure(
  result-table("safety", (
    ("benchmark", [benchmark]),
    ("bounds_checked", [bounds]),
    ("stays_cbox", [cbox]),
    ("leaves_as_raw", [raw]),
    ("unchanged", [const]),
    ("promoted", [promoted]),
    ("total", [total]),
    (r => pct(r.leaves_as_raw, r.total), [raw\_\%]),
  )),
  caption: [The safety improvement per benchmark. The eight benchmarks without owned arrays to rewrite (avl, binn, brotli, bst, libcsv, quadtree, rgba and robotfindskitten) have no rewritten interactions, and are therefore left out.],
) <figure_safety_improvement>

Altogether, 56% of the interactions with owned arrays have become safer in a way that goes beyond automatic deallocation. Of the 445 interactions that still produce a raw pointer, 110 are null checks and address computations, which only inspect the pointer value and do not directly access the array. The remaining 335 interactions, 33% of the total, hand a raw pointer to code that can access the array without bounds checking. This share differs strongly per benchmark: from 27% in libtree to 85% in urlparser, where most arrays are strings passed to C library functions such as `sscanf` and `strcmp`.

Benchmarks without owned arrays, such as avl, bst, binn and brotli, are only affected by the rerouting of their allocation calls from @rerouting_allocation_calls.

== Behavior preservation
All 20 benchmarks compile after the rewrite. As compilation alone does not prove a correct rewrite, we also ran the five available test suites to see if program behavior changed.

The test suites of buffer, genann, quadtree and rgba pass. Notably, quadtree and rgba contain no owned arrays, so their tests only exercise the rerouted allocation functions. The test suite of genann only passes due to the exception described in @context_rewriter: in 65 interactions, 64 of which are function results, the rewriter transferred ownership with `into_raw()` instead of borrowing with `as_ptr()`. Without this exception, genann's tests fail due to a use-after-free.

The test suite of urlparser initially failed, as shown in @figure_buffer_overflow_urlparser. Its C code allocates buffers that are too small, mostly single-byte buffers, and writes entire parts of a URL into them, a heap buffer overflow that is present in the original C code. This overflow was not caught because the rewrite made the program safer, as the buffer involved is not an owned array, but because the header changes the memory layout around allocations. We enlarged the 17 undersized allocations in the translated benchmark, after which its test suite passes both before and after the rewrite. Other undefined behavior remains, such as out-of-bounds reads, but it does not affect the outcome of the test suite. This again shows how easily undefined behavior remains hidden in C programs that appear to work correctly.

#figure(
  ```c
  static char *
  get_part (char *url, const char *format, int l) {
  	...
  	char *tmp = malloc(sizeof(char));
  	...
  	sscanf(fmt_url, format, tmp);
  	...
  }
  ```,
  caption: [The heap buffer overflow in urlparser's `get_part` function. `tmp` is allocated with room for a single character, but `sscanf` writes an entire part of the URL into it, followed by a terminating null byte. The code is simplified for brevity.],
) <figure_buffer_overflow_urlparser>

#pagebreak()

= Limitations <chapter_limitations>
The rewrite is only as correct as the ownership results it is based on. This chapter discusses where these results, or the assumptions of our scope, fall short.

== Dependence on Crown's ownership
While our rewriter correctly handles some edge cases with regards to Crowns analysis, not all exceptions can be covered. For example, if a temporary owning CBox stores a borrowed pointer elsewhere, and that borrowed pointer outlives the original CBox, a double free will occur, as the CBox would have already deallocated the memory the moment it went out of scope. This pattern occurs 11 times, spread over three benchmarks. Two causes lead to this. In `lil` and `lodepng`, Crown's result is incorrect, as the C code frees pointers that are marked as `Transient`. In `libtree`, the field implements a small-vector optimisation: it points to a buffer inside the struct for small sizes, and to a heap allocation for larger sizes. Crown assigns a single ownership to each field, so it cannot express ownership that depends on the size. Transferring ownership with `into_raw()` is not a general solution here, as the transferred value is still used after the assignment. Deciding whether a `CBox` is still used after a conversion requires a liveness analysis, which we leave as future work. More generally, dropping a CBox at the end of its scope assumes that Crown's ownership is complete: every place that keeps an owned pointer alive must be marked as owning.

== Programs with undefined behavior
Our rewrite only preserves the behavior of programs without undefined behavior. Undefined behavior may surface differently after the rewrite, as with urlparser's heap buffer overflow, or behave differently, such as testing a pointer against null after it has been deallocated, as discussed in @targeted_usage_rewriting.

== Scope
We only rewrite owned arrays that are not output parameters, which leaves the 2887 non-owned arrays, 12 owned output parameters and all nested arrays as raw pointers. For output parameters, Crown's 'owning' attribute can no longer indicate the pointer is always a pointer to a heap allocation, so we cannot retype it to a CBox. For nested arrays, Crown does not distinguish between the ownership of the individual elements. Lastly, 44% of the rewritten interactions still produce a raw pointer, as the code they pass the array to has not been rewritten.

#pagebreak()

= Concluding remarks <chapter_conclusion>
This thesis set out to answer how Crown's ownership analysis can be used to automatically rewrite owned heap arrays in C2Rust output into safe Rust, while preserving the behavior of the program.

*RQ1.* C2Rust represents every C array as a thin raw pointer without length information. Of the 7534 pointer sources in our 20 benchmarks, Crown's analysis identifies 3013 as arrays, of which only 114 are owned arrays we can rewrite. Most arrays are borrowed, especially in libraries that operate on buffers provided by their caller. Around a third of the usages of owned arrays access elements directly, which is exactly the unsafety a safe array type can address.

*RQ2.* No existing Rust type combines automatic deallocation, representation equivalence and bounds checking. Our `CBox` type achieves all three by storing the size of each allocation in a header directly before its data, at the cost of routing every allocation in the program through our own allocation functions, and a slight overhead in memory usage.

*RQ3.* Owned arrays can be rewritten by rerouting all allocations, retyping owned arrays to `CBox`, adapting each interaction to its context, and replacing indexing, deallocation and reallocation with their safe counterparts. This is correct as long as Crown's ownership results are correct and complete, and the original program contains no undefined behavior. Where Crown's results conflict, the rewriter transfers ownership, which fails as a memory leak rather than as undefined behavior.

*RQ4.* All 1002 interactions with the 114 owned arrays are rewritten, and all benchmarks still compile. 301 interactions are now bounds checked and 256 remain managed by a `CBox`, while 445 still produce a raw pointer, mostly to pass the array to code that is not rewritten yet. All five test suites pass after fixing a heap buffer overflow in urlparser's original code, which our allocation header coincidentally exposed.

Answering the main question, Crown's ownership analysis is sufficient to safely rewrite owned heap arrays, and thereby reduces the unsafe surface area of translated programs, but does not eliminate it. The rewrite is limited by the precision of the ownership results more than by the rewrite itself: owned arrays are only a small fraction of all arrays, and the conflicts we found in Crown's results for arrays were never exercised before, as Crown itself does not rewrite arrays.

Future work can extend this approach in several directions. Borrowed arrays could be rewritten to slices, which requires knowing their length, for example from the header of the allocation they point into. Nested arrays and output parameters require ownership results per level and per element. A liveness analysis would allow ownership transfers in more places, removing the remaining limitation of borrowed `CBox`es. Finally, rewriting the code that arrays are passed to would reduce the 44% of interactions that still produce a raw pointer.

Throughout this research, the benchmarks also turned out to contain undefined behavior and memory leaks, despite being small and well tested. This illustrates the danger of unsafe languages, and strengthens the case for migrating to safer languages such as Rust.

#pagebreak()

#basic_header(bibliography("thesis/citations.bib", full: false))
