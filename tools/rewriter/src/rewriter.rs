use std::{
	cmp::Reverse,
	collections::{HashMap, HashSet},
};

use base_db::SourceDatabase;
use hir::HasSource;
use ide::{TextRange, TextSize};
use syntax::{AstNode, ast::HasAttrs};

use crate::{
	ProjectContext, context_rewrite_planner::{
		ContextSwitchRewritePlan, TypeRewritePlan,
	}, extern_function::{CFunction, CFunctionDeclaration}, pointer_context::{PointerContext, get_pointer_context_of_source}, pointer_source::{NamedPointerSource, PointerSourceLocation, get_element_type_of_pointer}, runtime_install::RuntimePaths,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxMove {
	pub old_syntax: TextRange,
	pub new_location: TextSize,
}

#[derive(Clone, Debug, Eq)]
pub struct Rewrite {
	pub file_id: vfs::FileId,
	pub old_syntax_range: TextRange,
	pub new_text: String,
	pub syntax_moves: Vec<SyntaxMove>,
	pub _file_location: String,
}
impl PartialEq for Rewrite {
	fn eq(&self, other: &Self) -> bool {
		self.file_id == other.file_id
			&& self.old_syntax_range == other.old_syntax_range
			&& self.new_text == other.new_text
	}
}
impl std::hash::Hash for Rewrite {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.file_id.hash(state);
		self.old_syntax_range.hash(state);
		self.new_text.hash(state);
	}
}

pub fn get_rewrite_for_source_plan(plan: &TypeRewritePlan) -> Rewrite {
	let element_type = plan.element_type.text().to_string();
	Rewrite {
		file_id: plan.file_id,
		old_syntax_range: plan.old_type.text_range(),
		new_text: match plan.new_pointer_context {
			PointerContext::Box => format!("CBox<[{}]>", element_type),
			PointerContext::Raw { .. } => plan.old_type.text().to_string(),
			_ => unreachable!(
				"Should not rewrite sources to something other than raw, boxed or output param"
			),
		},
		syntax_moves: vec![],
		_file_location: plan._file_location.clone(),
	}
}

pub fn get_rewrites_for_struct_attributes(
	all_sources: &Vec<NamedPointerSource>,
	context: &ProjectContext,
) -> Vec<Rewrite> {
	// Structs that directly own a field being rewritten into something
	// non-Copy/non-Clone (Box, CBox, Option<&mut T>, ...).
	let directly_tainted: HashSet<hir::Struct> = all_sources
		.iter()
		.filter(|source| !get_pointer_context_of_source(source).is_raw())
		.filter_map(|source| match &source.location {
			PointerSourceLocation::StructField { struc, .. } => Some(*struc),
			_ => None,
		})
		.collect();

	let mut memo: HashMap<hir::Struct, bool> = HashMap::new();
	let mut tainted: HashSet<hir::Struct> = directly_tainted.clone();

	// Sweep every struct in the crate; each one pulls in whatever its own
	// by-value fields transitively taint, memoized so shared sub-structs
	// aren't recomputed.
	for krate in hir::Crate::all(context.db) {
		if !krate.origin(context.db).is_local() {
			continue;
		}
		for module in krate.modules(context.db) {
			for item in module.declarations(context.db) {
				let hir::ModuleDef::Adt(hir::Adt::Struct(struc)) = item else {
					continue;
				};
				if struct_is_tainted(struc, context, &directly_tainted, &mut memo) {
					tainted.insert(struc);
				}
			}
		}
	}

	let mut attr_rewrites: Vec<Rewrite> = vec![];

	for struc in tainted {
		let in_file = struc.source(context.db).unwrap();
		let Some(attr) = in_file
			.value
			.attrs()
			.find(|attr| attr.simple_name().as_deref() == Some("derive"))
		else {
			continue;
		};
		let Some(file_id_editioned) = in_file.file_id.file_id() else {
			continue; // macro-generated, nothing to rewrite
		};

		attr_rewrites.push(Rewrite {
			file_id: file_id_editioned.file_id(context.db),
			old_syntax_range: attr.syntax().text_range(),
			new_text: format!(""),
			syntax_moves: vec![],
			_file_location: format!("struct_attr_removal"),
		});
	}

	attr_rewrites
}

/// A struct is tainted (can't stay `Copy`/`Clone`) if it directly holds a
/// rewritten field, or if it embeds — *by value* — another tainted struct.
/// Pointer/CBox fields don't propagate taintedness: they're already opaque
/// indirections regardless of what they point to, which is exactly what
/// `as_adt()` already filters for (it returns `None` for raw pointer types).
fn struct_is_tainted(
	struc: hir::Struct,
	context: &ProjectContext,
	directly_tainted: &HashSet<hir::Struct>,
	memo: &mut HashMap<hir::Struct, bool>,
) -> bool {
	if let Some(&cached) = memo.get(&struc) {
		return cached;
	}
	memo.insert(struc, false); // defensive; by-value containment can't actually cycle

	let mut result = directly_tainted.contains(&struc);
	if !result {
		for field in struc.fields(context.db) {
			let Some(hir::Adt::Struct(inner)) = field.ty(context.db).to_type(context.db).as_adt()
			else {
				continue;
			};
			if struct_is_tainted(inner, context, directly_tainted, memo) {
				result = true;
				break;
			}
		}
	}

	memo.insert(struc, result);
	result
}

pub fn get_rewrites_for_runtime_installation(
	paths: &RuntimePaths,
	usage_files: HashSet<vfs::FileId>,
	context: &ProjectContext,
	all_c_functions: &HashMap<vfs::FileId, HashSet<CFunctionDeclaration>>,
) -> Vec<Rewrite> {
	let root_file_offset = get_file_offset_for_imports_and_such(paths.root_file, context);

	let mut rewrites = vec![];

	// In the root file, declare the modules
	rewrites.push(Rewrite {
		file_id: paths.root_file,
		old_syntax_range: TextRange::empty(root_file_offset),
		new_text: format!(
			"mod {};\nmod {};",
			paths.cbox_module_name, paths.alloc_module_name
		),
		syntax_moves: vec![],
		_file_location: format!("root"),
	});

	// In each file where CBox is used, import CBox
	for usage_file in usage_files {
		let file_offset = get_file_offset_for_imports_and_such(usage_file, context);
		rewrites.push(Rewrite {
			file_id: usage_file,
			old_syntax_range: TextRange::empty(file_offset),
			new_text: format!("use {}::*;", paths.cbox_base_path),
			syntax_moves: vec![],
			_file_location: format!("usage_file"),
		});
	}

	// And for all files in general, for each C allocation function that is
	// declared: remove its extern decl + add `use header_<name> as <name>;`, so
	// every allocation, including through function pointers, gets a header
	let rerouted_functions = [
		(CFunction::Malloc, "malloc"),
		(CFunction::Calloc, "calloc"),
		(CFunction::Realloc, "realloc"),
		(CFunction::Free, "free"),
		(CFunction::Strdup, "strdup"),
		(CFunction::Strndup, "strndup"),
	];
	for (file_id, declarations) in all_c_functions {
		let file_offset = get_file_offset_for_imports_and_such(*file_id, context);
		for (c_function, name) in &rerouted_functions {
			let Some(decl) = declarations
				.iter()
				.find(|decl| decl.function_name == *c_function)
			else {
				continue;
			};
			let remove_decl = Rewrite {
				file_id: *file_id,
				old_syntax_range: decl.decl.text_range(),
				new_text: format!(""),
				syntax_moves: vec![],
				_file_location: format!("usage_file_removal"),
			};
			let add_use = Rewrite {
				file_id: *file_id,
				old_syntax_range: TextRange::empty(file_offset),
				new_text: format!("use {}::header_{name} as {name};", paths.alloc_base_path),
				syntax_moves: vec![],
				_file_location: format!("usage_file_addition"),
			};
			rewrites.extend([remove_decl, add_use]);
		}
	}

	rewrites
}

/// Calculates at which position in a file new `use ...;` or `mod ...;`
/// statements can be added
pub fn get_file_offset_for_imports_and_such(
	file: vfs::FileId,
	context: &ProjectContext,
) -> TextSize {
	let source_file = context
		.rust_analysis
		.parse(file)
		.expect("Failed to get source of file");
	let file_offset = source_file
		.attrs()
		.last()
		.map(|attr| attr.syntax().text_range().end())
		.unwrap_or(TextSize::new(0));
	file_offset
}

/// If `child`'s range is fully covered by one of `parent`'s syntax moves (i.e.
/// `child` sits inside a span of `parent.new_text` that is still a verbatim
/// copy of the original source), returns the range within `parent.new_text`
/// where `child`'s original content currently lives.
fn find_nested_position(parent: &Rewrite, child: &Rewrite) -> Option<TextRange> {
	parent.syntax_moves.iter().find_map(|m| {
		if !m.old_syntax.contains_range(child.old_syntax_range) {
			return None;
		}
		let move_relative_start = m.new_location - parent.old_syntax_range.start();
		let child_offset_within_move = child.old_syntax_range.start() - m.old_syntax.start();
		let start = move_relative_start + child_offset_within_move;
		Some(TextRange::at(start, child.old_syntax_range.len()))
	})
}

/// Merges any rewrite whose range is nested inside another rewrite's
/// (still-verbatim) syntax directly into that outer rewrite's `new_text`, so
/// a single edit covers both instead of two overlapping ones. This is what
/// lets e.g. rewriting `first` inside `genann_copy(first)` compose with
/// wrapping the whole call in `.to_raw()`.
fn merge_nested_rewrites(mut rewrites: Vec<Rewrite>) -> Vec<Rewrite> {
	loop {
		// Always resolve the right-most pending nested pair first: splicing
		// into `new_text` only shifts positions *after* the splice, so
		// earlier (further left) not-yet-applied offsets, computed from the
		// untouched `syntax_moves`, stay valid.
		let next = rewrites
			.iter()
			.enumerate()
			.flat_map(|(child_idx, child)| {
				rewrites
					.iter()
					.enumerate()
					.filter(move |&(parent_idx, _)| parent_idx != child_idx)
					.filter_map(move |(parent_idx, parent)| {
						find_nested_position(parent, child)
							.map(|range| (child_idx, parent_idx, range))
					})
			})
			.max_by_key(|(_, _, range)| range.start());

		let Some((child_idx, parent_idx, splice_range)) = next else {
			break;
		};

		let child = rewrites[child_idx].clone();
		let parent_range_start = rewrites[parent_idx].old_syntax_range.start();
		let start: usize = splice_range.start().into();
		let end: usize = splice_range.end().into();
		rewrites[parent_idx]
			.new_text
			.replace_range(start..end, &child.new_text);

		// Any syntax move the child itself carried now lives inside the
		// parent's new_text - re-anchor it so deeper nesting keeps working.
		let shift = (splice_range.start() + parent_range_start) - child.old_syntax_range.start();
		rewrites[parent_idx]
			.syntax_moves
			.extend(child.syntax_moves.into_iter().map(|mut inner_move| {
				inner_move.new_location += shift;
				inner_move
			}));

		rewrites.remove(child_idx);
	}
	rewrites
}

/// Two rewrite ranges conflict when they overlap over a real span, or when
/// a zero-width insertion point falls strictly inside the other's
/// (non-empty) range. `TextRange::intersect` alone misses the second case:
/// intersecting a point with anything always yields a zero-length range,
/// which `is_empty()` reports as "no overlap" even when the point sits
/// buried inside a larger replacement — exactly the shape of bug that let a
/// genuine unresolved conflict (a `.as_ptr()` insertion nested inside an
/// un-merged `CBox::from_raw(..)` wrap, because the wrap's `syntax_moves`
/// was empty) slip through silently and corrupt output instead of panicking.
fn ranges_conflict(a: TextRange, b: TextRange) -> bool {
	if let Some(intersection) = a.intersect(b) {
		if !intersection.is_empty() {
			return true;
		}
	}

	let point_strictly_inside = |point: TextSize, range: TextRange| {
		!range.is_empty() && range.start() < point && point < range.end()
	};

	if a.is_empty() && point_strictly_inside(a.start(), b) {
		return true;
	}
	if b.is_empty() && point_strictly_inside(b.start(), a) {
		return true;
	}

	false
}

/// Sorts and removes duplicates of a list of rewrites
fn organize_rewrites(rewrites: Vec<Rewrite>) -> HashMap<vfs::FileId, Vec<Rewrite>> {
	let mut ordered: HashMap<vfs::FileId, HashSet<Rewrite>> = HashMap::new();

	// First, collect all rewrites under their respective file
	for rewrite in &rewrites {
		if !ordered
			.entry(rewrite.file_id)
			.or_default()
			.insert(rewrite.clone())
		{
			eprintln!("Already contained rewrite:");
			eprintln!("{}", rewrite._file_location);
			eprintln!("into: {}", rewrite.new_text.clone());
		}
	}

	let mut sorted = HashMap::new();

	for (file, intersections) in ordered {
		let merged = merge_nested_rewrites(intersections.into_iter().collect());

		// Whatever intersections remain after merging nested rewrites are
		// genuine, unresolvable conflicts.
		for a in &merged {
			for b in &merged {
				if a == b {
					continue;
				}
				if ranges_conflict(a.old_syntax_range, b.old_syntax_range) {
					println!("Intersecting rewrite:");
					println!("{}", a._file_location);
					println!("into: {}", a.new_text.clone());
					println!("Range: {:?}", a.old_syntax_range);
					println!(" - And - ");
					println!("{}", b._file_location);
					println!("into: {}", b.new_text.clone());
					println!("Range: {:?}", b.old_syntax_range);
					panic!("Found intersecting rewrite")
				}
			}
		}

		let mut sorted_rewrites = merged;
		sorted_rewrites.sort_by_key(|rewrite| Reverse(rewrite.old_syntax_range.start()));

		sorted.insert(file, sorted_rewrites);
	}

	sorted
}

pub fn write_rewrites(rewrites: Vec<Rewrite>, context: &ProjectContext) {
	let all_rewrites = organize_rewrites(rewrites);

	for (file_id, rewrites) in all_rewrites {
		let mut file_source_text = context.db.file_text(file_id).text(context.db).to_string();

		for rewrite in rewrites {
			let start: usize = rewrite.old_syntax_range.start().into();
			let end: usize = rewrite.old_syntax_range.end().into();
			file_source_text.replace_range(start..end, &rewrite.new_text);
		}

		let vfs_path = context.vfs.file_path(file_id);
		let abs_path = vfs_path.as_path().expect("Non-filesystem path in vfs");
		std::fs::write(abs_path, file_source_text)
			.unwrap_or_else(|e| panic!("Failed to write to {abs_path:?}: {e}"));
	}
}

// pub fn get_rewrite_for_context_switch_rewrite_plan(
// 	plan: &ContextSwitchRewritePlan,
// 	context: &ProjectContext,
// ) -> Rewrite {
// 	let file_id = plan.file_id;
// 	let syntax_moves = vec![];
// 	let _file_location = plan._file_location.clone();
//
// 	match (&plan.current_context, &plan.target_context) {
// 		(PointerContext::Box, PointerContext::Box) => {
// 			// Rewrite is simple: Keep as box, but all casts should be rewritten
// 			if let Some(last_cast) = plan.casts.last() {
// 				let last_cast_element = get_element_type_of_pointer(last_cast, context).unwrap();
// 				let wrapped_range = plan.effective_pointer.text_range();
//
// 				Rewrite {
// 					file_id,
// 					old_syntax_range: wrapped_range,
// 					new_text: format!(
// 						"{}.cast::<[{}]>()",
// 						plan.direct_pointer.text().to_string(),
// 						last_cast_element.syntax().text().to_string(),
// 					),
// 					syntax_moves: vec![SyntaxMove {
// 						old_syntax: plan.direct_pointer.text_range(),
// 						new_location: wrapped_range.start(),
// 					}],
// 					_file_location,
// 				}
// 			} else {
// 				//
// 				Rewrite {
// 					file_id,
// 					old_syntax_range: TextRange::empty(plan.effective_pointer.text_range().end()),
// 					new_text: format!(".take()"),
// 					syntax_moves,
// 					_file_location,
// 				}
// 			}
// 		}
// 		(PointerContext::Box, PointerContext::Raw { but_still_owned }) => {
// 			// When box is converted to raw, existing casts automatically function
// 			// normally, so no need to make distinction
// 			Rewrite {
// 				file_id,
// 				old_syntax_range: TextRange::empty(plan.direct_pointer.text_range().end()),
// 				new_text: if *but_still_owned {
// 					format!(".into_raw()")
// 				} else {
// 					format!(".as_ptr()")
// 				},
// 				syntax_moves,
// 				_file_location,
// 			}
// 		}
// 		(PointerContext::Box, PointerContext::RawRef { is_mut }) => {
// 			if let Some(_last_cast) = plan.casts.last() {
// 				todo!("Support cast")
// 			}
//
// 			// We have a Box, but we need a raw reference to the internal pointer
// 			Rewrite {
// 				file_id,
// 				old_syntax_range: plan.effective_pointer.text_range(),
// 				new_text: if *is_mut {
// 					format!("{}.get_raw_ref_mut()", plan.direct_pointer.text())
// 				} else {
// 					format!("{}.get_raw_ref()", plan.direct_pointer.text())
// 				},
// 				syntax_moves,
// 				_file_location,
// 			}
// 		}
// 		(PointerContext::Raw { .. }, PointerContext::Box) => {
// 			// CBox::from_raw(), and keep all casts inside
// 			let wrapped_range = plan.effective_pointer.text_range();
// 			let prefix = "CBox::from_raw(";
// 			Rewrite {
// 				file_id,
// 				old_syntax_range: wrapped_range,
// 				new_text: format!("{prefix}{})", plan.effective_pointer.text().to_string()),
// 				syntax_moves: vec![SyntaxMove {
// 					old_syntax: wrapped_range,
// 					new_location: wrapped_range.start() + TextSize::of(prefix),
// 				}],
// 				_file_location,
// 			}
// 		}
// 		(PointerContext::Raw { .. }, PointerContext::Raw { .. }) => {
// 			// Empty rewrite
// 			Rewrite {
// 				file_id,
// 				old_syntax_range: TextRange::empty(TextSize::new(0)),
// 				new_text: format!(""),
// 				syntax_moves,
// 				_file_location,
// 			}
// 		}
// 		(PointerContext::Raw { .. }, PointerContext::RawRef { .. }) => {
// 			// Empty rewrite
// 			Rewrite {
// 				file_id,
// 				old_syntax_range: TextRange::empty(TextSize::new(0)),
// 				new_text: format!(""),
// 				syntax_moves,
// 				_file_location,
// 			}
// 		}
// 		(PointerContext::RawRef { .. }, _) => unreachable!("Cannot have this as inner context"),
// 	}
// }


// pub fn _get_rewrite_for_usage_plan(plan: &ContextSwitchRewritePlan) -> Rewrite {
// 	let file_id = plan.file_id;
// 	let old_syntax_range = plan.text_to_rewrite.text_range();

// 	let mut rewrite = match &plan.value_rewrite_plan {
// 		ValueRewritePlan::NullPointer => Rewrite {
// 			file_id,
// 			old_syntax_range,
// 			new_text: format!("CBox::null()"),
// 			syntax_moves: vec![],
// 		},
// 		ValueRewritePlan::Deallocate { .. } => Rewrite {
// 			file_id,
// 			old_syntax_range,
// 			new_text: format!("()"),
// 			syntax_moves: vec![],
// 		},
// 		ValueRewritePlan::Realloc {
// 			old_pointer,
// 			new_size,
// 		} => {
// 			let old_pointer_start = old_syntax_range.start() + TextSize::new(14);
// 			let new_size_start =
// 				old_pointer_start + old_pointer.text_range().len() + TextSize::new(2);
// 			Rewrite {
// 				file_id,
// 				old_syntax_range,
// 				new_text: format!(
// 					"CBox::realloc({}, {})",
// 					old_pointer.text().to_string(),
// 					new_size.text().to_string()
// 				),
// 				syntax_moves: vec![
// 					SyntaxMove {
// 						old_syntax: old_pointer.text_range(),
// 						new_location: old_pointer_start,
// 					},
// 					SyntaxMove {
// 						old_syntax: new_size.text_range(),
// 						new_location: new_size_start,
// 					},
// 				],
// 			}
// 		}
// 		ValueRewritePlan::NullCheck { old_pointer } => Rewrite {
// 			file_id,
// 			old_syntax_range,
// 			new_text: format!("{}.is_null()", old_pointer.text().to_string()),
// 			syntax_moves: vec![],
// 		},
// 		ValueRewritePlan::KeepAsIs => todo!(),
// 		ValueRewritePlan::Calloc {
// 			element_count,
// 			element_size,
// 		} => {
// 			let element_count_start = old_syntax_range.start() + TextSize::new(18);
// 			let element_size_start =
// 				element_count_start + element_count.text_range().len() + TextSize::new(2);
// 			Rewrite {
// 				file_id,
// 				old_syntax_range,
// 				new_text: format!(
// 					"CBox::from_calloc({}, {})",
// 					element_count.text().to_string(),
// 					element_size.text().to_string()
// 				),
// 				syntax_moves: vec![
// 					SyntaxMove {
// 						old_syntax: element_count.text_range(),
// 						new_location: element_count_start,
// 					},
// 					SyntaxMove {
// 						old_syntax: element_size.text_range(),
// 						new_location: element_size_start,
// 					},
// 				],
// 			}
// 		}
// 		ValueRewritePlan::Malloc { byte_count } => {
// 			let byte_count_start = byte_count.text_range().start();
// 			Rewrite {
// 				file_id,
// 				old_syntax_range,
// 				new_text: format!("CBox::from_malloc({})", byte_count.text().to_string()),
// 				syntax_moves: vec![SyntaxMove {
// 					old_syntax: byte_count.text_range(),
// 					new_location: byte_count_start,
// 				}],
// 			}
// 		}
// 		ValueRewritePlan::DereferenceElement { .. } => Rewrite {
// 			file_id,
// 			old_syntax_range,
// 			new_text: todo!(),
// 			syntax_moves: todo!(),
// 		},
// 		ValueRewritePlan::ReferenceElement { .. } => todo!(),
// 		ValueRewritePlan::AddressOf { .. } => todo!(),
// 	};

// 	wrap_rewrite_in_right_context_switcher(
// 		&mut rewrite,
// 		&plan.inner_pointer_context,
// 		&plan.outer_pointer_context,
// 	);

// 	rewrite
// }
