use std::collections::{HashMap, HashSet};

use syntax::{AstNode, ast};

use crate::{
	ProjectContext,
	effective_usage_type::get_effective_usage_type,
	extern_function::CFunctionDeclaration,
	pointer_context::{
		PointerContext, get_pointer_context_of_source, get_pointer_contexts_of_interaction,
	},
	pointer_interactions::NamedPointerInteraction,
	pointer_source::{NamedPointerSource, PointerSourceLocation, get_element_type_of_pointer},
};

#[derive(Debug, Clone)]
pub struct ContextSwitchRewritePlan {
	pub direct_pointer: syntax::SyntaxNode,
	pub effective_pointer: syntax::SyntaxNode,
	pub current_context: PointerContext,
	pub casts: Vec<ast::Type>,
	pub target_context: PointerContext,
	pub file_id: vfs::FileId,
	pub element_type: Option<String>,
	pub _file_location: String,
}

#[derive(Debug, Clone)]
pub struct TypeRewritePlan {
	pub old_type: syntax::SyntaxNode,
	pub element_type: syntax::SyntaxNode,
	pub new_pointer_context: PointerContext,
	pub file_id: vfs::FileId,
	pub _file_location: String,
}

pub fn plan_rewrite_for_context_switch(
	usage: &NamedPointerInteraction,
	context: &ProjectContext,
	all_sources: &Vec<NamedPointerSource>,
	all_c_functions: &HashMap<vfs::FileId, HashSet<CFunctionDeclaration>>,
) -> ContextSwitchRewritePlan {
	let file_id = usage.file_id;
	let element_type = None;
	let _file_location = usage._file_location.clone();

	let usage_type = get_effective_usage_type(usage, context, all_c_functions).unwrap();

	let casts = usage_type.casts;

	let mut direct_pointer = usage.value.syntax().clone();
	let mut effective_pointer = usage_type.classification_pointer.clone();

	// One exception
	if usage_type.variant.is_raw_reference() {
		// Then suddenly the direct pointer becomes the effective pointer
		// And the effective pointer becomes the entire usage
		direct_pointer = effective_pointer;
		effective_pointer = usage_type.effective_usage;
	}

	let (current_context, target_context) =
		get_pointer_contexts_of_interaction(usage, context, all_sources, all_c_functions);

	let switch_plan = ContextSwitchRewritePlan {
		direct_pointer,
		effective_pointer,
		current_context,
		casts,
		target_context,
		file_id,
		element_type,
		_file_location,
	};

	// // If the source is a function result, and returns a CBox, we must always take its ownership.
	// // This means we cannot do `create_owned().as_ptr()`, as that would prematurely drop the box,
	// // leaving the used pointer inaccessible to free.
	// // Similar for when we return a CBox as a raw pointer: By the time it is at the caller,
	// // it would have already been deallocated if we did not transfer ownership
	// let must_transfer_ownership = if source_pointer_context.is_box() {
	// 	if let Some(source) = &usage.of_source {
	// 		source.is_function_result() || usage_type.variant.is_returned()
	// 	} else {
	// 		false
	// 	}
	// } else {
	// 	false
	// };
	// if must_transfer_ownership && switch_plan.target_context.is_raw() {
	// 	switch_plan.target_context = PointerContext::Raw {
	// 		but_still_owned: true,
	// 	}
	// }

	switch_plan
}

pub fn plan_rewrite_for_pointer_source(
	source: &NamedPointerSource,
	context: &ProjectContext,
) -> Option<TypeRewritePlan> {
	let pointer_context = get_pointer_context_of_source(source);

	let file_id = source.file_id;

	match &source.location {
		PointerSourceLocation::FnResult {
			pointer_type,
			element_type,
			..
		} => Some(TypeRewritePlan {
			old_type: pointer_type.syntax().clone(),
			element_type: element_type.syntax().clone(),
			new_pointer_context: pointer_context,
			file_id,
			_file_location: source._file_location.clone(),
		}),
		PointerSourceLocation::FnParam {
			pointer_type,
			element_type,
			..
		} => Some(TypeRewritePlan {
			old_type: pointer_type.syntax().clone(),
			element_type: element_type.syntax().clone(),
			new_pointer_context: pointer_context,
			file_id,
			_file_location: source._file_location.clone(),
		}),
		PointerSourceLocation::FnBody { let_statement, .. } => {
			// Only rewrite when the type is explicitly present
			if let Some(original_type) = let_statement.ty() {
				let element_type = get_element_type_of_pointer(&original_type, context).unwrap();
				Some(TypeRewritePlan {
					old_type: original_type.syntax().clone(),
					element_type: element_type.syntax().clone(),
					new_pointer_context: pointer_context,
					file_id,
					_file_location: source._file_location.clone(),
				})
			} else {
				None
			}
		}
		PointerSourceLocation::StructField {
			pointer_type,
			element_type,
			..
		} => Some(TypeRewritePlan {
			old_type: pointer_type.syntax().clone(),
			element_type: element_type.syntax().clone(),
			new_pointer_context: pointer_context,
			file_id,
			_file_location: source._file_location.clone(),
		}),
	}
}
