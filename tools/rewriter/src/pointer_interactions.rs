use std::collections::{HashMap, HashSet};

use ide_db::defs;
use syntax::{
	AstNode, ast::{self, HasArgList}, ted::Element,
};

use crate::{
	ProjectContext, effective_usage_type::{EffectiveUsageTypeVariant, classify_effective_usage_type, unwrap_all_useless_and_flow_parent_syntax, unwrap_control_flow_parent_syntax, unwrap_useless_child_syntax, unwrap_useless_parent_syntax}, extern_function::CFunctionDeclaration, pointer_source::{NamedPointerSource, PointerSourceLocation},
};

/// Represents an interaction of a pointer value.
///
/// Direct interactions
/// - `let _ = my_value`
/// - `some_fun(my_value)`
///
/// Indirect interactions
/// - `let my_value = ...;`
/// - `return ...;`
///
/// Indirect interactions could overlap with direct ones:
/// - `let my_value = my_other_value;`
/// - `return my_value`
///
pub struct NamedPointerInteraction {
	/// The entire minimal valid expression that represents this pointer value.
	/// Can either be the name reference of the source when direct interaction,
	/// or the value that interacted with the source when indirect.
	/// Examples:
	/// - `my_existing_var`
	/// - `std::ptr::null()`
	/// - `returning_call(params)`
	/// - `0 as *const T`
	pub value: ast::Expr,
	/// The pointer source that prompted the detection of this usage. This usage
	/// therefore can represent another value than this source:
	/// - `let my_source = pointer_usage;`
	pub source: NamedPointerSource,
	/// Indicates whether this usage is directly from an existing source (direct)
	/// or if this is value interacts with the source
	pub is_direct_usage: bool,
	pub file_id: vfs::FileId,
	/// The function whose body contains this usage.
	/// `None` when the usage occurs outside of any function body, such as a
	/// struct-literal field initializer inside a top-level `static`/`const` item.
	pub containing_function: Option<hir::Function>,
	pub _file_location: String,
}

/// Collects all pointer values that interact with given sources, including the
/// usages of the given sources as well. Removes duplicate usages,
pub fn get_all_pointer_interactions(
	pointer_sources: &Vec<NamedPointerSource>,
	context: &ProjectContext,
	all_c_functions: &HashMap<vfs::FileId, HashSet<CFunctionDeclaration>>,
) -> Vec<NamedPointerInteraction> {
	let mostly_direct_usages: Vec<NamedPointerInteraction> = pointer_sources
		.iter()
		.map(|source| get_direct_usages_of_pointer_source(context, source))
		.flatten()
		.collect();

	let indirect_usages: Vec<NamedPointerInteraction> = pointer_sources
		.iter()
		.map(|source| get_indirect_usages_of_pointer_source(context, source))
		.flatten()
		.collect();

	// Properly split now
	let (direct_usages, mut indirect_usages): (
		Vec<NamedPointerInteraction>,
		Vec<NamedPointerInteraction>,
	) = mostly_direct_usages
		.into_iter()
		.chain(indirect_usages.into_iter())
		.partition(|usage| usage.is_direct_usage);

	// Remove duplicates (usages that point to the same code, found from different sources)
	indirect_usages.retain(|indirect_usage| {
		let (indirect_minimal_expression, _) =
			unwrap_useless_child_syntax(&indirect_usage.value, vec![]);

		for direct_usage in &direct_usages {
			if direct_usage.file_id != indirect_usage.file_id {
				continue;
			}

			let (direct_minimal_expression, _) =
				unwrap_useless_child_syntax(&direct_usage.value, vec![]);
			let (direct_maximal_expression, _) =
				unwrap_all_useless_and_flow_parent_syntax(&direct_usage.value.syntax(), vec![]);

			let matches_minimal = indirect_minimal_expression.syntax().text_range() == direct_minimal_expression.syntax().text_range();
			let matches_maximal = indirect_minimal_expression.syntax().text_range() == direct_maximal_expression.text_range();

			if !matches_minimal && matches_maximal {
				println!("At: {}", indirect_usage._file_location);
				println!("Indirect value:\n{}", indirect_usage.value.syntax().text());
				println!("Direct value:\n{}", direct_usage.value.syntax().text());
				println!("Indirect min:\n{}", indirect_minimal_expression.syntax().text());
				println!("Direct min:\n{}", direct_minimal_expression.syntax().text());
				println!("Direct max:\n{}", direct_maximal_expression.text());

				// unreachable!("My assumption was wrong")
			}

			if matches_minimal || matches_maximal
			{
				// These usages boil down to the same thing, like:
				return false;
			}

			let mut extern_function_map: HashMap<&hir::Function, &CFunctionDeclaration> = HashMap::new();

			all_c_functions.get(&direct_usage.file_id).inspect(|decls| {
				for decl in *decls {
					extern_function_map.insert(&decl.function, decl);
				}
			});

			let (effective_direct_usage_value, _) = unwrap_useless_parent_syntax(direct_usage.value.syntax(), vec![]);
			let (classification_direct_usage_value, _) = unwrap_all_useless_and_flow_parent_syntax(&effective_direct_usage_value, vec![]);

			let Ok(usage_variant) = classify_effective_usage_type(&classification_direct_usage_value, direct_usage.containing_function, context, &extern_function_map) else {
				continue;
			};
			if let EffectiveUsageTypeVariant::Reallocate { .. } = usage_variant.variant {
				if indirect_minimal_expression.syntax().text_range() == usage_variant.entire_usage.text_range() {
					println!("=\n=\n=\n=\n=\n=\n=\n=\n=\n=\n=\n=\nREEEEEE=\n=\n=\n=\n=\n=\n=\n");
					return false;
				}
			}
			// TODO: Also do this to `&raw mut ..` being passed to functions
		}
		return true;
	});

	direct_usages
		.into_iter()
		.chain(indirect_usages.into_iter())
		.collect()
}

/// Finds all other pointer values that interact with the given source, such as
/// `X` in:
/// - `let my_source = X;`
///
/// Or when the function return value is a source:
/// - `return X;`
///
/// Situations that allow for indirectness:
/// - Return values
/// - Setting equal to a value
/// - Initializing with a value
/// - Parameter-passed values
fn get_indirect_usages_of_pointer_source(
	context: &ProjectContext,
	source: &NamedPointerSource,
) -> Vec<NamedPointerInteraction> {
	let mut indirect_usages: Vec<NamedPointerInteraction> = vec![];

	// If source is initialized in let statement, collect the new value
	if let PointerSourceLocation::FnBody {
		let_statement,
		function,
		..
	} = &source.location
	{
		if let Some(initializer) = let_statement.initializer() {
			indirect_usages.push(NamedPointerInteraction {
				value: unwrap_useless_child_syntax(&initializer, vec![]).0,
				source: source.clone(),
				is_direct_usage: false,
				file_id: source.file_id,
				containing_function: Some(*function),
				_file_location: source._file_location.clone(),
			});
		}
	}

	// If source is parameter, collect values that are passed to it
	if let PointerSourceLocation::FnParam {
		param_index,
		function,
		..
	} = source.location
	{
		indirect_usages.extend(get_values_passed_to_param_source(
			source,
			context,
			param_index,
			&function,
		));
	}

	// If source is function result, collect values that are returned
	if let PointerSourceLocation::FnResult { function, .. } = source.location {
		indirect_usages.extend(get_values_returned_to_source(source, &function, context));
	}

	indirect_usages
}

/// Get points where any value is passed in a function call to
/// a parameter that is the given source.
fn get_values_passed_to_param_source(
	source: &NamedPointerSource,
	context: &ProjectContext,
	param_index: usize,
	function: &hir::Function,
) -> Vec<NamedPointerInteraction> {
	let mut value_usages = vec![];

	// 1. Get all places where this function is used
	let function_definition = defs::Definition::Function(*function);
	let function_usages = function_definition.usages(context.rust_semantics).all();

	// See the matching comment in `get_direct_usages_of_pointer_source`: a
	// shared-source lib+bin package reports each real call site once per
	// crate-graph root, so dedupe by the resolved `(FileId, TextRange)`.
	let mut seen_usage_locations: HashSet<(vfs::FileId, syntax::TextRange)> = HashSet::new();

	for (file_id, file_usages) in function_usages {
		// 2. Get the values that are passed to this parameter
		let hir_file_id = hir::HirFileId::from(file_id);
		let file_syntax = context.rust_semantics.parse_or_expand(hir_file_id);
		let file_id = file_id.file_id(context.db);
		let line_index = context.rust_analysis.file_line_index(file_id).unwrap();

		let file_path = context.vfs.file_path(file_id).to_string();

		for usage in file_usages {
			if !seen_usage_locations.insert((file_id, usage.range)) {
				continue;
			}

			// Get the closest syntax node parent of our given usage
			let node = match file_syntax.covering_element(usage.range) {
				syntax::NodeOrToken::Node(n) => n,
				syntax::NodeOrToken::Token(t) => t.parent().unwrap(),
			};

			// Filter out occurrences where the function is only referenced and not called,
			// such as passed as a function pointer.
			let Some(call_expr) = node.ancestors().find_map(ast::CallExpr::cast) else {
				continue;
			};
			let is_callee = call_expr
				.expr()
				.is_some_and(|callee| callee.syntax().text_range().contains_range(usage.range));
			if !is_callee {
				continue;
			}

			let Some(arg_expr) = call_expr
				.arg_list()
				.and_then(|args| args.args().nth(param_index))
			else {
				// Fewer arguments were passed than this parameter's position requires
				continue;
			};

			let usage_line_index = line_index.line_col(arg_expr.syntax().text_range().start());
			let usage_file_location = format!(
				"{}:{}:{}",
				file_path,
				usage_line_index.line + 1,
				usage_line_index.col + 1
			);

			let encompassing_function =
				get_encompassing_function(call_expr.syntax(), file_id, context).unwrap();

			value_usages.push(NamedPointerInteraction {
				value: arg_expr,
				file_id,
				_file_location: usage_file_location,
				source: source.clone(),
				is_direct_usage: false,
				containing_function: encompassing_function,
			});
		}
	}
	value_usages
}

/// Gets all pointer values that are returned inside the given function.
fn get_values_returned_to_source(
	source: &NamedPointerSource,
	function: &hir::Function,
	context: &ProjectContext,
) -> Vec<NamedPointerInteraction> {
	let mut usages = vec![];

	let function_ast = context.rust_semantics.source(*function).unwrap().value;
	let file_id = source.file_id;
	let line_index = context.rust_analysis.file_line_index(file_id).unwrap();
	let file_path = context.vfs.file_path(file_id).to_string();

	let returned_values = get_return_value_exprs(&function_ast);

	for returned_value in &returned_values {
		let usage_line_index = line_index.line_col(returned_value.syntax().text_range().start());
		let usage_file_location = format!(
			"{}:{}:{}",
			file_path,
			usage_line_index.line + 1,
			usage_line_index.col + 1
		);
		let usage = NamedPointerInteraction {
			value: returned_value.clone(),
			file_id,
			containing_function: Some(*function),
			_file_location: usage_file_location,
			source: source.clone(),
			is_direct_usage: false,
		};
		usages.push(usage);
	}

	usages
}

/// Finds all places where the given pointer source is directly used.
/// This means this is a returned usage:
/// - `let _ = my_source;`
///
/// But not this (indirect):
/// - `let my_source = _;`
fn get_direct_usages_of_pointer_source(
	context: &ProjectContext,
	source: &NamedPointerSource,
) -> Vec<NamedPointerInteraction> {
	let mut usages = vec![];

	let Some(definition) = source.get_definition() else {
		// Unable to get definition
		// Happens for unnamed parameters, such as `free(_: *libc::c_void)`,
		// which has no usages
		return vec![];
	};
	let all_usages = definition.usages(context.rust_semantics).all();

	// A single Cargo package with a `[lib]` target plus another target
	// (e.g. a `[[bin]]`) that depends on it gets a separate crate-graph
	// root per target, so shared source files are analyzed once per root.
	// `Definition::usages()` walks the whole crate graph, so it reports the
	// exact same source location once per root it's reachable from, tagged
	// with different `EditionedFileId`s that collapse to the same `FileId`
	// once resolved below. Two usages at the same `(FileId, TextRange)` are
	// therefore guaranteed to be the same real occurrence, not two distinct
	// usages that both happen to need rewriting.
	let mut seen_usage_locations: HashSet<(vfs::FileId, syntax::TextRange)> = HashSet::new();

	for (file_id, file_usages) in all_usages {
		let hir_file_id = hir::HirFileId::from(file_id);
		let file_syntax = context.rust_semantics.parse_or_expand(hir_file_id);
		let file_id = file_id.file_id(context.db);
		let line_index = context.rust_analysis.file_line_index(file_id).unwrap();

		let file_path = context.vfs.file_path(file_id).to_string();

		for usage in file_usages {
			if !seen_usage_locations.insert((file_id, usage.range)) {
				continue;
			}

			let usage_line_index = line_index.line_col(usage.range.start());
			let usage_file_location = format!(
				"{}:{}:{}",
				file_path,
				usage_line_index.line + 1,
				usage_line_index.col + 1
			);

			// Get the closest syntax node parent of our given usage
			let node = match file_syntax.covering_element(usage.range) {
				syntax::NodeOrToken::Node(n) => n,
				syntax::NodeOrToken::Token(t) => t.parent().unwrap(),
			};

			let encompassing_function = get_encompassing_function(&node, file_id, context).unwrap();

			// Get the entire "atomic" ast expression that contains this reference
			let usage_expression = match &source.location {
				PointerSourceLocation::FnParam { .. } | PointerSourceLocation::FnBody { .. } => {
					// Simply the PathExpr we already have in Node is sufficient
					ast::Expr::PathExpr(node.ancestors().find_map(ast::PathExpr::cast).unwrap())
				}
				PointerSourceLocation::StructField { .. } => {
					let parent = node.parent().unwrap();

					if let Some(record_field) = ast::RecordExprField::cast(parent.clone()) {
						let initializer = record_field.expr().unwrap();
						usages.push(NamedPointerInteraction {
							value: initializer,
							source: source.clone(),
							is_direct_usage: false,
							file_id,
							containing_function: encompassing_function,
							_file_location: usage_file_location,
						});
						continue;
					} else if let Some(field_expr) = ast::FieldExpr::cast(parent.clone()) {
						ast::Expr::FieldExpr(field_expr)
					} else {
						unreachable!("Unknown field usage type")
					}
				}
				PointerSourceLocation::FnResult { .. } => {
					// That means that 'node' should be a NameRef
					let name_ref = ast::NameRef::cast(node.clone()).unwrap();
					get_atomic_expr_of_function_call(name_ref).unwrap()
				}
			};

			// Exception:
			// When the usage is the LHS of an assignment, the actual usage is that
			// of the new value, not this source:
			if let Some(rhs_value) = expr_is_lhs_of_assignment(&usage_expression) {
				usages.push(NamedPointerInteraction {
					value: rhs_value,
					source: source.clone(),
					is_direct_usage: false,
					file_id,
					containing_function: encompassing_function,
					_file_location: usage_file_location,
				});
				continue;
			}

			usages.push(NamedPointerInteraction {
				value: usage_expression,
				file_id,
				_file_location: usage_file_location,
				containing_function: encompassing_function,
				source: source.clone(),
				is_direct_usage: true,
			});
		}
	}

	// Second sweep based off syntax: `Definition::usages()` can fail to
	// attribute a `.field` access to its `Definition` when RA's combined
	// type inference gives up on the receiver (e.g. `(**x.offset(i)).field`
	// resolves the receiver to `{type error}`, even though `x`'s own type,
	// and thus the field, is perfectly well-defined). This sweep resolves
	// those receivers itself, bypassing the broken combined inference, and
	// catches usages the semantic search silently missed.
	if let PointerSourceLocation::StructField { .. } = &source.location {
		let syntax_usages = get_struct_field_usages(source, context);

		for syntax_usage in syntax_usages {
			let already_found = usages.iter().any(|existing| {
				existing.file_id == syntax_usage.file_id
					&& existing.value.syntax().text_range()
						== syntax_usage.value.syntax().text_range()
			});
			if !already_found {
				usages.push(syntax_usage);
			}
		}
	}

	usages
}

/// Finds usages of `source` (a `StructField` source) by walking every
/// function in the crate and resolving `.field_name` accesses directly,
/// rather than relying on `Definition::usages()`'s reference-search index.
/// See the comment at its call site for why this is necessary.
fn get_struct_field_usages(
	source: &NamedPointerSource,
	context: &ProjectContext,
) -> Vec<NamedPointerInteraction> {
	let PointerSourceLocation::StructField { field, .. } = &source.location else {
		return vec![];
	};

	let mut usages = vec![];
	let field_name = field.name(context.db).as_str().to_string();

	for krate in hir::Crate::all(context.db) {
		if !krate.origin(context.db).is_local() {
			continue;
		}

		for module in krate.modules(context.db) {
			for item in module.declarations(context.db) {
				let hir::ModuleDef::Function(function) = item else {
					continue;
				};

				let Some(in_file) = context.rust_semantics.source(function) else {
					continue;
				};
				let function_ast = in_file.value;
				let Some(file_id_editioned) = in_file.file_id.file_id() else {
					continue; // macro-generated, nothing to rewrite there
				};
				let file_id = file_id_editioned.file_id(context.db);
				let file_path = context.vfs.file_path(file_id).to_string();
				let line_index = context.rust_analysis.file_line_index(file_id).unwrap();

				for field_expr in function_ast
					.syntax()
					.descendants()
					.filter_map(ast::FieldExpr::cast)
				{
					let Some(name_ref) = field_expr.name_ref() else {
						continue;
					};
					if name_ref.text() != field_name {
						continue;
					}

					if resolve_field_with_fallback(&field_expr, context) != Some(*field) {
						continue;
					}

					let usage_expr = ast::Expr::FieldExpr(field_expr);
					let pos = line_index.line_col(usage_expr.syntax().text_range().start());
					let usage_file_location =
						format!("{}:{}:{}", file_path, pos.line + 1, pos.col + 1);

					if let Some(rhs_value) = expr_is_lhs_of_assignment(&usage_expr) {
						usages.push(NamedPointerInteraction {
							value: rhs_value,
							source: source.clone(),
							is_direct_usage: false,
							file_id,
							containing_function: Some(function),
							_file_location: usage_file_location,
						});
						continue;
					}

					usages.push(NamedPointerInteraction {
						value: usage_expr,
						source: source.clone(),
						is_direct_usage: true,
						file_id,
						containing_function: Some(function),
						_file_location: usage_file_location,
					});
				}
			}
		}
	}

	usages
}

/// Resolves a `FieldExpr` to the `hir::Field` it accesses, falling back to
/// `manually_resolve_receiver_adt` when RA's own `resolve_field` fails —
/// which it reliably does for receivers shaped like `(**x.offset(i))` (a
/// confirmed RA inference gap, not something specific to any one call site).
/// Use this instead of calling `Semantics::resolve_field` directly anywhere
/// a `FieldExpr` needs resolving — that's exactly what silently panicked
/// before this helper existed.
pub fn resolve_field_with_fallback(
	field_expr: &ast::FieldExpr,
	context: &ProjectContext,
) -> Option<hir::Field> {
	if let Some(field) = context
		.rust_semantics
		.resolve_field(field_expr)
		.and_then(|resolved| resolved.left())
	{
		return Some(field);
	}

	let receiver = field_expr.expr()?;
	let hir::Adt::Struct(struc) = manually_resolve_receiver_adt(&receiver, context)? else {
		return None;
	};
	let field_name = field_expr.name_ref()?.text().to_string();
	struc
		.fields(context.db)
		.into_iter()
		.find(|f| f.name(context.db).as_str() == field_name)
}

/// Peels off leading dereferences (`*expr`) and pointee-preserving
/// pointer-arithmetic method calls (`.offset(..)`, `.add(..)`, etc.) from
/// `receiver`, resolves the *remaining* base expression's own type — which,
/// unlike the combined expression, RA can normally infer just fine — and
/// then manually strips one raw-pointer layer per dereference counted.
/// This sidesteps the specific RA inference gap where `(**x.offset(i))`
/// infers to `{type error}` even though `x`'s own type is well-defined.
fn manually_resolve_receiver_adt(
	receiver: &ast::Expr,
	context: &ProjectContext,
) -> Option<hir::Adt> {
	let mut expr = receiver.clone();
	let mut deref_count = 0usize;

	loop {
		expr = match &expr {
			ast::Expr::ParenExpr(paren) => paren.expr()?,
			ast::Expr::PrefixExpr(prefix) if prefix.op_kind() == Some(ast::UnaryOp::Deref) => {
				deref_count += 1;
				prefix.expr()?
			}
			ast::Expr::MethodCallExpr(call) => {
				let name = call.name_ref()?.text().to_string();
				if matches!(
					name.as_str(),
					"offset" | "wrapping_offset" | "add" | "sub" | "wrapping_add" | "wrapping_sub"
				) {
					call.receiver()?
				} else {
					break;
				}
			}
			_ => break,
		};
	}

	let base_ty = context.rust_semantics.type_of_expr(&expr)?.original;

	let mut ty = base_ty;
	for _ in 0..deref_count {
		ty = ty.remove_raw_ptr()?;
	}

	ty.as_adt()
}

/// Checks if the given expression is the left-hand side of an assignment, such
/// as `x` in `x = _;`.
/// Returns the RHS value
fn expr_is_lhs_of_assignment(expr: &ast::Expr) -> Option<ast::Expr> {
	let parent = expr.syntax().parent().unwrap();

	if let Some(bin_expr) = ast::BinExpr::cast(parent) {
		if let ast::BinaryOp::Assignment { .. } = bin_expr.op_kind().unwrap() {
			if expr == &bin_expr.lhs().unwrap() {
				return bin_expr.rhs();
			}
		}
	}
	None
}

/// Gets all values that are returned inside the given function ast.
/// Skips:
/// - closure return statements
/// - implicit return values (not present in C2Rust output)
fn get_return_value_exprs(function_ast: &ast::Fn) -> Vec<ast::Expr> {
	let Some(body) = function_ast.body() else {
		return vec![];
	};
	body.syntax()
		.descendants()
		.filter_map(ast::ReturnExpr::cast)
		.filter(|ret| {
			!ret.syntax()
				.ancestors()
				.skip(1)
				.take_while(|ancestor| *ancestor != *body.syntax())
				.any(|ancestor| ast::ClosureExpr::can_cast(ancestor.kind()))
		})
		.filter_map(|ret| ret.expr())
		.collect()
}

/// Gets the first ancestor ast::Expr of the given NameRef of a function-call usage.
/// When name_ref is something like `foo`, this will return:
/// 	- `my_struct.foo()`
/// 	- `std::thing::foo()`
/// 	- None, in case `let a = foo;`
fn get_atomic_expr_of_function_call(name_ref: ast::NameRef) -> Option<ast::Expr> {
	// Check for method calls, which have ast::MethodCallExpr as direct parent
	if let Some(method_call) = name_ref
		.syntax()
		.parent()
		.and_then(ast::MethodCallExpr::cast)
	{
		return Some(ast::Expr::MethodCallExpr(method_call));
	}
	// All other cases have NameRef -> PathSegment -> Path -> PathExpr
	let path_expr = name_ref
		.syntax()
		.ancestors()
		.find_map(ast::PathExpr::cast)
		.unwrap();

	match path_expr.syntax().parent().and_then(ast::CallExpr::cast) {
		Some(call_expr) => Some(ast::Expr::CallExpr(call_expr)),
		// Used as a value, not called: `let f = foo;`, meaning it is not a pointer usage
		None => Some(ast::Expr::PathExpr(path_expr)),
	}
}

fn get_encompassing_function(
	node: &syntax::SyntaxNode,
	file_id: vfs::FileId,
	context: &ProjectContext,
) -> Result<Option<hir::Function>, String> {
	// Not every usage lives inside a function body: a struct-literal field
	// initializer inside a top-level `static`/`const` item (e.g.
	// `static mut X: T = T { field: <value> }`) has no `ast::Fn` ancestor at
	// all. That's a legitimate usage, just one with no containing function.
	let Some(encompassing_function_ast) = node.ancestors().find_map(ast::Fn::cast) else {
		return Ok(None);
	};

	let module = context.rust_semantics.file_to_module_def(file_id);

	let result = context
		.rust_semantics
		.to_def(&encompassing_function_ast)
		.or_else(|| {
			let target_range = encompassing_function_ast.syntax().text_range();
			module.and_then(|m| find_function_by_range(context, m, file_id, target_range))
		})
		.ok_or(format!("Failed to resolve encompassing function"));

	result.map(Some)
}

/// Finds the `hir::Function` declared in `module` (or one of its descendant modules/impls)
/// whose source lives in `file_id` and spans exactly `target_range`.
///
/// This is a fallback for when `Semantics::to_def` fails to resolve an `ast::Fn`, which can
/// happen when the node was parsed under a different (but equivalent) `EditionedFileId` than
/// the one the def map indexed it under. See the caller in `get_usages_of_pointer_source`.
fn find_function_by_range(
	context: &ProjectContext,
	module: hir::Module,
	file_id: vfs::FileId,
	target_range: syntax::TextRange,
) -> Option<hir::Function> {
	let matches = |f: &hir::Function| {
		let src = context.rust_semantics.source(*f)?;
		let src_file_id = src.file_id.original_file(context.db).file_id(context.db);
		(src_file_id == file_id && src.value.syntax().text_range() == target_range).then_some(())
	};

	for decl in module.declarations(context.db) {
		match decl {
			hir::ModuleDef::Function(f) if matches(&f).is_some() => return Some(f),
			hir::ModuleDef::Module(child) => {
				if let Some(f) = find_function_by_range(context, child, file_id, target_range) {
					return Some(f);
				}
			}
			_ => {}
		}
	}

	for imp in module.impl_defs(context.db) {
		for item in imp.items(context.db) {
			if let hir::AssocItem::Function(f) = item {
				if matches(&f).is_some() {
					return Some(f);
				}
			}
		}
	}

	None
}
