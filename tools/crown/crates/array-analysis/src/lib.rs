#![feature(rustc_private)]

extern crate rustc_middle;

use rustc_middle::{
    mir::{
        visit::{PlaceContext, Visitor},
        Body, Location,
    },
    ty::TyKind,
};
use serde::{Deserialize, Serialize};

use analysis::{
    ownership::solidify::SolidifiedOwnershipSchemes,
    type_qualifier::flow_insensitive::fatness::FatnessResult,
};
use common::CrateData;

#[derive(Default, Serialize, Deserialize)]
pub struct ArrayStats {
    pub sources_total: usize,
    pub sources_arr: usize,
    pub sources_thin: usize,
    pub sources_arr_owning: usize,
    pub sources_arr_transient: usize,
    pub sources_thin_owning: usize,
    pub sources_thin_transient: usize,

    pub usages_total: usize,
    pub usages_arr: usize,
    pub usages_thin: usize,
    pub usages_arr_owning: usize,
    pub usages_arr_transient: usize,
    pub usages_thin_owning: usize,
    pub usages_thin_transient: usize,
}

impl ArrayStats {
    pub fn new(
        crate_data: &CrateData,
        fatness_result: &FatnessResult,
        ownership_result: &SolidifiedOwnershipSchemes,
    ) -> Self {
        let mut stats = ArrayStats::default();
        let tcx = crate_data.tcx;

        // Sources: function-local user variables
        for &did in &crate_data.fns {
            let body = tcx.optimized_mir(did);
            for (local, local_decl) in body.local_decls.iter_enumerated() {
                if local_decl.is_user_variable() && local_decl.ty.is_unsafe_ptr() {
                    let is_arr =
                        fatness_result.fn_results(&did).local_result(local)[0].is_arr();
                    let is_owning =
                        ownership_result.fn_results(&did).local_result(local)[0].is_owning();
                    stats.record_source(is_arr, is_owning);
                }
            }
        }

        // Sources: struct fields
        for &did in &crate_data.structs {
            let struct_ty = tcx.type_of(did);
            let TyKind::Adt(adt_def, substs) = struct_ty.kind() else {
                continue;
            };
            for (field_idx, field_def) in adt_def.all_fields().enumerate() {
                let field_ty = field_def.ty(tcx, substs);
                if field_ty.is_unsafe_ptr() {
                    let is_arr =
                        fatness_result.struct_field_result(&did, field_idx)[0].is_arr();
                    let is_owning =
                        ownership_result.struct_field_result(&did, field_idx)[0].is_owning();
                    stats.record_source(is_arr, is_owning);
                }
            }
        }

        // Usages: MIR indirect-place dereferences
        for &did in &crate_data.fns {
            let body = tcx.optimized_mir(did);
            CollectUsages {
                body: &body,
                fatness_result,
                ownership_result,
                stats: &mut stats,
            }
            .visit_body(&body);
        }

        stats
    }

    fn record_source(&mut self, is_arr: bool, is_owning: bool) {
        self.sources_total += 1;
        if is_arr {
            self.sources_arr += 1;
            if is_owning {
                self.sources_arr_owning += 1;
            } else {
                self.sources_arr_transient += 1;
            }
        } else {
            self.sources_thin += 1;
            if is_owning {
                self.sources_thin_owning += 1;
            } else {
                self.sources_thin_transient += 1;
            }
        }
    }

    fn record_usage(&mut self, is_arr: bool, is_owning: bool) {
        self.usages_total += 1;
        if is_arr {
            self.usages_arr += 1;
            if is_owning {
                self.usages_arr_owning += 1;
            } else {
                self.usages_arr_transient += 1;
            }
        } else {
            self.usages_thin += 1;
            if is_owning {
                self.usages_thin_owning += 1;
            } else {
                self.usages_thin_transient += 1;
            }
        }
    }
}

struct CollectUsages<'me, 'tcx> {
    body: &'me Body<'tcx>,
    fatness_result: &'me FatnessResult,
    ownership_result: &'me SolidifiedOwnershipSchemes,
    stats: &'me mut ArrayStats,
}

impl<'me, 'tcx> Visitor<'tcx> for CollectUsages<'me, 'tcx> {
    fn visit_place(
        &mut self,
        place: &rustc_middle::mir::Place<'tcx>,
        context: PlaceContext,
        _: Location,
    ) {
        if matches!(context, PlaceContext::NonUse(..)) {
            return;
        }
        let local = place.local;
        let local_decl = &self.body.local_decls[local];
        if local_decl.is_user_variable()
            && local_decl.ty.is_unsafe_ptr()
        {
            let did = self.body.source.def_id();
            let is_arr =
                self.fatness_result.fn_results(&did).local_result(local)[0].is_arr();
            let is_owning =
                self.ownership_result.fn_results(&did).local_result(local)[0].is_owning();
            self.stats.record_usage(is_arr, is_owning);
        }
    }
}
