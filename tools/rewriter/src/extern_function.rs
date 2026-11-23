use std::collections::{HashMap, HashSet};

use hir::EditionedFileId;
use syntax::{
	AstNode,
	ast::{self, HasModuleItem, HasName},
};

use crate::ProjectContext;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CFunctionDeclaration {
	pub file: vfs::FileId,
	pub function_name: CFunction,
	pub function: hir::Function,
	pub decl: syntax::SyntaxNode,
	pub function_ast: ast::Fn,
	pub _file_location: String,
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub enum CFunction {
	Memcpy,
	Memmove,
	Memset,
	Strncat,
	Strcmp,
	Strstr,
	Strlen,
	Printf,
	Vsnprintf,
	Malloc,
	Calloc,
	Realloc,
	Free,
	Strdup,
	Strndup,
}

impl CFunction {
	fn from_name(value: &str) -> Option<Self> {
		match value {
			"memcpy" => Some(Self::Memcpy),
			"memmove" => Some(Self::Memmove),
			"memset" => Some(Self::Memset),
			"strncat" => Some(Self::Strncat),
			"strcmp" => Some(Self::Strcmp),
			"strstr" => Some(Self::Strstr),
			"strlen" => Some(Self::Strlen),
			"printf" => Some(Self::Printf),
			"vsnprintf" => Some(Self::Vsnprintf),
			"malloc" => Some(Self::Malloc),
			"calloc" => Some(Self::Calloc),
			"realloc" => Some(Self::Realloc),
			"free" => Some(Self::Free),
			"strdup" => Some(Self::Strdup),
			"strndup" => Some(Self::Strndup),
			_ => None,
		}
	}
}

pub fn get_all_external_function_decls(
	context: &ProjectContext,
) -> HashMap<vfs::FileId, HashSet<CFunctionDeclaration>> {
	let mut all_function_decls: HashMap<vfs::FileId, HashSet<CFunctionDeclaration>> =
		HashMap::new();

	for krate in hir::Crate::all(context.db) {
		if !krate.origin(context.db).is_local() {
			continue;
		}

		for module in krate.modules(context.db) {
			let file = module.definition_source_file_id(context.db);
			let editioned_file_id = file.file_id().unwrap();
			let file_id = editioned_file_id.file_id(context.db);
			// let source_file = context.rust_semantics.parse(editioned_file_id);

			let new_decls = get_external_c_functions(context, &editioned_file_id);

			for new_decl in new_decls {
				all_function_decls
					.entry(file_id)
					.or_default()
					.insert(new_decl);
			}
		}
	}

	all_function_decls
}

/// Gets a map of all the external C functions declared at the top of a
/// translated file
pub fn get_external_c_functions(
	context: &ProjectContext,
	file: &EditionedFileId,
) -> Vec<CFunctionDeclaration> {
	let mut functions = vec![];
	let file_id = file.file_id(context.db);

	let source_file = context.rust_semantics.parse(*file);

	let line_index = context.rust_analysis.file_line_index(file_id).unwrap();
	let function_file_path = context.vfs.file_path(file_id).to_string();

	for item in source_file.items() {
		if let ast::Item::ExternBlock(external_block) = item {
			if let Some(external_block) = external_block.extern_item_list() {
				for external_item in external_block.extern_items() {
					if let ast::ExternItem::Fn(extern_function) = external_item {
						let Some(name) = extern_function.name() else {
							// Extern function had no name
							continue;
						};
						let Some(c_name) = CFunction::from_name(&name.text()) else {
							continue;
						};

						let line_col = line_index.line_col(extern_function.syntax().text_range().start());
						let function_file_location = format!("{}:{}:{}", function_file_path, line_col.line + 1, line_col.col + 1);

						let hir_function = context
							.rust_semantics
							.to_fn_def(&extern_function)
							.expect("Failed to get hir from extern function");

						functions.push(CFunctionDeclaration {
							file: file_id.clone(),
							function_name: c_name,
							function: hir_function,
							decl: extern_function.syntax().clone(),
							function_ast: extern_function,
							_file_location: function_file_location,
						});
					}
				}
			}
		}
	}
	functions
}
