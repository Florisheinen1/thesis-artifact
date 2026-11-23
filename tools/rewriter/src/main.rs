use std::{
	collections::{HashMap, HashSet},
	fs,
	path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use ide::RootDatabase;
use serde::Serialize;
use syntax::AstNode;
use vfs::Vfs;

use crate::{
	context_rewrite_planner::{
		ContextSwitchRewritePlan, TypeRewritePlan, plan_rewrite_for_context_switch,
		plan_rewrite_for_pointer_source,
	},
	effective_usage_type::{EffectiveUsageTypeVariant, get_effective_usage_type},
	extern_function::{CFunctionDeclaration, get_all_external_function_decls},
	pointer_interactions::get_all_pointer_interactions,
	pointer_source::{NamedPointerSource, PointerSourceLocation, get_all_pointer_sources},
	project::{CrownAnalysis, HirProject},
	rewrite_planner::{RewriteOutcome, get_rewrite_for_pointer_interaction},
	rewriter::{
		Rewrite, get_rewrite_for_source_plan,
		get_rewrites_for_runtime_installation, get_rewrites_for_struct_attributes, write_rewrites,
	},
	runtime_install::install_runtime,
};

mod context_rewrite_planner;
mod effective_usage_type;
mod extern_function;
mod pointer_context;
mod pointer_interactions;
mod pointer_source;
mod project;
mod rewrite_planner;
mod rewriter;
mod runtime_install;

#[derive(Parser)]
#[command(about = "Analyze owned arrays in C-to-Rust translated code")]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Analyze benchmark(s)
	Analyze {
		#[command(subcommand)]
		target: Target,
	},
	/// Rewrite benchmark(s)
	Rewrite {
		#[command(subcommand)]
		target: Target,
	},
	/// Test heman
	Test {
		#[command(subcommand)]
		target: Target,
	},
}

#[derive(Subcommand)]
enum Target {
	/// Operate on a single benchmark project
	One {
		/// Path to the project folder (must contain a Cargo.toml)
		path: PathBuf,
	},
	/// Operate on all benchmark projects in a folder
	All {
		/// Path to the folder containing benchmark projects
		path: PathBuf,
	},
}

pub struct ProjectContext<'a> {
	pub vfs: &'a Vfs,
	pub crown_analysis: &'a CrownAnalysis,
	pub db: &'a RootDatabase,
	pub rust_analysis: &'a ide::Analysis,
	pub rust_semantics: &'a ide::Semantics<'a, RootDatabase>,
}

fn resolve_target(target: Target) -> Vec<PathBuf> {
	match target {
		Target::One { path } => vec![path.join("Cargo.toml")],
		Target::All { path } => collect_cargo_tomls(&path, &[]),
		// Target::All { path } => collect_cargo_tomls(&path, &[PathBuf::from("../input/brotli")]),
	}
}

fn main() {
	let cli = Cli::parse();
	match cli.command {
		Command::Analyze { target } => {
			let project_paths = resolve_target(target);
			analyze_projects(&project_paths);
		}
		Command::Rewrite { target } => {
			let project_paths = resolve_target(target);
			rewrite_projects(&project_paths);
		}
		Command::Test { target } => {
			let project_paths = resolve_target(target);
			test_project(&project_paths);
		}
	}
}

fn test_project(project_paths: &[PathBuf]) {
	let projects: Vec<HirProject> = project_paths
		.iter()
		.map(|path| HirProject::from(path.to_str().unwrap()))
		.collect();

	let project = projects.into_iter().next().unwrap();

	let analysis_host = ide::AnalysisHost::with_database(project.db);
	let db = analysis_host.raw_database();
	hir::attach_db(db, || {
		let project_context = ProjectContext {
			vfs: &project.vfs,
			crown_analysis: &project.crown_analysis,
			db,
			rust_analysis: &analysis_host.analysis(),
			rust_semantics: &hir::Semantics::new(db),
		};

		// Probe: does `resolve_method_call` succeed for raw-pointer inherent
		// methods like `.offset_from()`, vs. a normal user-defined method,
		// in the same file?
		for (file_id, vfs_path) in project_context.vfs.iter() {
			let file_path = vfs_path.to_string();
			if !file_path.ends_with("libtree.rs") {
				continue;
			}

			let edition_file_id =
				hir::EditionedFileId::current_edition_guess_origin(project_context.db, file_id);
			let hir_file_id = hir::HirFileId::from(edition_file_id);
			let file_syntax = project_context.rust_semantics.parse_or_expand(hir_file_id);
			let line_index = project_context
				.rust_analysis
				.file_line_index(file_id)
				.unwrap();

			for method_call in file_syntax
				.descendants()
				.filter_map(syntax::ast::MethodCallExpr::cast)
			{
				let Some(name_ref) = method_call.name_ref() else {
					continue;
				};
				let name = name_ref.text().to_string();
				if name != "offset_from" && name != "offset" {
					continue;
				}

				let pos = line_index.line_col(method_call.syntax().text_range().start());
				let resolved = project_context
					.rust_semantics
					.resolve_method_call(&method_call);

				println!(
					"{}:{}:{}  .{}(..)  resolve_method_call = {}   text = `{}`",
					file_path,
					pos.line + 1,
					pos.col + 1,
					name,
					resolved.is_some(),
					method_call.syntax().text(),
				);
			}
		}
	})
}

fn rewrite_projects(project_paths: &[PathBuf]) {
	let projects: Vec<HirProject> = project_paths
		.iter()
		.map(|path| HirProject::from(path.to_str().unwrap()))
		.collect();
	println!("Rewriting {} project(s)", projects.len());

	for project in projects {
		let analysis_folder = project.analysis_folder.clone();

		let analysis_host = ide::AnalysisHost::with_database(project.db);
		let db = analysis_host.raw_database();

		hir::attach_db(db, || {
			let project_context = ProjectContext {
				vfs: &project.vfs,
				crown_analysis: &project.crown_analysis,
				db,
				rust_analysis: &analysis_host.analysis(),
				rust_semantics: &hir::Semantics::new(db),
			};

			rewrite_project(&project_context, &analysis_folder);
		})
	}
}

fn analyze_projects(project_paths: &[PathBuf]) {
	let projects: Vec<HirProject> = project_paths
		.iter()
		.map(|path| HirProject::from(path.to_str().unwrap()))
		.collect();

	println!("Analyzing {} project(s)", projects.len());

	for project in projects {
		let analysis_folder = project.analysis_folder.clone();

		let analysis_host = ide::AnalysisHost::with_database(project.db);
		let db = analysis_host.raw_database();

		hir::attach_db(db, || {
			let project_context = ProjectContext {
				vfs: &project.vfs,
				crown_analysis: &project.crown_analysis,
				db,
				rust_analysis: &analysis_host.analysis(),
				rust_semantics: &hir::Semantics::new(db),
			};

			analyze_project(&project_context, &analysis_folder);
		})
	}
}

/// Analyzes a project: How arrays are used
/// Stores the results in the same analysis folder that Crown already made
fn analyze_project(context: &ProjectContext, analysis_folder: &PathBuf) {
	let project_name = analysis_folder.to_string_lossy().to_string();
	println!("Analyzing {project_name}");

	let all_c_functions: HashMap<vfs::FileId, HashSet<CFunctionDeclaration>> =
		get_all_external_function_decls(context);

	let all_pointer_sources: Vec<NamedPointerSource> =
		get_all_pointer_sources(context, &all_c_functions);

	// Get a bunch of stats on pointer sources
	// 1. How many arrays are there, per benchmark
	// 2. What is the owned/non-owned ratio in arrays

	#[derive(Debug, Serialize)]
	struct NamedPointerSourceRecord {
		is_owned: bool,
		is_fat: bool,
		is_mut: bool,
		/// Output parameters are left out of the rewrite, even when owned
		is_output_param: bool,
		name: String,
		source_type: String, // FnResult, FnParam, FnBody or StructField
	}

	let source_records: Vec<NamedPointerSourceRecord> = all_pointer_sources
		.iter()
		.map(|source| NamedPointerSourceRecord {
			is_owned: source.is_owned,
			is_fat: source.is_fat,
			is_mut: source.is_mut,
			is_output_param: source.is_output_param(),
			name: source.name.clone(),
			source_type: get_source_type_name(source),
		})
		.collect();

	write_json_records(&source_records, analysis_folder, "pointer_sources.json");

	// let owned_array_sources: Vec<NamedPointerSource> = all_pointer_sources
	// 	.iter()
	// 	.cloned()
	// 	.into_iter()
	// 	.filter(|source| source.is_owned && source.is_fat)
	// 	.collect();

	#[derive(Debug, Serialize)]
	struct ArrayUsageRecord {
		source_is_owned: bool,
		source_is_fat: bool,
		source_is_output_param: bool,
		is_direct: bool,
		usage_type: String, // StoreElsewhere, ReferenceRawArray, NullCheck, etc.
		source_name: String,
		location: String,
	}

	let array_interactions = get_all_pointer_interactions(&all_pointer_sources, context, &all_c_functions);

	let all_effective_usages: Vec<ArrayUsageRecord> = array_interactions
		.iter()
		.map(|usage| {
			// An `Err` means the classifier does not support the usage's shape,
			// which is different from a usage that is `Unused`
			let usage_type_name = get_effective_usage_type(usage, context, &all_c_functions)
				.map(|usage_type| usage_type.variant.str())
				.unwrap_or(format!("Unclassified"));

			ArrayUsageRecord {
				source_is_owned: usage.source.is_owned,
				source_is_fat: usage.source.is_fat,
				source_is_output_param: usage.source.is_output_param(),
				usage_type: usage_type_name,
				is_direct: usage.is_direct_usage,
				source_name: usage.source.name.clone(),
				location: usage._file_location.clone(),
			}
		})
		.collect();

	write_json_records(&all_effective_usages, analysis_folder, "array_usages.json");

	// Get stats on usages (on both owned/non-owned arrays)
	// 1. How often element accessed (this is our golden chance)
	// 2. How often deallocated
	// 3. How often reallocated
	// etc.

	// - Don't care about casts

	// Maybe: Also get stats on pointer switches
	// - Box to Box
	// - Box to Const
	// - etc.
	// Purpose: See some simply do not occur
}

fn get_source_type_name(source: &NamedPointerSource) -> String {
	match &source.location {
		PointerSourceLocation::FnResult { .. } => format!("FnResult"),
		PointerSourceLocation::FnParam { .. } => format!("FnParam"),
		PointerSourceLocation::FnBody { .. } => format!("FnBody"),
		PointerSourceLocation::StructField { .. } => format!("StructField"),
	}
}

/// Serializes `records` as pretty-printed JSON and writes them to
/// `analysis_folder/file_name`, alongside Crown's own analysis output.
fn write_json_records<T: Serialize>(records: &[T], analysis_folder: &Path, file_name: &str) {
	let output_path = analysis_folder.join(file_name);
	let file = fs::File::create(&output_path)
		.unwrap_or_else(|e| panic!("Failed to create {}: {e}", output_path.display()));
	serde_json::to_writer_pretty(file, records)
		.unwrap_or_else(|e| panic!("Failed to write {}: {e}", output_path.display()));
	println!(
		"Wrote {} record(s) to {}",
		records.len(),
		output_path.display()
	);
}

fn rewrite_project(context: &ProjectContext, analysis_folder: &Path) {
	println!("Rewriting...");

	// Collect all external c functions
	let all_c_functions: HashMap<vfs::FileId, HashSet<CFunctionDeclaration>> =
		get_all_external_function_decls(context);

	// Collect pointer sources
	let all_pointer_sources: Vec<NamedPointerSource> =
		get_all_pointer_sources(context, &all_c_functions);

	// Filter owned array pointers to rewrite
	// (Output params are excluded as well)
	let owned_array_sources: Vec<NamedPointerSource> = all_pointer_sources
		.iter()
		.cloned()
		.into_iter()
		.filter(|source| source.is_owned && source.is_fat && !source.is_output_param())
		.collect();

	// Collect all (direct and indirect) usages of owned arrays
	let mut owned_array_interactions = get_all_pointer_interactions(&owned_array_sources, context, &all_c_functions);

	owned_array_interactions.sort_by_key(|x| x._file_location.clone());

	for usage in &owned_array_interactions {
		println!(
			"\nLoc: {}\nVal: {}\nSrc: {}\nDirect: {}",
			usage._file_location, usage.value, usage.source.name, usage.is_direct_usage
		)
	}

	// Collect rewrite plans for sources
	let source_rewrite_plans: Vec<TypeRewritePlan> = owned_array_sources
		.iter()
		.map(|source| plan_rewrite_for_pointer_source(source, context))
		.flatten()
		.collect();

	// let mut context_switch_plans: Vec<ContextSwitchRewritePlan> = vec![];
	// for usage in &owned_array_usages {
	// 	let plan =
	// 		plan_rewrite_for_context_switch(usage, context, &all_pointer_sources, &all_c_functions);
	// 	context_switch_plans.push(plan);
	// }

	// Install the CBox and allocation runtime dependencies
	let runtime_paths = install_runtime(context);

	let mut all_files_that_need_cbox: HashSet<vfs::FileId> = HashSet::new();

	for usage in &owned_array_interactions {
		all_files_that_need_cbox.insert(usage.file_id);
		all_files_that_need_cbox.insert(usage.source.file_id);
	}

	let mut rewrites: Vec<Rewrite> = vec![];

	rewrites.extend(get_rewrites_for_runtime_installation(
		&runtime_paths,
		all_files_that_need_cbox,
		context,
		&all_c_functions,
	));

	for plan in &source_rewrite_plans {
		rewrites.push(get_rewrite_for_source_plan(&plan))
	}
	// What happened to each interaction, so the safety gain can be counted
	#[derive(Debug, Serialize)]
	struct RewriteOutcomeRecord {
		source_name: String,
		source_type: String, // FnResult, FnParam, FnBody or StructField
		usage_type: String,  // StoreElsewhere, DereferenceElement, etc.
		is_direct: bool,
		inner_context: Option<String>,
		outer_context: Option<String>,
		outcome: RewriteOutcome,
		promoted: bool,
		location: String,
	}
	let mut outcome_records: Vec<RewriteOutcomeRecord> = vec![];

	for interaction in &owned_array_interactions {
		let interaction_rewrite = get_rewrite_for_pointer_interaction(
			interaction,
			context,
			&all_c_functions,
			&all_pointer_sources,
		);

		let usage_type = get_effective_usage_type(interaction, context, &all_c_functions)
			.map(|usage_type| usage_type.variant.str())
			.unwrap_or(format!("Unclassified"));
		outcome_records.push(RewriteOutcomeRecord {
			source_name: interaction.source.name.clone(),
			source_type: get_source_type_name(&interaction.source),
			usage_type,
			is_direct: interaction.is_direct_usage,
			inner_context: interaction_rewrite.contexts.as_ref().map(|(inner, _)| inner.str()),
			outer_context: interaction_rewrite.contexts.as_ref().map(|(_, outer)| outer.str()),
			outcome: interaction_rewrite.outcome,
			promoted: interaction_rewrite.promoted,
			location: interaction._file_location.clone(),
		});

		if let Some(rewrite) = interaction_rewrite.rewrite {
			rewrites.push(rewrite);
		}
	}

	write_json_records(&outcome_records, analysis_folder, "rewrite_outcomes.json");

	// TODO: Special case rewrites
	// - Reallocs
	// - Deallocs
	// - Element access
	// - Nullpointer creation

	// And rewrites for struct Copy and Clone attribute removal
	rewrites.extend(get_rewrites_for_struct_attributes(
		&owned_array_sources,
		context,
	));

	write_rewrites(rewrites, context);
}

pub fn collect_cargo_tomls(root: &Path, skip_projects: &[PathBuf]) -> Vec<PathBuf> {
	let mut result = Vec::new();

	let Ok(entries) = fs::read_dir(root) else {
		return result;
	};

	for entry in entries.flatten() {
		let path = entry.path();

		if !path.is_dir() {
			continue;
		}

		if skip_projects.iter().any(|skip| {
			let canonical_skip = fs::canonicalize(skip).unwrap_or(skip.clone());
			let canonical_path = fs::canonicalize(&path).unwrap_or(path.clone());
			canonical_skip == canonical_path
		}) {
			println!("Skipping {path:?}");
			continue;
		}

		let cargo_toml = path.join("Cargo.toml");

		if cargo_toml.exists() {
			result.push(cargo_toml);
		}
	}

	result
}
