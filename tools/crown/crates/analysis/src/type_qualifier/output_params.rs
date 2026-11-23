use std::collections::HashMap;

use alias::AliasResult;
use rustc_hash::{FxHashMap, FxHashSet};
use rustc_hir::def_id::DefId;
use rustc_index::vec::Idx;
use rustc_middle::{
    mir::{Body, Local, TerminatorKind, VarDebugInfoContents},
    ty::TyCtxt,
};
use rustc_type_ir::TyKind::FnDef;
use serde::Serialize;

use super::flow_insensitive::mutability::{Mutability, MutabilityResult};
use crate::call_graph::CallGraph;

pub type OutputParams = FxHashMap<DefId, FxHashSet<Local>>;

#[derive(Serialize)]
pub struct OutputParamsData {
    pub fn_data: HashMap<String, Vec<String>>,
}

pub fn make_output_params_data(
    output_params: &OutputParams,
    crate_data: &common::CrateData,
) -> OutputParamsData {
    let tcx = crate_data.tcx;
    let mut fn_data = HashMap::with_capacity(crate_data.fns.len());

    for &did in &crate_data.fns {
        let def_path_str = tcx.def_path_str(did);
        let body = tcx.optimized_mir(did);

        let user_idents: HashMap<Local, _> = body
            .var_debug_info
            .iter()
            .filter_map(|info| match info.value {
                VarDebugInfoContents::Place(place) => {
                    Some((place.as_local()?, info.name))
                }
                _ => None,
            })
            .collect();

        let mut param_names: Vec<String> = output_params
            .get(&did)
            .map(|params| {
                params
                    .iter()
                    .filter_map(|local| {
                        user_idents.get(local).map(|name| name.as_str().to_owned())
                    })
                    .collect()
            })
            .unwrap_or_default();
        param_names.sort();

        fn_data.insert(def_path_str, param_names);
    }

    OutputParamsData { fn_data }
}

pub fn show_output_params(
    crate_data: &common::CrateData,
    alias_result: &AliasResult,
    mutability_result: &MutabilityResult,
) {
    fn show_set<T: std::fmt::Debug>(set: impl Iterator<Item = T>) -> String {
        set.map(|x| format!("{:?}", x))
            .collect::<Vec<_>>()
            .join(", ")
    }

    let output_params = compute_output_params(crate_data, alias_result, mutability_result);

    for (did, noalias_params) in output_params {
        let noalias_params_str = show_set(noalias_params.into_iter());
        println!(
            "@{}: {noalias_params_str}",
            crate_data.tcx.def_path_str(did)
        );
    }
}

pub fn compute_output_params(
    crate_data: &common::CrateData,
    alias_result: &AliasResult,
    mutability_result: &MutabilityResult,
) -> OutputParams {
    let mut output_params = FxHashMap::default();
    output_params.reserve(crate_data.fns.len());

    let call_graph = CallGraph::new(crate_data.tcx, &crate_data.fns);

    for &did in call_graph.sccs().flatten() {
        let body = crate_data.tcx.optimized_mir(did);
        conservative_output_params(
            body,
            alias_result,
            mutability_result,
            &mut output_params,
            crate_data.tcx,
        )
    }

    output_params
}

fn conservative_output_params(
    body: &Body,
    alias_result: &AliasResult,
    mutability_result: &MutabilityResult,
    known_facts: &mut OutputParams,
    tcx: TyCtxt,
) {
    let location_of = alias_result.local_locations(&body.source.def_id());
    let fn_result = mutability_result.fn_results(&body.source.def_id());

    let mut this_output_params = body
        .args_iter()
        .filter(|&arg| {
            // !body.local_decls[arg].ty.is_primitive_ty()
            // &&
            matches!(fn_result
                .local_result(arg)
                .first(),
                Some(&mutability)
                if mutability == Mutability::Mut
            )
        })
        .collect::<FxHashSet<Local>>();

    let call_args_mapping = collect_call_args_mapping(body, tcx);

    for arg1 in body.args_iter().map(|arg| arg.index()) {
        for arg2 in body.args_iter().map(|arg| arg.index()) {
            if arg1 == arg2 {
                continue;
            };
            if alias_result.may_alias(location_of[arg1], location_of[arg2]) {
                tracing::debug!(
                    "@{:?}: {:?} removed because it aliases another argument {:?}",
                    body.source.def_id(),
                    &Local::new(arg1),
                    &Local::new(arg2)
                );
                this_output_params.remove(&Local::new(arg1));
            }
        }
    }

    for arg in body.args_iter().map(|arg| arg.index()) {
        for (local, local_decl) in body.local_decls.iter_enumerated() {
            if local_decl.ty.is_primitive_ty() {
                continue;
            }
            if let Some(&(_callee, _idx)) = call_args_mapping.get(&local) {
                continue;
            }
            let local = local.index();
            if local == arg {
                continue;
            }
            if alias_result.may_alias(location_of[arg], location_of[local])
                && matches!(fn_result
                    .local_result(Local::new(local))
                    .first(),
                    Some(&mutability) if mutability == Mutability::Mut)
            {
                tracing::debug!(
                    "@{:?}: {:?} removed because it aliases {:?}, which is mutable",
                    body.source.def_id(),
                    &Local::new(arg),
                    &Local::new(local)
                );
                this_output_params.remove(&Local::new(arg));
            }
        }
    }

    known_facts.insert(body.source.def_id(), this_output_params);
}

/// mapping call arg temp to callee and index of parameter
type CallArgsMapping = FxHashMap<Local, (DefId, usize)>;

fn collect_call_args_mapping(body: &Body, tcx: TyCtxt) -> CallArgsMapping {
    let mut call_args_mapping = FxHashMap::default();
    for bb_data in body.basic_blocks.iter() {
        let Some(terminator) = &bb_data.terminator else { continue; };
        if let TerminatorKind::Call { func, args, .. } = &terminator.kind {
            let Some(func) = func.constant() else { continue };
            let ty = func.ty();
            let &FnDef(callee, _) = ty.kind() else { unreachable!() };
            let Some(local_did) = callee.as_local() else { continue };
            let rustc_hir::Node::Item(_) = tcx.hir().find_by_def_id(local_did).unwrap() else { continue };

            for (idx, arg) in args.iter().enumerate() {
                let Some(arg) = arg.place() else { continue };
                let arg = arg.as_local().unwrap();
                call_args_mapping.insert(arg, (callee, idx));
            }
        }
    }

    call_args_mapping
}
