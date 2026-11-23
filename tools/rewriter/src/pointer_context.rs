use std::collections::{HashMap, HashSet};

use crate::{
	ProjectContext,
	effective_usage_type::{
		EffectiveUsageTypeVariant, PointerStoreDestination, get_effective_usage_type,
	},
	extern_function::CFunctionDeclaration,
	pointer_interactions::NamedPointerInteraction,
	pointer_source::{NamedPointerSource, PointerSourceLocation},
};

#[derive(Debug, Clone)]
pub enum PointerContext {
	/// CBox<T>
	Box,
	// Mut is not possible in our current rewrite solution
	// /// Option<&mut T>
	// Mut,
	/// *T
	Raw { but_still_owned: bool },
	/// A pointer to the internal pointer
	RawRef { is_mut: bool },
}

impl PointerContext {
	pub fn is_raw(&self) -> bool {
		matches!(self, Self::Raw { .. })
	}
	pub fn str(&self) -> String {
		match self {
			Self::Box => format!("Box"),
			Self::Raw { but_still_owned } => {
				format!("Raw({})", if *but_still_owned { "own" } else { "borrow" })
			}
			Self::RawRef { is_mut } => format!("RawRef({})", if *is_mut { "mut" } else { "const" }),
		}
	}
}

/// Gets the internal and external pointer context
/// - Internal: The pointer context of the value itself
/// - External: The pointer context the surrounding code expects
/// If mismatch, the rewriter has to apply syntax changes
pub fn get_pointer_contexts_of_interaction(
	interaction: &NamedPointerInteraction,
	context: &ProjectContext,
	all_sources: &Vec<NamedPointerSource>,
	all_c_functions: &HashMap<vfs::FileId, HashSet<CFunctionDeclaration>>,
) -> (PointerContext, PointerContext) {
	if interaction.is_direct_usage {
		let inner = get_pointer_context_of_source(&interaction.source);

		let interaction_type = get_effective_usage_type(interaction, context, all_c_functions).unwrap();
		let outer = get_pointer_context_of_usage_variant(&interaction_type.variant, all_sources);
		(inner, outer)
	} else {
		let inner = PointerContext::Raw {
			but_still_owned: true,
		};
		let outer = get_pointer_context_of_source(&interaction.source);
		(inner, outer)
	}
}

pub fn get_pointer_context_of_usage_variant(
	variant: &EffectiveUsageTypeVariant,
	all_sources: &Vec<NamedPointerSource>,
) -> PointerContext {
	match variant {
		EffectiveUsageTypeVariant::StoreElsewhere(pointer_store_destination) => {
			get_pointer_context_of_store_destination(pointer_store_destination, all_sources)
		}
		EffectiveUsageTypeVariant::PassToFunction {
			function,
			as_nth_param,
		} => {
			if let Some(function) = function {
				get_pointer_context_of_param(*as_nth_param, function, all_sources)
			} else {
				PointerContext::Raw {
					but_still_owned: false,
				}
			}
		}
		EffectiveUsageTypeVariant::Reallocate { .. } => PointerContext::Raw {
			but_still_owned: true,
		},
		EffectiveUsageTypeVariant::Deallocate => PointerContext::Raw {
			but_still_owned: true,
		},
		EffectiveUsageTypeVariant::Returned { function, .. } => {
			get_pointer_context_of_return_value(function, all_sources)
		}
		EffectiveUsageTypeVariant::ReferenceRawArray { is_mut, .. } => {
			PointerContext::RawRef { is_mut: *is_mut }
		}
		_ => PointerContext::Raw {
			but_still_owned: false,
		},
	}
}

fn get_pointer_context_of_param(
	nth_param: u32,
	function: &hir::Function,
	all_sources: &Vec<NamedPointerSource>,
) -> PointerContext {
	for source in all_sources {
		if let PointerSourceLocation::FnParam {
			param_index,
			function: other_fn,
			..
		} = &source.location
		{
			// This source is a parameter
			if *param_index as u32 == nth_param && function == other_fn {
				// This is the same parameter as we were asking about
				return get_pointer_context_of_source(source);
			}
		}
	}
	// If unfindable, assume raw
	PointerContext::Raw {
		but_still_owned: false,
	}
}

pub fn get_pointer_context_of_source(source: &NamedPointerSource) -> PointerContext {
	if source.is_output_param() {
		return PointerContext::Raw { but_still_owned: false }
	}
	if source.is_owned && source.is_fat {
		return PointerContext::Box
	}
	return PointerContext::Raw { but_still_owned: source.is_owned }
}

fn get_pointer_context_of_return_value(
	function: &hir::Function,
	all_sources: &Vec<NamedPointerSource>,
) -> PointerContext {
	// First, find matching source
	let Some(source) = all_sources.iter().find(|source| {
		if let PointerSourceLocation::FnResult {
			function: other, ..
		} = &source.location
		{
			other == function
		} else {
			false
		}
	}) else {
		eprintln!("Failed to find pointer context of function");
		return PointerContext::Raw {
			but_still_owned: false,
		};
	};

	get_pointer_context_of_source(source)
}

fn get_pointer_context_of_store_destination(
	destination: &PointerStoreDestination,
	all_sources: &Vec<NamedPointerSource>,
) -> PointerContext {
	match destination {
		PointerStoreDestination::NewVariable { local, .. } => {
			get_pointer_context_of_variable(local, all_sources)
		}
		PointerStoreDestination::ExistingVariable { local, .. } => {
			get_pointer_context_of_variable(local, all_sources)
		}
		PointerStoreDestination::InStructField { field, .. } => {
			get_pointer_context_of_struct_field(field, all_sources)
		}
		PointerStoreDestination::InCollection {
			collection_usage, ..
		} => {
			// Stack arrays are completely excluded from Crown's ownership
			// analysis, meaning all stack arrays will not contain raw pointers
			// rewritten as boxes. Therefore, the target context must be raw here.
			// For owned raw or non-owned raw, we continue checking
			get_pointer_context_of_usage_variant(&collection_usage.variant, all_sources)
		}
		PointerStoreDestination::SomeOtherStorage { syntax } => {
			eprint!("TODO: Other storage: \n{}", syntax.text().to_string());
			PointerContext::Raw {
				but_still_owned: true,
			}
		}
		PointerStoreDestination::InStatic { .. } => todo!(),
	}
}

fn get_pointer_context_of_variable(
	local: &hir::Local,
	all_sources: &Vec<NamedPointerSource>,
) -> PointerContext {
	let source = all_sources
		.iter()
		.find(|source| match &source.location {
			PointerSourceLocation::FnParam { param, .. } => param.as_ref() == Some(local),
			PointerSourceLocation::FnBody { variable, .. } => variable == local,
			_ => false,
		})
		.expect("Failed to find source of {local:?}");

	get_pointer_context_of_source(source)
}

fn get_pointer_context_of_struct_field(
	field: &hir::Field,
	all_sources: &Vec<NamedPointerSource>,
) -> PointerContext {
	let source = all_sources
		.iter()
		.find(|source| {
			if let PointerSourceLocation::StructField { field: other, .. } = &source.location {
				field == other
			} else {
				false
			}
		})
		.unwrap();

	get_pointer_context_of_source(source)
}
