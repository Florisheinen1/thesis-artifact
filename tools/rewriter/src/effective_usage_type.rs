use std::collections::{HashMap, HashSet};

use hir::ModuleDef;
use syntax::{
	AstNode,
	ast::{self, HasArgList},
};

use crate::{
	ProjectContext,
	extern_function::{CFunction, CFunctionDeclaration},
	pointer_interactions::{NamedPointerInteraction, resolve_field_with_fallback},
};

pub struct EffectiveUsageType {
	/// The syntactically-largest pointer value that directly is used in the
	/// found usage. Can span multiple control-flow syntax nodes, such as if and
	/// tail statements
	pub classification_pointer: syntax::SyntaxNode,
	/// The syntactically largest pointer value that does not contain
	/// control-flow-like syntax. Use this for rewriting this value.
	pub effective_pointer: syntax::SyntaxNode,
	pub effective_usage: syntax::SyntaxNode,
	pub casts: Vec<ast::Type>,
	pub variant: EffectiveUsageTypeVariant,
}

pub enum EffectiveUsageTypeVariant {
	// /// `my_ptr = ..`
	// SetToSomeValue(NewPointerValue),
	/// `.. = my_ptr`
	StoreElsewhere(PointerStoreDestination),
	/// A reference into this raw pointer's target: `&raw mut my_ptr`/`&raw
	/// my_ptr`, or the equivalent safe-reference spelling C2Rust also emits,
	/// `&mut *my_ptr`/`&*my_ptr` (and the same with an `.offset(..)` in
	/// between, e.g. `&*my_ptr.offset(i)` for C's `&arr[i]`).
	ReferenceRawArray {
		is_mut: bool,
		is_raw: bool,
	},
	/// `my_arr.is_null()`
	NullCheck,
	/// `*my_arr.offset(..)`
	DereferenceElement {
		element_index: Option<syntax::SyntaxNode>,
		is_mut: bool,
	},
	/// `my_arr.offset(..)`
	ReferenceElement {
		element_index: syntax::SyntaxNode,
	},
	/// `my_arr.addr()`
	AddressOfPointer,
	/// `my_arr < ..`
	ComparePointer,
	/// `my_arr.offset_from(..)`
	OffsetFrom {
		other: syntax::SyntaxNode,
	},
	/// free(..)
	Deallocate,
	/// The given pointer was passed to the realloc function
	Reallocate {
		/// Is the realloc result stored, passed to another function, or returned, etc.?
		result_usage: Box<EffectiveUsageType>,
		new_size: syntax::SyntaxNode,
	},
	/// A pointer is passed to any function that is not `free` or `realloc`
	PassToFunction {
		function: Option<hir::Function>,
		as_nth_param: u32,
	},
	/// The value is stored at the function caller's place
	Returned {
		function: hir::Function,
	},
	Unused,
}

impl EffectiveUsageTypeVariant {
	pub fn str(&self) -> String {
		match self {
			// Self::SetToSomeValue { .. } => format!("SetToSomeValue"),
			Self::StoreElsewhere { .. } => format!("StoreElsewhere"),
			Self::ReferenceRawArray { .. } => format!("ReferenceRawArray"),
			Self::NullCheck { .. } => format!("NullCheck"),
			Self::DereferenceElement { .. } => format!("DereferenceElement"),
			Self::ReferenceElement { .. } => format!("ReferenceElement"),
			Self::AddressOfPointer { .. } => format!("AddressOfPointer"),
			Self::ComparePointer { .. } => format!("ComparePointer"),
			Self::OffsetFrom { .. } => format!("OffsetFrom"),
			Self::Deallocate { .. } => format!("Deallocate"),
			Self::Reallocate { .. } => format!("Reallocate"),
			Self::PassToFunction { .. } => format!("PassToFunction"),
			Self::Returned { .. } => format!("Returned"),
			Self::Unused => format!("Unused"),
		}
	}
	pub fn is_returned(&self) -> bool {
		matches!(self, Self::Returned { .. })
	}
	pub fn is_raw_reference(&self) -> bool {
		matches!(self, Self::ReferenceRawArray { .. })
	}
}

/// `.. = my_arr;`
pub enum PointerStoreDestination {
	/// New variable introduced in let-statement
	NewVariable {
		local: hir::Local,
	},
	/// Existing local variable, from either previous
	/// let statement or function parameter
	ExistingVariable {
		local: hir::Local,
		variable: syntax::SyntaxNode,
	},
	/// Store in a static variable
	InStatic {
		item: hir::Static,
		syntax: syntax::SyntaxNode,
	},
	InStructField {
		field: hir::Field,
		syntax: syntax::SyntaxNode,
	},
	/// For [.., my_arr, ..] or (.., my_arr, ..)
	InCollection {
		collection: syntax::SyntaxNode,
		collection_usage: Box<EffectiveUsageClassification>,
	},
	SomeOtherStorage {
		syntax: syntax::SyntaxNode,
	},
}
impl PointerStoreDestination {
	pub fn str(&self) -> String {
		match self {
			PointerStoreDestination::NewVariable { .. } => format!("NewVariable()"),
			PointerStoreDestination::ExistingVariable { variable, .. } => {
				format!("ExistingVariable({})", variable.text().to_string())
			}
			PointerStoreDestination::InStructField { syntax, .. } => {
				format!("InStructField({})", syntax.text().to_string())
			}
			PointerStoreDestination::InCollection { collection, .. } => {
				format!("InCollection({})", collection.text().to_string())
			}
			PointerStoreDestination::SomeOtherStorage { syntax } => {
				format!("SomeOtherStorage({})", syntax.text().to_string())
			}
			PointerStoreDestination::InStatic { syntax, .. } => {
				format!("SomeOtherStorage({})", syntax.text().to_string())
			}
		}
	}
}

pub fn get_effective_usage_type(
	usage: &NamedPointerInteraction,
	context: &ProjectContext,
	external_functions: &HashMap<vfs::FileId, HashSet<CFunctionDeclaration>>,
) -> Result<EffectiveUsageType, String> {
	let mut extern_function_map: HashMap<&hir::Function, &CFunctionDeclaration> = HashMap::new();

	external_functions.get(&usage.file_id).inspect(|decls| {
		for decl in *decls {
			extern_function_map.insert(&decl.function, decl);
		}
	});

	let (effective_pointer, skipped_casts) =
		unwrap_useless_parent_syntax(usage.value.syntax(), vec![]);

	let (classification_pointer, skipped_casts) =
		unwrap_all_useless_and_flow_parent_syntax(&effective_pointer, skipped_casts);

	assert_eq!(
		classification_pointer,
		unwrap_useless_parent_syntax(&classification_pointer, vec![]).0,
		"Did not expect casts after control-flow syntax"
	);

	let usage_classification = classify_effective_usage_type(
		&classification_pointer,
		usage.containing_function,
		context,
		&extern_function_map,
	)?;

	Ok(EffectiveUsageType {
		classification_pointer,
		effective_pointer,
		effective_usage: usage_classification.entire_usage,
		casts: skipped_casts,
		variant: usage_classification.variant,
	})
}

/// Removes all 'useless' children of given expression until 'non-useless'
/// syntax is found.
/// Useless here means: casts, parenthesis, block statements, etc: Syntax that
/// does not directly change data.
/// Returns the same expression if it contains no useless syntax.
pub fn unwrap_useless_child_syntax(
	pointer_value: &ast::Expr,
	mut skipped_casts: Vec<ast::Type>,
) -> (ast::Expr, Vec<ast::Type>) {
	match pointer_value {
		ast::Expr::CastExpr(cast_expr) => {
			let cast_type = cast_expr.ty().unwrap();
			skipped_casts.push(cast_type);
			let inner = cast_expr.expr().unwrap();
			unwrap_useless_child_syntax(&inner, skipped_casts)
		}
		ast::Expr::ParenExpr(paren_expr) => {
			let inner = paren_expr.expr().unwrap();
			unwrap_useless_child_syntax(&inner, skipped_casts)
		}
		ast::Expr::BlockExpr(block_expr) => {
			let tail = block_expr.tail_expr().unwrap();
			unwrap_useless_child_syntax(&tail, skipped_casts)
		}
		_ => (pointer_value.clone(), skipped_casts),
	}
}

/// Repeatedly unwraps both control flow and useless syntax until it can no more.
pub fn unwrap_all_useless_and_flow_parent_syntax(
	pointer_value: &syntax::SyntaxNode,
	mut skipped_casts: Vec<ast::Type>,
) -> (syntax::SyntaxNode, Vec<ast::Type>) {
	let (new_pointer_value, new_skipped_cast) =
		unwrap_useless_parent_syntax(pointer_value, skipped_casts);
	let new_new_pointer_value = unwrap_control_flow_parent_syntax(&new_pointer_value);

	if &new_new_pointer_value != pointer_value {
		return unwrap_all_useless_and_flow_parent_syntax(&new_new_pointer_value, new_skipped_cast);
	} else {
		(new_new_pointer_value, new_skipped_cast)
	}
}

/// Recursively iterates through all ancestors of the given syntax node, as long
/// as the outer expression is one that does not belong to a specific usage type
/// Skips e.g.:
/// - casts
/// - parentheses
///
/// Does not skip control flow syntax such as if blocks
///
/// Collects all casts that were made on the way.
/// Returns the syntax node that is not a useless syntax, and directly used some
/// way. Returns the given syntax if the given is not useless, in which case the
/// parent will be directly classifiable in the usage categories
pub fn unwrap_useless_parent_syntax(
	pointer_value: &syntax::SyntaxNode,
	mut skipped_casts: Vec<ast::Type>,
) -> (syntax::SyntaxNode, Vec<ast::Type>) {
	let parent = pointer_value.parent().unwrap();

	// Skip casts
	if let Some(cast_expr) = ast::CastExpr::cast(parent.clone()) {
		let cast_type = cast_expr.ty().unwrap();
		skipped_casts.push(cast_type);

		return unwrap_useless_parent_syntax(&parent, skipped_casts);
	};
	// Skip parentheses
	if ast::ParenExpr::cast(parent.clone()).is_some() {
		return unwrap_useless_parent_syntax(&parent, skipped_casts);
	};
	(pointer_value.clone(), skipped_casts)
}

pub fn unwrap_control_flow_parent_syntax(pointer_value: &syntax::SyntaxNode) -> syntax::SyntaxNode {
	let parent = pointer_value.parent().unwrap();
	// Skip tail expressions
	if let Some(statement_list) = ast::StmtList::cast(parent.clone()) {
		assert!(
			statement_list.tail_expr().unwrap().syntax() == pointer_value,
			"A list of statements without a tail expression cannot be 'used' as a pointer"
		);
		return unwrap_control_flow_parent_syntax(&parent);
	};
	// Skip more tail expressions
	if let Some(block_expression) = ast::BlockExpr::cast(parent.clone()) {
		// `pointer_value` here is the StmtList itself, propagated by the
		// "skip tail expressions" branch above (which already verified it
		// used to be that StmtList's tail expression, before being replaced
		// by the StmtList node itself). `BlockExpr::tail_expr()` unwraps
		// straight through to the inner expression, so it can never equal a
		// StmtList — check structural identity with `stmt_list()` instead.
		assert!(
			block_expression.stmt_list().as_ref().map(|s| s.syntax()) == Some(pointer_value),
			"Block's stmt_list did not match the propagated pointer_value:\n{}\nWith pointer value:\n{}",
			block_expression.syntax().text().to_string(),
			pointer_value.text().to_string(),
		);

		return unwrap_control_flow_parent_syntax(&parent);
	};
	// Skip if expressions
	if ast::IfExpr::cast(parent.clone()).is_some() {
		return unwrap_control_flow_parent_syntax(&parent);
	};
	pointer_value.clone()
}

pub struct EffectiveUsageClassification {
	pub entire_usage: syntax::SyntaxNode,
	pub variant: EffectiveUsageTypeVariant,
}

/// Classifies how the given pointer is used.
/// Returns the syntax of the entire usage, and the usage type.
/// This means a *my_arr.offset(6) is not just a method call of `my_arr`, but
/// a DereferenceElement, and a my_arr.offset(6) is a ReferenceElement.
/// Achieves this by recursively walking up the list of ancestors.
pub fn classify_effective_usage_type(
	pointer_value: &syntax::SyntaxNode,
	// `None` when this usage lives outside any function body, e.g. a
	// struct-literal field initializer inside a top-level `static`/`const`
	// item. Only some branches below need a concrete function; those `expect`
	// one, which is safe since Rust syntax guarantees a `let`/`return`/call
	// argument can't exist outside of a function body in the first place.
	parent_function: Option<hir::Function>,
	context: &ProjectContext,
	external_functions: &HashMap<&hir::Function, &CFunctionDeclaration>,
) -> Result<EffectiveUsageClassification, String> {
	let parent = pointer_value.parent().unwrap();

	if let Some(record_field) = ast::RecordExprField::cast(parent.clone()) {
		// `SomeStruct { arr: <pointer_value> }` — a struct-literal field
		// initializer. Same shape as the `FieldExpr` case inside the
		// assignment branch below (a value flowing into a struct field),
		// just without an actual assignment expression to hang off of.
		let Some((field, _local, _ty)) = context.rust_semantics.resolve_record_field(&record_field)
		else {
			return Err(format!(
				"Failed to resolve record field:\n{}",
				record_field.syntax().text()
			));
		};
		return Ok(EffectiveUsageClassification {
			entire_usage: record_field.syntax().clone(),
			variant: EffectiveUsageTypeVariant::StoreElsewhere(
				PointerStoreDestination::InStructField {
					field,
					syntax: record_field.syntax().clone(),
				},
			),
		});
	}

	if let Some(binary_expression) = ast::BinExpr::cast(parent.clone()) {
		match binary_expression.op_kind().unwrap() {
			ast::BinaryOp::Assignment { .. } => {
				let lhs = binary_expression.lhs().unwrap();
				let rhs = binary_expression.rhs().unwrap();
				if lhs.syntax() == pointer_value {
					return Err(format!(
						"Should not receive usage of a LHS of an assignment"
					));
				}
				assert!(
					rhs.syntax() == pointer_value,
					"If x is in `a = b`, and it ain't `a`, x must be `b`"
				);
				// We know we store our pointer somewhere. Now, figure out where

				let storage_variant = match &lhs {
					ast::Expr::PathExpr(path_expr) => {
						let path = path_expr.path().unwrap();
						let local = context.rust_semantics.resolve_path(&path).unwrap();

						if let hir::PathResolution::Local(local) = local {
							PointerStoreDestination::ExistingVariable {
								local,
								variable: lhs.syntax().clone(),
							}
						} else if let hir::PathResolution::Def(hir::ModuleDef::Static(
							static_item,
						)) = local
						{
							PointerStoreDestination::InStatic {
								item: static_item,
								syntax: lhs.syntax().clone(),
							}
						} else {
							todo!("Unresolved storage path expression")
						}
					}
					ast::Expr::FieldExpr(field_expr) => {
						let Some(field) = resolve_field_with_fallback(field_expr, context) else {
							return Err(format!(
								"Failed to resolve field of assignment LHS: {}",
								field_expr.syntax().text()
							));
						};
						PointerStoreDestination::InStructField {
							field,
							syntax: field_expr.syntax().clone(),
						}
					}
					_ => PointerStoreDestination::SomeOtherStorage {
						syntax: lhs.syntax().clone(),
					},
				};

				return Ok(EffectiveUsageClassification {
					entire_usage: binary_expression.syntax().clone(),
					variant: EffectiveUsageTypeVariant::StoreElsewhere(storage_variant),
				});
			}
			ast::BinaryOp::CmpOp(_comparator) => {
				return Ok(EffectiveUsageClassification {
					entire_usage: binary_expression.syntax().clone(),
					variant: EffectiveUsageTypeVariant::ComparePointer,
				});
			}
			_ => {
				return Err(format!(
					"Parent kind: {:?}\nPointer: {}\nParent: {}",
					parent.kind(),
					pointer_value.to_string(),
					parent.to_string()
				));
			}
		}
	}

	if let Some(arg_list) = ast::ArgList::cast(parent.clone()) {
		// This array is passed on to a function as parameter
		return classify_usage_of_function_param_list(
			pointer_value,
			&arg_list,
			parent_function,
			context,
			external_functions,
		);
	}
	if let Some(method_call) = ast::MethodCallExpr::cast(parent.clone()) {
		let method_name = method_call.name_ref().unwrap().to_string();
		return match method_name.as_str() {
			"is_null" => Ok(EffectiveUsageClassification {
				entire_usage: method_call.syntax().clone(),
				variant: EffectiveUsageTypeVariant::NullCheck,
			}),
			"offset" => {
				// At least referencing an element
				// But, if this parent's parent is an immediate deref,
				// we are essentially also just doing a DerefElement!
				let offset_value = method_call.arg_list().unwrap().args().nth(0).unwrap();

				let (effective_parent, _) =
					unwrap_useless_parent_syntax(method_call.syntax(), vec![]);

				let method_call_usage = classify_effective_usage_type(
					&effective_parent,
					parent_function,
					context,
					external_functions,
				);

				if let Ok(EffectiveUsageClassification {
					entire_usage: method_call_usage,
					variant:
						EffectiveUsageTypeVariant::DereferenceElement {
							element_index: _,
							is_mut,
						},
				}) = method_call_usage
				{
					// The parent of this `.offset()` immediately dereferences,
					// essentially resulting in a direct access of this element.
					// The recursive call above only ever reaches this variant
					// via the bare `*x` deref branch, which always reports
					// `element_index: None` since it can't see the `.offset()`
					// underneath it — the real index is `offset_value`, known
					// locally right here.
					Ok(EffectiveUsageClassification {
						entire_usage: method_call_usage,
						variant: EffectiveUsageTypeVariant::DereferenceElement {
							element_index: Some(offset_value.syntax().clone()),
							is_mut,
						},
					})
				} else {
					Ok(EffectiveUsageClassification {
						entire_usage: method_call.syntax().clone(),
						variant: EffectiveUsageTypeVariant::ReferenceElement {
							element_index: offset_value.syntax().clone(),
						},
					})
				}
			}
			"addr" => Ok(EffectiveUsageClassification {
				entire_usage: method_call.syntax().clone(),
				variant: EffectiveUsageTypeVariant::AddressOfPointer,
			}),
			"offset_from" => {
				let offset_value = method_call.arg_list().unwrap().args().nth(0).unwrap();

				Ok(EffectiveUsageClassification {
					entire_usage: method_call.syntax().clone(),
					variant: EffectiveUsageTypeVariant::OffsetFrom {
						other: offset_value.syntax().clone(),
					},
				})
			}
			"as_mut_ptr" => {
				// x.as_mut_ptr() can be called on slices to get the raw pointer,
				// essentially the same as just having the raw pointer to an unsafe
				// array in the first place. For effective usage, look one parent
				// higher
				let (effective_parent, _skipped_casts) =
					unwrap_all_useless_and_flow_parent_syntax(&parent, vec![]);
				classify_effective_usage_type(
					&effective_parent,
					parent_function,
					context,
					external_functions,
				)
			}
			_ => Err(format!(
				"Parent kind: {:?}\nArray: {}\nParent: {}",
				parent.kind(),
				pointer_value.to_string(),
				parent.to_string()
			)),
		};
	}
	if let Some(prefix) = ast::PrefixExpr::cast(parent.clone()) {
		match prefix.op_kind().unwrap() {
			ast::UnaryOp::Deref => {
				let prefix_parent_usage = classify_effective_usage_type(
					&prefix.syntax(),
					parent_function,
					context,
					external_functions,
				);

				if let Ok(EffectiveUsageClassification {
					entire_usage: prefix_parent_usage,
					variant: EffectiveUsageTypeVariant::ReferenceRawArray { .. },
				}) = prefix_parent_usage
				{
					// We immediately reference the dereferenced element, which
					// essentially is exactly the same as an immediate reference
					// of an element
					return classify_effective_usage_type(
						&prefix_parent_usage,
						parent_function,
						context,
						external_functions,
					);
				}

				return Ok(EffectiveUsageClassification {
					entire_usage: prefix.syntax().clone(),
					variant: EffectiveUsageTypeVariant::DereferenceElement {
						element_index: None,
						is_mut: is_mutable_access_path(
							&ast::Expr::PrefixExpr(prefix.clone()),
							context,
						),
					},
				});
			}
			_ => {
				return Err(format!(
					"Parent kind: {:?}\nArray: {}\nParent: {}",
					parent.kind(),
					pointer_value.to_string(),
					parent.to_string()
				));
			}
		}
	}
	if let Some(let_statement) = ast::LetStmt::cast(parent.clone()) {
		// We are initializing this let statement
		// Get the hir::Local of the initializing variable
		let ast::Pat::IdentPat(ident_pat) = let_statement.pat().unwrap() else {
			todo!("Deal with storing pointer value in nameless variable");
		};

		let Some(local) = context.rust_semantics.to_def(&ident_pat).or_else(|| {
			let parent_function =
				parent_function.expect("A `let` statement must live inside a function body");
			resolve_local_by_range(context, parent_function, ident_pat.syntax().text_range())
		}) else {
			panic!("Failed to get local of let statement");
		};

		return Ok(EffectiveUsageClassification {
			entire_usage: let_statement.syntax().clone(),
			variant: EffectiveUsageTypeVariant::StoreElsewhere(
				PointerStoreDestination::NewVariable { local },
			),
		});
	}
	if let Some(index_expression) = ast::IndexExpr::cast(parent.clone()) {
		return Ok(EffectiveUsageClassification {
			entire_usage: index_expression.syntax().clone(),
			variant: EffectiveUsageTypeVariant::DereferenceElement {
				element_index: Some(index_expression.index().unwrap().syntax().clone()),
				is_mut: is_mutable_access_path(
					&ast::Expr::cast(pointer_value.clone()).unwrap(),
					context,
				),
			},
		});
	}

	if let Some(return_expression) = ast::ReturnExpr::cast(parent.clone()) {
		return Ok(EffectiveUsageClassification {
			entire_usage: return_expression.syntax().clone(),
			variant: EffectiveUsageTypeVariant::Returned {
				function: parent_function
					.expect("A `return` expression must live inside a function body"),
			},
		});
	}
	if let Some(ref_expression) = ast::RefExpr::cast(parent.clone()) {
		// We reference this pointer. Check if the referenced value is
		// dereferenced again
		let ref_parent_usage =
			classify_effective_usage_type(&parent, parent_function, context, external_functions);

		if let Ok(EffectiveUsageClassification {
			entire_usage: ref_parent_usage,
			variant: EffectiveUsageTypeVariant::DereferenceElement { .. },
		}) = ref_parent_usage
		{
			return classify_effective_usage_type(
				&ref_parent_usage,
				parent_function,
				context,
				external_functions,
			);
		}

		let ref_is_raw = ref_expression.raw_token().is_some();
		let ref_is_mut = ref_expression.mut_token().is_some();

		return Ok(EffectiveUsageClassification {
			entire_usage: ref_expression.syntax().clone(),
			variant: EffectiveUsageTypeVariant::ReferenceRawArray {
				is_mut: ref_is_mut,
				is_raw: ref_is_raw,
			},
		});
	}

	if let Some(array_expression) = ast::ArrayExpr::cast(parent.clone()) {
		// Our array is put inside a stack array: `[a]`.
		// We now want to know whether this stack array is supposed to be owning
		// or not.
		let stack_array_usage =
			classify_effective_usage_type(&parent, parent_function, context, external_functions)
				.unwrap();

		return Ok(EffectiveUsageClassification {
			entire_usage: array_expression.syntax().clone(),
			variant: EffectiveUsageTypeVariant::StoreElsewhere(
				PointerStoreDestination::InCollection {
					collection: array_expression.syntax().clone(),
					collection_usage: Box::new(stack_array_usage),
				},
			),
		});
	}

	if let Some(statement) = ast::Stmt::cast(parent.clone()) {
		// The entire statement has been reached, so there is no more
		// 'effective use'ness to be found.
		return Ok(EffectiveUsageClassification {
			entire_usage: statement.syntax().clone(),
			variant: EffectiveUsageTypeVariant::Unused,
		});
	}

	let err = format!(
		"Parent kind: {:?}\nArray: {}\nParent: {}",
		parent.kind(),
		pointer_value.to_string(),
		parent.to_string()
	);

	Err(err)
}

fn classify_usage_of_function_param_list(
	param_value: &syntax::SyntaxNode,
	arg_list: &ast::ArgList,
	parent_function: Option<hir::Function>,
	context: &ProjectContext,
	external_functions: &HashMap<&hir::Function, &CFunctionDeclaration>,
) -> Result<EffectiveUsageClassification, String> {
	let arg_index = arg_list
		.args()
		.position(|arg| arg.syntax() == param_value)
		.unwrap();

	let call_node = arg_list.syntax().parent().unwrap();

	if let Some(function_call) = ast::CallExpr::cast(call_node.clone()) {
		// The pointer was passed to a function
		if let Some(function) = get_hir_function_from_call_expr(&function_call, context) {
			match external_functions
				.get(&function)
				.map(|decl| decl.function_name.clone())
			{
				Some(CFunction::Free) => {
					return Ok(EffectiveUsageClassification {
						entire_usage: function_call.syntax().clone(),
						variant: EffectiveUsageTypeVariant::Deallocate,
					});
				}
				Some(CFunction::Realloc) => {
					// We want to know how the result of realloc is used
					let (effective_realloc_parent, effective_realloc_casts) =
						unwrap_useless_parent_syntax(&function_call.syntax(), vec![]);
					let (classification_realloc_parent, effective_realloc_casts) =
						unwrap_all_useless_and_flow_parent_syntax(
							&effective_realloc_parent,
							effective_realloc_casts,
						);

					assert_eq!(
						classification_realloc_parent,
						unwrap_useless_parent_syntax(&classification_realloc_parent, vec![]).0,
						"Did not expect casts after control-flow syntax"
					);

					let effective_realloc_usage_type = classify_effective_usage_type(
						&classification_realloc_parent,
						parent_function,
						context,
						external_functions,
					)
					.unwrap();

					let new_allocation_size = arg_list.args().nth(1).unwrap();

					let effective_realloc_usage = EffectiveUsageType {
						classification_pointer: classification_realloc_parent,
						casts: effective_realloc_casts,
						variant: effective_realloc_usage_type.variant,
						effective_pointer: effective_realloc_parent,
						effective_usage: effective_realloc_usage_type.entire_usage,
					};

					return Ok(EffectiveUsageClassification {
						entire_usage: function_call.syntax().clone(),
						variant: EffectiveUsageTypeVariant::Reallocate {
							result_usage: Box::new(effective_realloc_usage),
							new_size: new_allocation_size.syntax().clone(),
						},
					});
				}
				_ => {
					// The pointer is passed to any other function
					return Ok(EffectiveUsageClassification {
						entire_usage: function_call.syntax().clone(),
						variant: EffectiveUsageTypeVariant::PassToFunction {
							function: Some(function),
							as_nth_param: arg_index as u32,
						},
					});
				}
			}
		} else {
			// Unable to infer which function got called. Likely a function pointer then
			return Ok(EffectiveUsageClassification {
				entire_usage: function_call.syntax().clone(),
				variant: EffectiveUsageTypeVariant::PassToFunction {
					function: None,
					as_nth_param: arg_index as u32,
				},
			});
		}
	};
	// Otherwise may be method call instead
	if let Some(method_call) = ast::MethodCallExpr::cast(call_node.clone()) {
		let method_name = method_call.name_ref().unwrap().to_string();

		let method = context.rust_semantics.resolve_method_call(&method_call);
		if let Some(method) = method {
			return Ok(EffectiveUsageClassification {
				entire_usage: method_call.syntax().clone(),
				variant: EffectiveUsageTypeVariant::PassToFunction {
					function: Some(method),
					as_nth_param: arg_index as u32,
				},
			});
		} else {
			if method_name == "offset" || method_name == "offset_from" {
				return Ok(EffectiveUsageClassification {
					entire_usage: param_value.clone(),
					variant: EffectiveUsageTypeVariant::PassToFunction {
						function: None,
						as_nth_param: arg_index as u32,
					},
				});
			}
		}
	};
	Err(format!("Failed to resolve function called"))
}

/// Given a call expression, get the hir::Function that corresponds to it
/// Returns None when function pointer for example is called
pub fn get_hir_function_from_call_expr(
	call_expr: &ast::CallExpr,
	context: &ProjectContext,
) -> Option<hir::Function> {
	let expr = call_expr.expr().unwrap();
	let Some(path_expr) = ast::PathExpr::cast(expr.syntax().clone()) else {
		// We are not calling a directly-named function.
		// This is likely a function pointer thats being called
		return None;
	};
	let path = path_expr.path().unwrap();
	let path = context.rust_semantics.resolve_path(&path)?;

	let hir::PathResolution::Def(ModuleDef::Function(func)) = path else {
		return None;
	};
	Some(func)
}

fn resolve_local_by_range(
	context: &ProjectContext,
	function: hir::Function,
	target_range: syntax::TextRange,
) -> Option<hir::Local> {
	let anchored_fn = context.rust_semantics.source(function)?.value;
	let node = match anchored_fn.syntax().covering_element(target_range) {
		syntax::NodeOrToken::Node(n) => n,
		syntax::NodeOrToken::Token(t) => t.parent()?,
	};
	let ident_pat = node.ancestors().find_map(ast::IdentPat::cast)?;
	context.rust_semantics.to_def(&ident_pat)
}

fn is_mutable_access_path(expr: &ast::Expr, context: &ProjectContext) -> bool {
	match expr {
		ast::Expr::ParenExpr(paren) => is_mutable_access_path(&paren.expr().unwrap(), context),
		ast::Expr::FieldExpr(field) => is_mutable_access_path(&field.expr().unwrap(), context),
		ast::Expr::CastExpr(cast) => is_mutable_access_path(&cast.expr().unwrap(), context),
		ast::Expr::MethodCallExpr(method_call)
			if method_call.name_ref().unwrap().text() == "offset" =>
		{
			// `.offset(..)` preserves the receiver's constness
			is_mutable_access_path(&method_call.receiver().unwrap(), context)
		}
		ast::Expr::PrefixExpr(prefix) if prefix.op_kind() == Some(ast::UnaryOp::Deref) => {
			let operand = prefix.expr().unwrap();

			if let Some(hir::Mutability::Shared) = raw_ptr_mutability_via_syntax(&operand, context)
			{
				return false;
			}

			// This deref is `*mut` (or not a raw pointer at all) - keep
			// walking, since an earlier deref further down the chain
			// could still be `*const`.
			is_mutable_access_path(&operand, context)
		}
		// No more pointer derefs to check - whatever's left (a bare
		// variable, a call returning an owned value, etc.) doesn't itself
		// constrain mutability any further.
		_ => true,
	}
}

/// Determines a raw pointer's mutability by tracing it back to its
/// declaring `let`/parameter and reading the `const`/`mut` keyword directly
/// off that declaration's syntax - no `hir::Type` involved.
///
/// Only resolves the case where `expr` is a bare local/parameter reference
/// with a visible type annotation. Anything else - a field access, a call
/// result, a re-borrowed pointer with no annotation of its own - has no
/// single "declaration site" to trace back to, and returns `None`.
fn raw_ptr_mutability_via_syntax(
	expr: &ast::Expr,
	context: &ProjectContext,
) -> Option<hir::Mutability> {
	let ast::Expr::PathExpr(path_expr) = expr else {
		return None;
	};
	let path = path_expr.path()?;
	let hir::PathResolution::Local(local) = context.rust_semantics.resolve_path(&path)? else {
		return None;
	};

	let source = local.primary_source(context.rust_semantics.db);
	let ident_pat = source.as_ident_pat()?;

	let declared_type = ident_pat
		.syntax()
		.ancestors()
		.find_map(ast::LetStmt::cast)
		.and_then(|let_stmt| let_stmt.ty())
		.or_else(|| {
			ident_pat
				.syntax()
				.ancestors()
				.find_map(ast::Param::cast)
				.and_then(|param| param.ty())
		})?;

	let ptr_type = ast::PtrType::cast(declared_type.syntax().clone())?;

	if ptr_type.const_token().is_some() {
		Some(hir::Mutability::Shared)
	} else if ptr_type.mut_token().is_some() {
		Some(hir::Mutability::Mut)
	} else {
		None
	}
}
