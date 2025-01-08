//! Desugar some unary/binary operations and the array repeats to function calls.
//! For instance, we desugar ArrayToSlice from an unop to a function call.
//! This allows a more uniform treatment later on.
//! TODO: actually transform all the unops and binops to function calls?
use crate::transform::TransformCtx;
use crate::ullbc_ast::*;

use super::ctx::UllbcPass;

fn transform_st(s: &mut Statement) {
    match &s.content {
        // Transform the ArrayToSlice unop
        RawStatement::Assign(p, Rvalue::UnaryOp(UnOp::ArrayToSlice(ref_kind, ty, cg), op)) => {
            // We could avoid the clone operations below if we take the content of
            // the statement. In practice, this shouldn't have much impact.
            let id = match ref_kind {
                RefKind::Mut => BuiltinFunId::ArrayToSliceMut,
                RefKind::Shared => BuiltinFunId::ArrayToSliceShared,
            };
            let func = FunIdOrTraitMethodRef::mk_builtin(id);
            let generics = GenericArgs::new(
                vec![Region::Erased].into(),
                vec![ty.clone()].into(),
                vec![cg.clone()].into(),
                vec![].into(),
                GenericsSource::Builtin,
            );
            let func = FnOperand::Regular(FnPtr { func, generics });
            s.content = RawStatement::Call(Call {
                func,
                args: vec![op.clone()],
                dest: p.clone(),
            });
        }
        // Transform the array aggregates to function calls
        RawStatement::Assign(p, Rvalue::Repeat(op, ty, cg)) => {
            // We could avoid the clone operations below if we take the content of
            // the statement. In practice, this shouldn't have much impact.
            let id = BuiltinFunId::ArrayRepeat;
            let func = FunIdOrTraitMethodRef::mk_builtin(id);
            let generics = GenericArgs::new(
                vec![Region::Erased].into(),
                vec![ty.clone()].into(),
                vec![cg.clone()].into(),
                vec![].into(),
                GenericsSource::Builtin,
            );
            let func = FnOperand::Regular(FnPtr { func, generics });
            s.content = RawStatement::Call(Call {
                func,
                args: vec![op.clone()],
                dest: p.clone(),
            });
        }
        _ => {}
    }
}

pub struct Transform;
impl UllbcPass for Transform {
    fn transform_body(&self, _ctx: &mut TransformCtx, b: &mut ExprBody) {
        b.visit_statements(&mut transform_st);
    }
}
