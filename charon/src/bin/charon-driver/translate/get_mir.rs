//! Various utilities to load MIR.
//! Allow to easily load the MIR code generated by a specific pass.

use crate::options::MirLevel;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::Body;
use rustc_middle::ty::TyCtxt;

/// Indicates if the constants should be extracted in their own identifier,
/// or if they must be evaluated to a constant value, depending on the
/// MIR level which we extract.
pub fn extract_constants_at_top_level(level: MirLevel) -> bool {
    match level {
        MirLevel::Built => true,
        MirLevel::Promoted => true,
        MirLevel::Optimized => false,
    }
}

/// Are boxe manipulations desugared to very low-level code using raw pointers,
/// unique and non-null pointers? See [crate::types::Ty::RawPtr] for detailed explanations.
pub fn boxes_are_desugared(level: MirLevel) -> bool {
    match level {
        MirLevel::Built => false,
        MirLevel::Promoted => false,
        MirLevel::Optimized => true,
    }
}

/// Query the MIR for a function at a specific level. Return `None` in the case of a foreign body
/// with no MIR available (e.g. because it is not available for inlining).
pub fn get_mir_for_def_id_and_level(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    level: MirLevel,
) -> Option<Body<'_>> {
    // Below: we **clone** the bodies to make sure we don't have issues with
    // locked values (we had in the past).
    let body = if let Some(local_def_id) = def_id.as_local() {
        match level {
            MirLevel::Built => {
                let body = tcx.mir_built(local_def_id);
                // We clone to be sure there are no problems with locked values
                body.borrow().clone()
            }
            MirLevel::Promoted => {
                let (body, _) = tcx.mir_promoted(local_def_id);
                // We clone to be sure there are no problems with locked values
                body.borrow().clone()
            }
            MirLevel::Optimized => tcx.optimized_mir(def_id).clone(),
        }
    } else {
        // There are only two MIRs we can fetch for non-local bodies: CTFE mir for globals and
        // const fns, and optimized MIR for inlinable functions. The rest don't have MIR in the
        // rlib.
        if tcx.is_mir_available(def_id) {
            tcx.optimized_mir(def_id).clone()
        } else if tcx.is_ctfe_mir_available(def_id) {
            tcx.mir_for_ctfe(def_id).clone()
        } else {
            return None;
        }
    };
    Some(body)
}
