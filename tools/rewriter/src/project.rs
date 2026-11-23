use std::{
	collections::{HashMap, HashSet},
	fmt, fs,
	io::BufReader,
	path::PathBuf,
};

use base_db::RootQueryDb;
use hir::{Adt, Field, Function, Module, ModuleDef, ScopeDef, Semantics, Struct};
use ide::RootDatabase;
use load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace};
use project_model::{CargoConfig, ProjectManifest, ProjectWorkspace};
use serde::Deserialize;
use vfs::{AbsPathBuf, FileId, Vfs, VfsPath};

#[derive(Debug)]
pub struct HirProject {
	pub db: RootDatabase,
	pub vfs: Vfs,
	pub crown_analysis: CrownAnalysis,
	pub analysis_folder: PathBuf,
	pub project_name: String,
}

impl HirProject {
	pub fn from(cargo_file: &str) -> Self {
		let pb = PathBuf::from(cargo_file)
			.canonicalize()
			.expect("Failed to canonicalize path");

		let project_name = pb
			.parent()
			.unwrap()
			.file_name()
			.unwrap()
			.to_string_lossy()
			.to_string();

		let analysis_folder = pb.parent().unwrap().join("analysis_results");

		let crown_analysis = if analysis_folder.is_dir() {
			// We can add analysis
			CrownAnalysis::from_path(analysis_folder.clone())
		} else {
			panic!(
				"No analysis path found here: {}",
				analysis_folder.to_str().unwrap()
			);
		};

		let apb = AbsPathBuf::try_from(pb.to_str().unwrap()).unwrap();

		let manifest =
			ProjectManifest::from_manifest_file(apb).expect("Could not load manifest file");

		let workspace =
			ProjectWorkspace::load(manifest, &CargoConfig::default(), &|_: String| {}).unwrap();

		let load_cargo_config = LoadCargoConfig {
			load_out_dirs_from_check: true,
			with_proc_macro_server: ProcMacroServerChoice::None,
			prefill_caches: false,
			proc_macro_processes: 1,
		};

		let (db, vfs, _) = load_workspace(workspace, &Default::default(), &load_cargo_config)
			.expect("Failed to load workspace");

		Self {
			db,
			vfs,
			crown_analysis,
			analysis_folder,
			project_name,
		}
	}

	pub fn get_local_modules(&self) -> Vec<Module> {
		let local_crates: Vec<hir::Crate> = self
			.db
			.all_crates()
			.to_vec()
			.into_iter()
			.filter_map(|krate| {
				if krate.data(&self.db).origin.is_local() {
					// let hir_crate = hir::Crate { id: krate };
					let hir_crate2 = hir::Crate::from(krate);
					Some(hir_crate2)
				} else {
					None
				}
			})
			.collect();

		let mut found_modules: Vec<Module> = vec![];
		let mut modules_left_to_check: Vec<Module> = vec![];

		local_crates.iter().for_each(|krate| {
			let root_module = krate.root_module(&self.db);
			modules_left_to_check.push(root_module);
			// modules.push(root_module);
			// let mut modules_left_stack = vec![root_module];
			// while let Some(module) = modules_left_stack.pop() {
			// 	let file_id = module.definition_source_file_id(&self.db);
			// 	files.push(file_id.file_id().unwrap().file_id(&self.db));
			// 	for child in module.children(&self.db) {
			// 		modules_left_stack.push(child);
			// 	}
			// }
		});

		while let Some(module) = modules_left_to_check.pop() {
			found_modules.push(module);
			module.children(&self.db).for_each(|child| {
				modules_left_to_check.push(child);
			});
		}

		found_modules
	}

	pub fn get_file_id_of_filepath(&self, path: &str) -> FileId {
		let pb = PathBuf::from(path).canonicalize().unwrap();
		let path = VfsPath::new_real_path(pb.to_str().unwrap().to_string());
		let (file_id, _) = self.vfs.file_id(&path).unwrap();
		file_id
	}

	pub fn find_function_in_file(&self, file_id: FileId, function_name: &str) -> Function {
		let semantics = Semantics::new(&self.db);
		let module = semantics.file_to_module_def(file_id).unwrap();
		for (name, scope) in module.scope(&self.db, None) {
			if name.as_str() == function_name {
				if let ScopeDef::ModuleDef(ModuleDef::Function(function)) = scope {
					return function;
				}
			}
		}
		unreachable!()
	}

	pub fn find_struct_in_file(&self, file_id: FileId, struct_name: &str) -> Struct {
		let semantics = Semantics::new(&self.db);
		let module = semantics.file_to_module_def(file_id).unwrap();
		for (name, scope) in module.scope(&self.db, None) {
			if name.as_str() == struct_name {
				if let ScopeDef::ModuleDef(ModuleDef::Adt(Adt::Struct(strukt))) = scope {
					return strukt;
				}
			}
		}
		unreachable!()
	}

	fn find_variable_in_struct(&self, strukt: Struct, variable_name: &str) -> Field {
		for field in strukt.fields(&self.db) {
			if field.name(&self.db).as_str() == variable_name {
				return field;
			}
		}
		unreachable!()
	}
}

#[derive(Debug, Deserialize, Clone)]
pub struct OutputParamFile {
	pub fn_data: HashMap<String, Vec<String>>,
}
impl OutputParamFile {
	fn from_file(path: PathBuf) -> Self {
		let file_content = fs::File::open(path).expect("Failed to open file");
		let reader = BufReader::new(file_content);
		serde_json::from_reader(reader).expect("Failed")
	}
}

#[derive(Debug, Deserialize, Clone)]
pub struct AnalysisFile {
	pub fn_data: HashMap<String, HashMap<String, Vec<String>>>,
	pub struct_data: HashMap<String, HashMap<String, Vec<String>>>,
}

impl AnalysisFile {
	fn from_file(path: PathBuf) -> Self {
		let file_content = fs::File::open(path).expect("Failed to open file");
		let reader = BufReader::new(file_content);
		serde_json::from_reader(reader).expect("Failed")
	}
}

#[derive(Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct RustItemPath(pub Vec<String>);
impl fmt::Debug for RustItemPath {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let path = if self.0.is_empty() {
			"<root>".to_string()
		} else {
			self.0.join("::")
		};
		write!(f, "{path}")
	}
}

/// Represents a "src::buffer::main" path.
/// Includes both rust source file, module path and name of function/struct
impl RustItemPath {
	pub fn from(path_string: &str) -> Self {
		Self(
			path_string
				.split("::")
				.filter_map(|x| {
					if x.is_empty() {
						None
					} else {
						Some(x.to_string())
					}
				})
				.collect(),
		)
	}

	pub fn str(&self) -> String {
		self.0.join("::")
	}

	/// Gets the name of the object this path points to, a.k.a. the last segment:
	/// `['a', 'b', 'fun_name']` -> `'fun_name'`
	pub fn get_name(&self) -> String {
		self.0.last().expect("No name in path").clone()
	}

	/// Tests if the given segments, like `['a', 'b']` match the current path
	/// excluding the struct/function name `['a', 'b', 'fun_a']`
	pub fn is_directly_in(&self, segments: &[String]) -> bool {
		let mut without_last = self.0.clone();
		without_last.pop();

		without_last == segments
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum VarLocation {
	Function(RustItemPath),
	FunctionResult(RustItemPath),
	Struct(RustItemPath),
}
impl VarLocation {
	pub fn path(&self) -> &RustItemPath {
		match self {
			VarLocation::FunctionResult(rust_item_path)
			| VarLocation::Function(rust_item_path)
			| VarLocation::Struct(rust_item_path) => rust_item_path,
		}
	}
	pub fn is_function_result(&self) -> bool {
		matches!(self, VarLocation::FunctionResult(_))
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum VarOwnership {
	Owned,
	Transient,
	Unknown,
}
impl VarOwnership {
	pub fn is_owned(&self) -> bool {
		matches!(self, VarOwnership::Owned)
	}
	pub fn str(&self) -> &str {
		match self {
			VarOwnership::Owned => "Owned",
			VarOwnership::Transient => "Transient",
			VarOwnership::Unknown => "Unknown",
		}
	}
}
impl From<&str> for VarOwnership {
	fn from(value: &str) -> Self {
		match value {
			"Owning" => Self::Owned,
			"Transient" => Self::Transient,
			"Unknown" => Self::Unknown,
			_ => unreachable!(),
		}
	}
}

type VarIsFat = bool;
type VarIsMutable = bool;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VarInfo {
	pub name: String,
	pub location: VarLocation,
	pub ownership: VarOwnership,
	pub is_fat: VarIsFat,
	pub is_mutable: VarIsMutable,
	pub is_output_param: bool,
	pub nested: Option<Box<VarInfo>>,
}

#[derive(Debug)]
pub struct CrownAnalysis {
	pub variables: HashSet<VarInfo>,
}

impl CrownAnalysis {
	pub fn arrays(&self) -> impl Iterator<Item = &VarInfo> {
		self.variables.iter().filter(|info| info.is_fat)
	}

	pub fn get(&self, item_path: &RustItemPath, name: &str) -> Option<&VarInfo> {
		self.variables
			.iter()
			.find(|var_info| var_info.location.path() == item_path && var_info.name == name)
	}

	pub fn functions(&self) -> impl Iterator<Item = &RustItemPath> {
		let mut seen = HashSet::new();
		self.variables.iter().filter_map(move |info| {
			if let VarLocation::Function(path) = &info.location {
				seen.insert(path).then_some(path)
			} else {
				None
			}
		})
	}
	pub fn structs(&self) -> impl Iterator<Item = &RustItemPath> {
		let mut seen = HashSet::new();
		self.variables.iter().filter_map(move |info| {
			if let VarLocation::Function(path) = &info.location {
				seen.insert(path).then_some(path)
			} else {
				None
			}
		})
	}
	pub fn arrays_in_item<'a>(
		&'a self,
		item: &'a RustItemPath,
	) -> impl Iterator<Item = &'a VarInfo> {
		self.variables
			.iter()
			.filter(move |info| info.is_fat && info.location.path() == item)
	}
	pub fn return_pointers(&self) -> impl Iterator<Item = &VarInfo> {
		self.variables
			.iter()
			.filter(|info| info.location.is_function_result())
	}

	/// From a given map between a path to an Item (function or struct), mapped to
	/// the name of the item, which is in turn mapped to a list of attributes,
	/// it binds the fatness, ownership and mutability properties together
	fn merge_analyses(
		fatness: HashMap<String, HashMap<String, Vec<String>>>,
		ownership: HashMap<String, HashMap<String, Vec<String>>>,
		mutability: HashMap<String, HashMap<String, Vec<String>>>,
	) -> HashMap<RustItemPath, HashMap<String, Vec<(VarIsFat, VarOwnership, VarIsMutable)>>> {
		let all_paths: HashSet<String> = fatness
			.keys()
			.cloned()
			.chain(ownership.keys().cloned())
			.chain(mutability.keys().cloned())
			.collect();

		let mut result = HashMap::new();
		for path in all_paths {
			let all_vars: HashSet<String> = fatness[&path]
				.keys()
				.cloned()
				.chain(ownership[&path].keys().cloned())
				.chain(mutability[&path].keys().cloned())
				.collect();
			let mut var_values = HashMap::new();
			for var in all_vars {
				assert_eq!(fatness[&path][&var].len(), ownership[&path][&var].len());
				assert_eq!(ownership[&path][&var].len(), mutability[&path][&var].len());

				let triples = fatness[&path][&var]
					.iter()
					.zip(&ownership[&path][&var])
					.zip(&mutability[&path][&var])
					.map(|((fatness, ownership), mutability)| {
						(
							if fatness == "Arr" {
								true
							} else if fatness == "Ptr" {
								false
							} else {
								unreachable!()
							},
							VarOwnership::from(ownership.as_str()),
							if mutability == "Mut" {
								true
							} else if mutability == "Imm" {
								false
							} else {
								unreachable!()
							},
						)
					})
					.collect();
				var_values.insert(var, triples);
			}
			result.insert(RustItemPath::from(&path), var_values);
		}
		result
	}

	/// Loads the analysis data from the given folder path
	fn from_path(path: PathBuf) -> Self {
		let fatness: AnalysisFile = AnalysisFile::from_file(path.join("fatness.json")).into();
		let ownership: AnalysisFile = AnalysisFile::from_file(path.join("ownership.json")).into();
		let mutability: AnalysisFile = AnalysisFile::from_file(path.join("mutability.json")).into();

		let output_params: OutputParamFile =
			OutputParamFile::from_file(path.join("output_params.json")).into();

		Self::from_analysis_files(fatness, ownership, mutability, output_params)
	}

	fn from_analysis_files(
		fatness: AnalysisFile,
		ownership: AnalysisFile,
		mutability: AnalysisFile,
		output_params: OutputParamFile,
	) -> Self {
		let fn_data = Self::merge_analyses(fatness.fn_data, ownership.fn_data, mutability.fn_data);
		let struct_data = Self::merge_analyses(
			fatness.struct_data,
			ownership.struct_data,
			mutability.struct_data,
		);

		let mut variables = HashSet::new();

		for (fn_path, vars) in &fn_data {
			for (var, triples) in vars {
				if var == "@return" || triples.is_empty() {
					continue;
				}
				let var_is_output_parameter = output_params
					.fn_data
					.get(&fn_path.str())
					.expect("Function not present in output param analysis")
					.contains(var);

				let var_info = triples
					.iter()
					.map(|(fatness, ownership, is_mutable)| VarInfo {
						location: VarLocation::Function(fn_path.clone()),
						ownership: ownership.clone(),
						is_fat: *fatness,
						is_mutable: *is_mutable,
						nested: None,
						name: var.clone(),
						is_output_param: var_is_output_parameter,
					})
					.into_iter()
					.rfold(None, |inner, mut info| {
						info.nested = inner.map(Box::new);
						Some(info)
					});
				let Some(var_info) = var_info else {
					panic!("Failed to get var info for: {} -> {}", fn_path.str(), var);
				};
				variables.insert(var_info);
			}
		}
		for (struct_path, vars) in &struct_data {
			for (var, triples) in vars {
				if triples.is_empty() {
					continue;
				}
				let var_info = triples
					.iter()
					.map(|(fatness, ownership, is_mutable)| VarInfo {
						location: VarLocation::Struct(struct_path.clone()),
						ownership: ownership.clone(),
						is_fat: *fatness,
						is_mutable: *is_mutable,
						nested: None,
						name: var.clone(),
						is_output_param: false,
					})
					.into_iter()
					.rfold(None, |inner, mut info| {
						info.nested = inner.map(Box::new);
						Some(info)
					});
				variables.insert(var_info.unwrap());
			}
		}

		let mut return_values = HashSet::new();
		for (fn_path, vars) in &fn_data {
			for (var, triples) in vars {
				if var != "@return" {
					continue;
				}
				let return_info = triples
					.iter()
					.map(|(fatness, ownership, is_mutable)| VarInfo {
						ownership: ownership.clone(),
						is_mutable: *is_mutable,
						is_fat: *fatness,
						nested: None,
						name: format!("@return"),
						location: VarLocation::FunctionResult(fn_path.clone()),
						is_output_param: false,
					})
					.into_iter()
					.rfold(None, |inner, mut info| {
						info.nested = inner.map(Box::new);
						Some(info)
					});
				return_values.insert(return_info.unwrap());
			}
		}
		variables.extend(return_values);

		Self { variables }
	}
}
