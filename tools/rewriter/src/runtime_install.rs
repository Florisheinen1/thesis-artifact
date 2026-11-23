use std::{collections::HashSet, fs};

use base_db::{Crate, RootQueryDb};

use crate::ProjectContext;

const RUNTIME_CBOX_SRC: &str = include_str!("../runtime/src/cbox.rs");
const RUNTIME_ALLOCATION_SRC: &str = include_str!("../runtime/src/allocation.rs");

pub struct RuntimePaths {
	pub cbox_base_path: String,
	pub cbox_module_name: String,
	pub alloc_base_path: String,
	pub alloc_module_name: String,
	pub root_file: vfs::FileId,
}

/// Copies the cbox runtime (`cbox.rs` + `allocation.rs`) next to the crate root of
/// the project being rewritten and declares both modules in the root.
/// Returns the import paths of both files (`crate::cbox::CBox` and `crate::cbox::allocation`)
///
/// Panics if the runtime already appears to be installed (either the files already
/// exist, or the crate root already declares `mod cbox`/`mod allocation`)
pub fn install_runtime(context: &ProjectContext) -> RuntimePaths {
	let root_file_id = get_root_file(context);
	let vfs_path = context.vfs.file_path(root_file_id);
	let abs_path = vfs_path.as_path().expect("Non-filesystem path in vfs");
	let dir = abs_path
		.parent()
		.expect("Crate root file has no parent directory");

	write_runtime_file(&dir.join("cbox.rs"), RUNTIME_CBOX_SRC);
	write_runtime_file(&dir.join("allocation.rs"), RUNTIME_ALLOCATION_SRC);

	RuntimePaths {
		cbox_base_path: format!("crate::cbox"),
		cbox_module_name: format!("cbox"),
		alloc_base_path: format!("crate::allocation"),
		alloc_module_name: format!("allocation"),
		root_file: root_file_id,
	}
}

pub fn get_root_file(context: &ProjectContext) -> vfs::FileId {
	let root_module = find_local_root_module(context);
	let hir_file_id = root_module.definition_source_file_id(context.db);
	let file_id_editioned = hir_file_id
		.file_id()
		.expect("Crate root is not a real file (unexpected macro expansion)");
	let crate_root_file_id = file_id_editioned.file_id(context.db);
	return crate_root_file_id;
}

fn write_runtime_file(path: &paths::AbsPath, contents: &str) {
	if std::fs::exists(path).unwrap_or(false) {
		panic!("{path} already exists; refusing to overwrite it with the cbox runtime");
	}
	fs::write(path, contents).unwrap_or_else(|e| panic!("Failed to write {path}: {e}"));
}

/// Finds the root module of the project's local library crate.
///
/// A project can have more than one local crate:
/// - a `build.rs` gets compiled as its own local crate (seen in every benchmark
///   here, since they all have a build script), but never contains C2Rust output.
/// - a `test` binary target that links against the `[lib]` target (seen in
///   `benchmarks_testable/buffer`).
///
/// C2Rust always puts the translated code in the `[lib]` target, so we first drop
/// any local crate rooted at `build.rs`, then — if more than one candidate is left
/// — pick whichever remaining crate the others depend on.
fn find_local_root_module(context: &ProjectContext) -> hir::Module {
	let local_crates: Vec<Crate> = context
		.db
		.all_crates()
		.iter()
		.copied()
		.filter(|krate| krate.data(context.db).origin.is_local())
		.collect();

	let candidates: Vec<Crate> = local_crates
		.iter()
		.copied()
		.filter(|krate| root_file_name(context, *krate).as_deref() != Some("build.rs"))
		.collect();

	let target_crate = if candidates.len() == 1 {
		candidates[0]
	} else {
		let candidate_set: HashSet<Crate> = candidates.iter().copied().collect();
		let depended_upon: HashSet<Crate> = candidates
			.iter()
			.flat_map(|krate| {
				krate
					.data(context.db)
					.dependencies
					.iter()
					.map(|dep| dep.crate_id)
			})
			.filter(|dep_id| candidate_set.contains(dep_id))
			.collect();

		assert_eq!(
			depended_upon.len(),
			1,
			"Expected exactly one local library crate, found {} candidates (excluding build scripts) out of {} local crates total",
			candidates.len(),
			local_crates.len()
		);
		*depended_upon.iter().next().unwrap()
	};

	hir::Crate::from(target_crate).root_module(context.db)
}

fn root_file_name(context: &ProjectContext, krate: Crate) -> Option<String> {
	let file_id = krate.root_file_id(context.db).file_id(context.db);
	let vfs_path = context.vfs.file_path(file_id);
	let abs_path = vfs_path.as_path()?;
	abs_path.file_name().map(str::to_string)
}
