//! In the MIR AST, it seems `assert` are introduced to check preconditions
//! (for the binops for example). The `assert!` introduced by the user
//! introduce `if ... then { panic!(...) } else { ...}`.
//! This pass introduces `assert` instead in order to make the code shorter.

use std::mem;

use crate::llbc_ast::*;
use crate::transform::TransformCtx;

use super::ctx::LlbcPass;

fn transform_st(st: &mut Statement) -> Vec<Statement> {
    if let RawStatement::Switch(Switch::If(_, then_block, _)) = &mut st.content
        && let Some(first) = then_block.statements.first_mut()
        && first.content.is_abort()
    {
        // Replace the `if` with a `nop`.
        let (op, then_block, else_block) = mem::replace(&mut st.content, RawStatement::Nop)
            .to_switch()
            .unwrap()
            .to_if()
            .unwrap();
        let assert = Statement::new(
            then_block.span,
            RawStatement::Assert(Assert {
                cond: op,
                expected: false,
            }),
        );
        [assert].into_iter().chain(else_block.statements).collect()
    } else {
        Vec::new()
    }
}

pub struct Transform;
impl LlbcPass for Transform {
    fn transform_body(&self, _ctx: &mut TransformCtx<'_>, b: &mut ExprBody) {
        b.body.transform(&mut transform_st);
    }
}
