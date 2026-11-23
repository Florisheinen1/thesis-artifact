use std::{
	collections::{HashMap, HashSet},
	unreachable,
};

use hir::{self, HasSource};
use ide_db::defs;
use syntax::{
	AstNode,
	ast::{self, HasName},
};

use crate::{
	ProjectContext,
	extern_function::{CFunction, CFunctionDeclaration},
	project::RustItemPath,
};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
/// Represents a named thing, and by referencing it, you end up with a pointer
/// value.
///
/// - Let-bound variables
/// - Struct fields
/// - Function parameters
/// - Function returnings
pub struct NamedPointerSource {
	pub is_owned: bool,
	pub is_fat: bool,
	pub is_mut: bool,
	pub location: PointerSourceLocation,
	pub name: String,
	pub file_id: vfs::FileId,
	pub rust_item_path: RustItemPath,
	pub _file_location: String,
}

impl NamedPointerSource {
	pub fn get_definition(&self) -> Option<defs::Definition> {
		match self.location {
			PointerSourceLocation::FnResult { function, .. } => {
				Some(defs::Definition::Function(function))
			}
			PointerSourceLocation::FnParam { param, .. } => Some(defs::Definition::Local(param?)),
			PointerSourceLocation::FnBody { variable, .. } => {
				Some(defs::Definition::Local(variable))
			}
			PointerSourceLocation::StructField { field, .. } => {
				Some(defs::Definition::Field(field))
			}
		}
	}
	pub fn is_output_param(&self) -> bool {
		if let PointerSourceLocation::FnParam { output_param, .. } = &self.location {
			*output_param
		} else {
			false
		}
	}
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PointerSourceLocation {
	FnResult {
		function: hir::Function,
		pointer_type: ast::Type,
		element_type: ast::Type,
		is_extern: bool,
	},
	FnParam {
		// Not every function that takes a parameter has a name. Take for example
		// the extern c function declarations like `free(_: libc::c_void)`
		param: Option<hir::Local>,
		param_index: usize,
		function: hir::Function,
		output_param: bool,
		pointer_type: ast::Type,
		element_type: ast::Type,
	},
	FnBody {
		variable: hir::Local,
		let_statement: ast::LetStmt,
		function: hir::Function,
	},
	StructField {
		field: hir::Field,
		struc: hir::Struct,
		pointer_type: ast::Type,
		element_type: ast::Type,
	},
}

pub fn get_all_pointer_sources(
	context: &ProjectContext,
	c_functions: &HashMap<vfs::FileId, HashSet<CFunctionDeclaration>>,
) -> Vec<NamedPointerSource> {
	let mut sources: Vec<NamedPointerSource> = vec![];

	for krate in hir::Crate::all(context.db) {
		if !krate.origin(context.db).is_local() {
			continue;
		}

		for module in krate.modules(context.db) {
			let module_path = get_path_of_module(context.db, module);

			for item in module.declarations(context.db) {
				let new_sources = match item {
					hir::ModuleDef::Function(function) => {
						get_pointer_sources_of_function(context, &function, &module_path)
					}
					hir::ModuleDef::Adt(adt) => match adt {
						hir::Adt::Struct(struc) => {
							get_pointer_sources_of_struct(context, &struc, &module_path)
						}
						hir::Adt::Union(_) => {
							// Crown's analysis assumes unions contain no pointer fields
							continue;
						}
						hir::Adt::Enum(_) => todo!(),
					},
					_ => continue,
				};
				sources.extend(new_sources);
			}

			// Also use the external C functions as pointer sources
			let file = module.definition_source_file_id(context.db);
			let file_id = file.file_id().unwrap().file_id(context.db);

			if let Some(c_functions_in_file) = c_functions.get(&file_id) {
				let extra_sources =
					get_pointer_sources_of_extern_c_functions(&c_functions_in_file, context);
				sources.extend(extra_sources);
			}
		}
	}

	// Check if we missed any locals
	for pointer in &context.crown_analysis.variables {
		if !sources.iter().any(|source| {
			let path_matches = &source.rust_item_path == pointer.location.path();
			let name_matches = (source.name.starts_with("@") && pointer.name == "@return")
				|| (source.name == pointer.name);

			path_matches && name_matches
		}) {
			println!("We missed a pointer source according to the Crown analysis!");
			println!("{} in {}", pointer.name, pointer.location.path().str());
			unreachable!()
		}
	}

	sources
}

fn get_path_of_module(db: &ide::RootDatabase, module: hir::Module) -> RustItemPath {
	RustItemPath(
		module
			.path_to_root(db)
			.iter()
			.rev()
			.filter_map(|module| {
				if let Some(name) = module.name(db) {
					Some(name.as_str().to_string())
				} else {
					None
				}
			})
			.collect(),
	)
}

fn get_pointer_sources_of_extern_c_functions(
	funcs: &HashSet<CFunctionDeclaration>,
	context: &ProjectContext,
) -> Vec<NamedPointerSource> {
	let mut sources = vec![];
	for func in funcs {
		let return_type = func.function_ast.ret_type().map(|x| x.ty()).flatten();
		let element_type = return_type
			.clone()
			.map(|x| get_element_type_of_pointer(&x, context))
			.flatten();

		match &func.function_name {
			CFunction::Calloc => {
				sources.push(NamedPointerSource {
					is_owned: true,
					is_fat: false,
					is_mut: true,
					location: PointerSourceLocation::FnResult {
						function: func.function,
						pointer_type: return_type.unwrap(),
						element_type: element_type.unwrap(),
						is_extern: true,
					},
					name: format!("calloc"),
					file_id: func.file,
					rust_item_path: RustItemPath::from("extern"),
					_file_location: func._file_location.clone(),
				});
			}
			CFunction::Malloc => {
				sources.push(NamedPointerSource {
					is_owned: true,
					is_fat: false,
					is_mut: true,
					location: PointerSourceLocation::FnResult {
						function: func.function,
						pointer_type: return_type.unwrap(),
						element_type: element_type.unwrap(),
						is_extern: true,
					},
					name: format!("malloc"),
					file_id: func.file,
					rust_item_path: RustItemPath::from("extern"),
					_file_location: func._file_location.clone(),
				});
			}
			CFunction::Strdup | CFunction::Strndup => {
				sources.push(NamedPointerSource {
					is_owned: true,
					is_fat: false,
					is_mut: true,
					location: PointerSourceLocation::FnResult {
						function: func.function,
						pointer_type: return_type.unwrap(),
						element_type: element_type.unwrap(),
						is_extern: true,
					},
					name: if func.function_name == CFunction::Strdup {
						format!("strdup")
					} else {
						format!("strndup")
					},
					file_id: func.file,
					rust_item_path: RustItemPath::from("extern"),
					_file_location: func._file_location.clone(),
				});
			}
			CFunction::Realloc => {
				let return_source = NamedPointerSource {
					is_owned: true,
					is_fat: false,
					is_mut: true,
					location: PointerSourceLocation::FnResult {
						function: func.function,
						pointer_type: return_type.unwrap(),
						element_type: element_type.unwrap(),
						is_extern: true,
					},
					name: format!("realloc"),
					file_id: func.file,
					rust_item_path: RustItemPath::from("extern"),
					_file_location: func._file_location.clone(),
				};
				let old_pointer_param = func
					.function_ast
					.param_list()
					.unwrap()
					.params()
					.nth(0)
					.unwrap();
				let old_pointer_type = old_pointer_param.ty().unwrap();
				let old_pointer_element_type =
					get_element_type_of_pointer(&old_pointer_type, context).unwrap();
				let param_source = NamedPointerSource {
					is_owned: true,
					is_fat: false,
					is_mut: true,
					location: PointerSourceLocation::FnParam {
						param: None,
						param_index: 0,
						function: func.function,
						output_param: false,
						pointer_type: old_pointer_type,
						element_type: old_pointer_element_type,
					},
					name: format!("realloc_old_pointer"),
					file_id: func.file,
					rust_item_path: RustItemPath::from("extern"),
					_file_location: func._file_location.clone(),
				};
				sources.extend([return_source, param_source]);
			}
			CFunction::Free => {
				let pointer_param = func
					.function_ast
					.param_list()
					.unwrap()
					.params()
					.nth(0)
					.unwrap();
				let pointer_type = pointer_param.ty().unwrap();
				let pointer_element_type =
					get_element_type_of_pointer(&pointer_type, context).unwrap();
				sources.push(NamedPointerSource {
					is_owned: true,
					is_fat: false,
					is_mut: true,
					location: PointerSourceLocation::FnParam {
						param: None,
						param_index: 0,
						function: func.function,
						output_param: false,
						pointer_type,
						element_type: pointer_element_type,
					},
					name: format!("free"),
					file_id: func.file,
					rust_item_path: RustItemPath::from("extern"),
					_file_location: func._file_location.clone(),
				});
			}
			_ => {
				continue;
			}
		};
	}
	sources
}

fn get_pointer_sources_of_function(
	context: &ProjectContext,
	func: &hir::Function,
	module_path: &RustItemPath,
) -> Vec<NamedPointerSource> {
	let mut sources = vec![];

	// Get common information
	let function_name = func.name(context.db).as_str().to_string();

	let mut function_path = module_path.clone();
	function_path.0.push(function_name.clone());

	let src = match func.source(context.db) {
		Some(s) => s,
		None => unreachable!(),
	};
	let file_id_editioned = match src.file_id.file_id() {
		Some(id) => id,
		None => unreachable!(), // Skip macro files?
	};
	context.rust_semantics.parse(file_id_editioned);
	let file_id = file_id_editioned.file_id(context.db);
	let line_index = context.rust_analysis.file_line_index(file_id).unwrap();

	let function_file_path = context.vfs.file_path(file_id).to_string();
	let function_file_line_index = line_index.line_col(src.syntax().text_range().start());
	let function_file_location = format!(
		"{}:{}:{}",
		function_file_path,
		function_file_line_index.line + 1,
		function_file_line_index.col + 1
	);
	let function_ast = src.value;

	// First, check return type
	let return_info = context
		.crown_analysis
		.return_pointers()
		.find(|var_info| var_info.location.path() == &function_path);

	if let Some(return_info) = return_info {
		let return_type = function_ast.ret_type().unwrap().ty().unwrap();
		let return_element_type = get_element_type_of_pointer(&return_type, context).unwrap();

		sources.push(NamedPointerSource {
			is_owned: return_info.ownership.is_owned(),
			is_fat: return_info.is_fat,
			is_mut: return_info.is_mutable,
			location: PointerSourceLocation::FnResult {
				function: func.clone(),
				pointer_type: return_type,
				element_type: return_element_type,
				is_extern: false,
			},
			name: format!("@{}", function_name),
			file_id,
			rust_item_path: function_path.clone(),
			_file_location: function_file_location.clone(),
		});
	}

	// Then, check parameters
	if let Some(param_list) = function_ast.param_list() {
		for (param_index, param) in param_list.params().enumerate() {
			let param_line_index = line_index.line_col(param.syntax().text_range().start());

			let parameter_pattern = param.pat().unwrap();
			let ast::Pat::IdentPat(identity_pattern) = &parameter_pattern else {
				// We can skip anonymous parameters: They cannot be used anyways
				continue;
			};
			let parameter_name = identity_pattern.name().unwrap().to_string();
			let param_info = context.crown_analysis.get(&function_path, &parameter_name);
			let Some(param_info) = param_info else {
				// This parameter is not a pointer analysed by Crown. Ignore it
				continue;
			};

			let parameter_source = context.rust_semantics.to_def(identity_pattern).unwrap();
			let parameter_file_location = format!(
				"{}:{}:{}",
				function_file_path,
				param_line_index.line + 1,
				param_line_index.col + 1
			);

			let parameter_type = param.ty().unwrap();
			let parameter_element_type =
				get_element_type_of_pointer(&parameter_type, context).unwrap();

			sources.push(NamedPointerSource {
				is_owned: param_info.ownership.is_owned(),
				is_fat: param_info.is_fat,
				is_mut: param_info.is_mutable,
				location: PointerSourceLocation::FnParam {
					param: Some(parameter_source),
					param_index,
					function: func.clone(),
					output_param: param_info.is_output_param,
					pointer_type: parameter_type,
					element_type: parameter_element_type,
				},
				name: parameter_name,
				file_id,
				rust_item_path: function_path.clone(),
				_file_location: parameter_file_location,
			});
		}
	}

	// Lastly, check let statements
	if let Some(body) = function_ast.body() {
		for let_stmt in body.syntax().descendants().filter_map(ast::LetStmt::cast) {
			if let Some(pat) = let_stmt.pat() {
				let ast::Pat::IdentPat(ident_pat) = &pat else {
					println!("Problem:\n{}", let_stmt.syntax().text().to_string());
					todo!();
				};
				let variable_source = context.rust_semantics.to_def(ident_pat).unwrap();
				let variable_name = ident_pat.name().map(|x| x.to_string()).unwrap();

				let variable_line_index =
					line_index.line_col(ident_pat.syntax().text_range().start());
				let variable_file_location = format!(
					"{}:{}:{}",
					function_file_path,
					variable_line_index.line + 1,
					variable_line_index.col + 1
				);

				let var_info = context.crown_analysis.get(&function_path, &variable_name);

				let (is_owned, is_fat, is_mut) = match var_info {
					Some(info) => (info.ownership.is_owned(), info.is_fat, info.is_mutable),
					None => {
						// Crowns analysis shows no information about this variable
						// This means the variable is not a pointer

						let ty = variable_source.ty(context.db).strip_references();
						if ty.is_raw_ptr() || ty.is_array() {
							(false, false, false)
						} else {
							continue;
						}
					}
				};

				sources.push(NamedPointerSource {
					is_owned,
					is_fat,
					is_mut,
					location: PointerSourceLocation::FnBody {
						variable: variable_source,
						let_statement: let_stmt,
						function: func.clone(),
					},
					name: variable_name,
					file_id,
					rust_item_path: function_path.clone(),
					_file_location: variable_file_location,
				});
			}
		}
	};

	return sources;
}

fn get_pointer_sources_of_struct(
	context: &ProjectContext,
	struc: &hir::Struct,
	module_path: &RustItemPath,
) -> Vec<NamedPointerSource> {
	let mut sources = vec![];

	let struct_name = struc.name(context.db).as_str().to_string();
	let mut struct_path = module_path.clone();

	struct_path.0.push(struct_name.clone());
	let src = match struc.source(context.db) {
		Some(s) => s,
		None => unreachable!(),
	};
	let file_id_editioned = match src.file_id.file_id() {
		Some(id) => id,
		None => unreachable!(), // Skip macro files?
	};
	context.rust_semantics.parse(file_id_editioned);
	let file_id = file_id_editioned.file_id(context.db);
	let line_index = context.rust_analysis.file_line_index(file_id).unwrap();

	let struct_file_path = context.vfs.file_path(file_id).to_string();

	let ast::FieldList::RecordFieldList(field_list) = src.value.field_list().unwrap() else {
		unreachable!("Cannot get fields of tupe");
	};
	for field in field_list.fields() {
		let field_name = field.name().unwrap().to_string();
		let field_line_index = line_index.line_col(field.syntax().text_range().start());

		let field_file_location = format!(
			"{}:{}:{}",
			struct_file_path,
			field_line_index.line + 1,
			field_line_index.col + 1
		);

		let field_info = context.crown_analysis.get(&struct_path, &field_name);
		let Some(field_info) = field_info else {
			// Crown has no information about this struct field: skip it
			continue;
		};
		let hir_field = context.rust_semantics.to_def(&field).unwrap();

		let pointer_type = field.ty().unwrap();
		let element_type = get_element_type_of_pointer(&pointer_type, context).unwrap();

		sources.push(NamedPointerSource {
			is_owned: field_info.ownership.is_owned(),
			is_fat: field_info.is_fat,
			is_mut: field_info.is_mutable,
			location: PointerSourceLocation::StructField {
				field: hir_field,
				struc: struc.clone(),
				pointer_type,
				element_type,
			},
			name: field_name,
			file_id,
			rust_item_path: struct_path.clone(),
			_file_location: field_file_location,
		});
	}

	sources
}

/// from `*T` to `T`
pub fn get_element_type_of_pointer(
	pointer_type: &ast::Type,
	context: &ProjectContext,
) -> Option<ast::Type> {
	match pointer_type {
		ast::Type::PtrType(ptr_type) => Some(ptr_type.ty().unwrap()),
		ast::Type::PathType(path_type) => {
			let path = path_type.path()?;
			let definition = context.rust_semantics.resolve_path(&path)?;
			let hir::PathResolution::Def(module_def) = definition else {
				return None;
			};
			let hir::ModuleDef::TypeAlias(alias) = module_def else {
				return None;
			};
			let alias_source = context.rust_semantics.source(alias)?.value;
			let alias_type = alias_source.ty()?;

			get_element_type_of_pointer(&alias_type, context)
		}
		ast::Type::ArrayType(array_type) => Some(array_type.ty().unwrap()),
		x => todo!("Handle other types-that-hide-that-they-are-pointers {}", x.syntax().text().to_string()),
	}
}
