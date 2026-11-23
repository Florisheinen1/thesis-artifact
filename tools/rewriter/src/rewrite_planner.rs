use std::collections::{HashMap, HashSet};

use ide::TextSize;
use serde::Serialize;
use syntax::{AstNode, ast};

use crate::{
	ProjectContext,
	effective_usage_type::{EffectiveUsageTypeVariant, get_effective_usage_type},
	extern_function::CFunctionDeclaration,
	pointer_context::{
		PointerContext, get_pointer_context_of_source, get_pointer_context_of_usage_variant,
	},
	pointer_interactions::NamedPointerInteraction,
	pointer_source::{NamedPointerSource, PointerSourceLocation, get_element_type_of_pointer},
	rewriter::{Rewrite, SyntaxMove},
};

/// What the rewriter did with a single pointer interaction. Recorded for
/// every interaction, so the safety gain of a rewrite can be counted exactly.
#[derive(Debug, Clone, Copy, Serialize)]
pub enum RewriteOutcome {
	/// `*ptr.offset(i)` to `*ptr.get(i).unwrap()` (or `get_mut`): bounds
	/// checked, through a safe reference
	BoundsCheckedElement,
	/// `ptr.offset(i)` to `ptr.get_ptr(i).unwrap()`: bounds checked, but the
	/// result is still a raw pointer
	BoundsCheckedPointer,
	/// `realloc(ptr, n)` to `ptr.from_realloc(n)`, the result stays a CBox
	Reallocate,
	/// Same, but the result is handed on as an owning raw pointer
	ReallocateIntoRaw,
	/// `free(ptr)` to `drop(ptr.take())`
	Deallocate,
	/// A CBox moved into another CBox with `take()`
	MoveBox,
	/// A raw allocation adopted by a CBox with `CBox::from_raw()`
	FromRaw,
	/// Ownership handed to raw code with `into_raw()`
	IntoRaw,
	/// Borrowed by raw code with `as_ptr()`, the CBox stays the owner
	AsPtr,
	/// `&raw mut ptr` to `ptr.get_raw_ref_mut()`
	RawReference,
	/// Both sides are raw, so nothing is rewritten
	Unchanged,
}

impl RewriteOutcome {
	fn of_context_switch(inner: &PointerContext, outer: &PointerContext) -> Self {
		match (inner, outer) {
			(PointerContext::Box, PointerContext::Box) => Self::MoveBox,
			(PointerContext::Box, PointerContext::Raw { but_still_owned: true }) => Self::IntoRaw,
			(PointerContext::Box, PointerContext::Raw { but_still_owned: false }) => Self::AsPtr,
			(PointerContext::Box, PointerContext::RawRef { .. }) => Self::RawReference,
			(PointerContext::Raw { .. }, PointerContext::Box) => Self::FromRaw,
			_ => Self::Unchanged,
		}
	}
}

/// The rewrite of a single pointer interaction, together with what it did.
pub struct InteractionRewrite {
	pub rewrite: Option<Rewrite>,
	pub outcome: RewriteOutcome,
	/// Inner and outer pointer context, for interactions handled by the
	/// context rewriter rather than a targeted rewrite
	pub contexts: Option<(PointerContext, PointerContext)>,
	/// Whether a borrow (`as_ptr()`) was promoted to an ownership transfer
	/// (`into_raw()`), because the CBox would not outlive the raw pointer
	pub promoted: bool,
}

impl InteractionRewrite {
	fn targeted(rewrite: Rewrite, outcome: RewriteOutcome) -> Self {
		Self {
			rewrite: Some(rewrite),
			outcome,
			contexts: None,
			promoted: false,
		}
	}
}

pub fn get_rewrite_for_pointer_interaction(
	interaction: &NamedPointerInteraction,
	context: &ProjectContext,
	external_functions: &HashMap<vfs::FileId, HashSet<CFunctionDeclaration>>,
	all_sources: &Vec<NamedPointerSource>,
) -> InteractionRewrite {
	let effective_usage =
		get_effective_usage_type(interaction, context, external_functions).unwrap();

	// let (inner_context, outer_context) =
	// 	get_pointer_contexts_of_interaction(interaction, context, all_sources, all_c_functions);

	// Specific syntax for:
	// - deref element
	// - ref element
	// - deallocate
	// - reallocate
	//
	// Maybe also:
	// - nullcheck
	// -
	//
	let effective_cast = effective_usage.casts.last();

	match &effective_usage.variant {
		EffectiveUsageTypeVariant::DereferenceElement {
			element_index,
			is_mut,
		} => {
			// Deref element, so we should end up with `T`
			// Rewrite: `*(my_struct.my_ptr as *T).offset(X)`
			// To:		`*(my_struct.my_ptr).get_mut(X).unwrap()` (or `.get(..)`
			// when this dereference is only ever read, not written through -
			// `.get_mut` would fail to borrow when the base pointer is `*const`)
			let old_syntax_range = effective_usage.effective_usage.text_range();

			let base_pointer_text = interaction.value.syntax().text();

			let cast_text = if let Some(cast_type) = effective_cast {
				let element_type = get_element_type_of_pointer(cast_type, context).unwrap();
				format!(".cast_mut::<[{}]>()", element_type.syntax().text())
			} else {
				format!("")
			};

			let get_method = if *is_mut { "get_mut" } else { "get" };
			let indexing_text = if let Some(element_index) = element_index {
				format!(".{get_method}({} as usize).unwrap()", element_index.text())
			} else {
				format!(".{get_method}(0).unwrap()")
			};

			let new_text = format!("*({}){}{}", base_pointer_text, cast_text, indexing_text,);
			// TODO: Syntax moves of both the pointer and the index

			let pointer_move = SyntaxMove {
				old_syntax: interaction.value.syntax().text_range(),
				new_location: old_syntax_range.start() + TextSize::new(2),
			};
			let index_move = element_index.as_ref().map(|index| SyntaxMove {
				old_syntax: index.text_range(),
				new_location: old_syntax_range.start()
					+ base_pointer_text.len()
					+ TextSize::new(cast_text.len() as u32)
					+ TextSize::new(12),
			});

			InteractionRewrite::targeted(
				Rewrite {
					new_text,
					file_id: interaction.file_id,
					old_syntax_range,
					syntax_moves: if let Some(index_move) = index_move {
						vec![pointer_move, index_move]
					} else {
						vec![pointer_move]
					},
					_file_location: interaction._file_location.clone(),
				},
				RewriteOutcome::BoundsCheckedElement,
			)
		}
		EffectiveUsageTypeVariant::ReferenceElement { element_index } => {
			// Ref element, so we should end up with `&T`
			// Rewrite: `(my_struct.my_ptr).offset(X)`
			// To:		`(my_struct.my_ptr).get_mut_raw_unchecked(X).unwrap()`
			// Keep
			let old_syntax_range = effective_usage.effective_usage.text_range();

			let base_pointer_text = interaction.value.syntax().text();

			let cast_text = if let Some(cast_type) = effective_cast {
				let element_type = get_element_type_of_pointer(cast_type, context).unwrap();
				format!(".cast_mut::<[{}]>()", element_type.syntax().text())
			} else {
				format!("")
			};

			let indexing_text = format!(".get_ptr({} as usize).unwrap()", element_index.text());

			let new_text = format!("({}){}{}", base_pointer_text, cast_text, indexing_text);

			// New syntax moves of both the pointer and the index
			let pointer_move = SyntaxMove {
				old_syntax: interaction.value.syntax().text_range(),
				new_location: old_syntax_range.start() + TextSize::new(1),
			};
			let index_move = SyntaxMove {
				old_syntax: element_index.text_range(),
				new_location: old_syntax_range.start()
					+ base_pointer_text.len()
					+ TextSize::new(cast_text.len() as u32)
					+ TextSize::new(11),
			};

			InteractionRewrite::targeted(
				Rewrite {
					old_syntax_range,
					new_text,
					file_id: interaction.file_id,
					syntax_moves: vec![pointer_move, index_move],
					_file_location: interaction._file_location.clone(),
				},
				RewriteOutcome::BoundsCheckedPointer,
			)
		}
		EffectiveUsageTypeVariant::Reallocate {
			result_usage,
			new_size,
		} => {
			// TODO: Make test case where indirect `let x = realloc(y, _);` is used
			// Here, the indirect `let x =` should be ignored in favor of the direct
			// `_ = realloc(y, _);`

			// The effective pointer is being reallocated
			// Rewrite: `let _ = realloc(my_val, 20)`
			// To: 		`let _ = my_val.from_realloc(20).into_raw()`
			// Or: 		`let _ = my_val.from_realloc(20)`

			// First, we need to figure out how the realloc result is stored
			let outer_context_of_realloc =
				get_pointer_context_of_usage_variant(&result_usage.variant, all_sources);

			let realloc_result_should_be_raw = match outer_context_of_realloc {
				PointerContext::Box => false,
				PointerContext::Raw {
					but_still_owned: true,
				} => true,
				PointerContext::Raw {
					but_still_owned: false,
				} => {
					// This means we give an owned pointer to a piece of code that expects no ownership
					// This, although technically incorrect, is assumed to be correct due to Crowns
					// analysis. For now, simply log this happened, and continue
					eprintln!(
						"Warning: Passing owned Realloc result directly to non-owning context at: {}",
						interaction._file_location
					);
					true
				}
				_ => unreachable!(),
			};

			let old_syntax_range = result_usage.effective_pointer.text_range();

			let to_raw_suffix = if realloc_result_should_be_raw {
				format!(".into_raw()")
			} else {
				format!("")
			};

			let new_text = format!(
				"({}).from_realloc({}){}",
				interaction.value.syntax().text(),
				new_size.text(),
				to_raw_suffix,
			);

			let pointer_move = SyntaxMove {
				old_syntax: effective_usage.effective_pointer.text_range(),
				new_location: old_syntax_range.start() + TextSize::new(1),
			};
			let size_move = SyntaxMove {
				old_syntax: new_size.text_range(),
				new_location: old_syntax_range.start()
					+ interaction.value.syntax().text_range().len()
					+ TextSize::new(16),
			};

			InteractionRewrite::targeted(
				Rewrite {
					old_syntax_range,
					new_text,
					file_id: interaction.file_id,
					syntax_moves: vec![pointer_move, size_move],
					_file_location: interaction._file_location.clone(),
				},
				if realloc_result_should_be_raw {
					RewriteOutcome::ReallocateIntoRaw
				} else {
					RewriteOutcome::Reallocate
				},
			)
		}
		EffectiveUsageTypeVariant::Unused => {
			// A discarded value needs no rewrite: a discarded CBox is dropped
			// right away, which deallocates it
			InteractionRewrite {
				rewrite: None,
				outcome: RewriteOutcome::Unchanged,
				contexts: None,
				promoted: false,
			}
		}
		EffectiveUsageTypeVariant::Deallocate => {
			// Rewrite: `free(my_val as *mut libc::c_void)`
			// To:		`drop((my_val).take())`
			let old_syntax_range = effective_usage.effective_usage.text_range();
			let new_text = format!("drop(({}).take())", interaction.value.syntax().text());

			let pointer_move = SyntaxMove {
				old_syntax: interaction.value.syntax().text_range(),
				new_location: old_syntax_range.start() + TextSize::new(6),
			};

			InteractionRewrite::targeted(
				Rewrite {
					old_syntax_range,
					new_text,
					file_id: interaction.file_id,
					syntax_moves: vec![pointer_move],
					_file_location: interaction._file_location.clone(),
				},
				RewriteOutcome::Deallocate,
			)
		}
		x => {
			// For most variants routed through here, the pointer *value*
			// itself is what gets type-converted, with everything around it
			// left untouched - `effective_pointer` (just the value, no
			// wrapping operator) is the right span to replace.
			//
			// `ReferenceRawArray` is different: `&raw mut deflatedata` (or
			// `&raw`/`&mut *`/`&*`) is itself the syntax being consumed and
			// folded into `(deflatedata).get_raw_ref_mut()` - the operator
			// is not "useless wrapper syntax" like a cast or paren, it's
			// exactly what this rewrite exists to remove. Using
			// `effective_pointer` here would leave the original `&raw mut`
			// sitting untouched in front of the rewritten call.
			let old_syntax_range = if matches!(
				effective_usage.variant,
				EffectiveUsageTypeVariant::ReferenceRawArray { .. }
			) {
				effective_usage.effective_usage.text_range()
			} else {
				effective_usage.effective_pointer.text_range()
			};

			let (inner_context, mut outer_context) = if interaction.is_direct_usage {
				let inner = get_pointer_context_of_source(&interaction.source);
				let outer =
					get_pointer_context_of_usage_variant(&effective_usage.variant, all_sources);
				(inner, outer)
			} else {
				let inner = PointerContext::Raw {
					but_still_owned: true,
				};
				let outer = get_pointer_context_of_source(&interaction.source);
				(inner, outer)
			};

			// Exception:
			// When the inner context is Box and the outer context is Raw-non-owning,
			// and the borrowed value lives longer than the box (which is the case
			// for return statements and function calls), we still need to consider
			// the outer context as taking ownership
			let owner_dies_first = match &interaction.source.location {
				PointerSourceLocation::FnResult { .. } => true,
				PointerSourceLocation::FnParam { .. } | PointerSourceLocation::FnBody { .. } => {
					effective_usage.variant.is_returned()
				}
				PointerSourceLocation::StructField { .. } => false,
			};
			let promoted = interaction.is_direct_usage
				&& owner_dies_first
				&& matches!(inner_context, PointerContext::Box)
				&& matches!(
					outer_context,
					PointerContext::Raw {
						but_still_owned: false
					}
				);
			if promoted {
				eprintln!(
					"Promoting borrow of dying CBox to ownership transfer isntead at {:?}",
					interaction._file_location
				);
				outer_context = PointerContext::Raw {
					but_still_owned: true,
				}
			}

			let context_switch = get_new_text_for_context_switch(
				&effective_usage.effective_pointer,
				interaction.value.syntax(),
				&inner_context,
				&outer_context,
				effective_usage.casts.last(),
				context,
			);

			let outcome = match &context_switch {
				Some(_) => RewriteOutcome::of_context_switch(&inner_context, &outer_context),
				None => RewriteOutcome::Unchanged,
			};
			let rewrite = context_switch.map(|context_switch| Rewrite {
				file_id: interaction.file_id,
				old_syntax_range,
				new_text: context_switch.new_text,
				syntax_moves: if let Some(syntax_move) = context_switch.syntax_move {
					vec![syntax_move]
				} else {
					vec![]
				},
				_file_location: interaction._file_location.clone(),
			});

			InteractionRewrite {
				rewrite,
				outcome,
				contexts: Some((inner_context, outer_context)),
				promoted,
			}
		}
	}
}

struct ContextSwitch {
	new_text: String,
	syntax_move: Option<SyntaxMove>,
}

/// Figure out, given a `value`, what context transition should happen.
/// In code like `let x = value`, where `value` is a raw pointer and `x` is a CBox,
/// we should apply the `CBox::from_raw()` to value, resulting in
/// `let x = CBox::from_raw(value);`
fn get_new_text_for_context_switch(
	effective_pointer: &syntax::SyntaxNode,
	direct_pointer: &syntax::SyntaxNode,
	inner_context: &PointerContext,
	outer_context: &PointerContext,
	effective_cast: Option<&ast::Type>,
	context: &ProjectContext,
) -> Option<ContextSwitch> {
	match (inner_context, outer_context) {
		(PointerContext::Box, PointerContext::Box) => {
			// Means to move ownership, use normal assignment
			if let Some(cast_type) = effective_cast {
				let pointer_element = get_element_type_of_pointer(cast_type, context).unwrap();
				let new_text = format!(
					"({}).cast_mut::<[{}]>().take()",
					direct_pointer.text(),
					pointer_element.syntax().text()
				);

				let syntax_move = Some(SyntaxMove {
					old_syntax: effective_pointer.text_range(),
					new_location: effective_pointer.text_range().start() + TextSize::new(1),
				});

				Some(ContextSwitch {
					new_text,
					syntax_move,
				})
			} else {
				let new_text = format!("({}).take()", direct_pointer.text());
				let syntax_move = Some(SyntaxMove {
					old_syntax: effective_pointer.text_range(),
					new_location: effective_pointer.text_range().start() + TextSize::new(1),
				});
				Some(ContextSwitch {
					new_text,
					syntax_move,
				})
			}
		}
		(
			PointerContext::Box,
			PointerContext::Raw {
				but_still_owned: true,
			},
		) => {
			// Take ownership and convert to raw
			let new_text = if let Some(cast_type) = effective_cast {
				format!(
					"({}).into_raw() as {}",
					direct_pointer.text(),
					cast_type.syntax().text()
				)
			} else {
				format!("({}).into_raw()", direct_pointer.text())
			};

			let syntax_move = Some(SyntaxMove {
				old_syntax: effective_pointer.text_range(),
				new_location: effective_pointer.text_range().start() + TextSize::new(1),
			});

			Some(ContextSwitch {
				new_text,
				syntax_move,
			})
		}
		(
			PointerContext::Box,
			PointerContext::Raw {
				but_still_owned: false,
			},
		) => {
			// Only get a reference pointer to inner value
			let new_text = if let Some(cast_type) = effective_cast {
				format!(
					"({}).as_ptr() as {}",
					direct_pointer.text(),
					cast_type.syntax().text()
				)
			} else {
				format!("({}).as_ptr()", direct_pointer.text())
			};

			let syntax_move = Some(SyntaxMove {
				old_syntax: effective_pointer.text_range(),
				new_location: effective_pointer.text_range().start() + TextSize::new(1),
			});
			Some(ContextSwitch {
				new_text,
				syntax_move,
			})
		}
		(PointerContext::Box, PointerContext::RawRef { .. }) => {
			// TODO: Check mutability still compiles
			// `&raw mut value` to `value.get_raw_ref_mut()`

			let new_text = if let Some(cast_type) = effective_cast {
				todo!()
			} else {
				format!("({}).get_raw_ref_mut()", direct_pointer.text())
			};

			let syntax_move = Some(SyntaxMove {
				old_syntax: effective_pointer.text_range(),
				new_location: effective_pointer.text_range().start() + TextSize::new(1),
			});

			Some(ContextSwitch {
				new_text,
				syntax_move,
			})
		}
		(
			PointerContext::Raw {
				but_still_owned: true,
			},
			PointerContext::Box,
		) => {
			let new_text = if let Some(cast_type) = effective_cast {
				format!(
					"CBox::from_raw({} as {})",
					direct_pointer.text(),
					cast_type.syntax().text()
				)
			} else {
				format!("CBox::from_raw({})", direct_pointer.text())
			};

			let syntax_move = Some(SyntaxMove {
				old_syntax: effective_pointer.text_range(),
				new_location: effective_pointer.text_range().start() + TextSize::new(15),
			});
			Some(ContextSwitch {
				new_text,
				syntax_move,
			})
		}
		(
			PointerContext::Raw {
				but_still_owned: false,
			},
			PointerContext::Box,
		) => {
			todo!("SHould be reachable, actually");
			unreachable!()
		}
		(
			PointerContext::Raw {
				but_still_owned: false,
			},
			PointerContext::Raw {
				but_still_owned: true,
			},
		) => unreachable!(),
		(PointerContext::Raw { .. }, PointerContext::Raw { .. }) => None,
		(PointerContext::Raw { .. }, PointerContext::RawRef { .. }) => todo!(),
		(PointerContext::RawRef { .. }, _) => unreachable!(),
	}
}
