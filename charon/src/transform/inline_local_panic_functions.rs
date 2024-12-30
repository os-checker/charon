//! `panic!()` expands to:
//! ```ignore
//! fn panic_cold_explicit() -> ! {
//!     core::panicking::panic_explicit()
//! }
//! panic_cold_explicit()
//! ```
//! Which defines a new function each time. This pass recognizes these functions and replaces calls
//! to them by a `Panic` terminator.
use std::collections::HashSet;

use super::{ctx::LlbcPass, TransformCtx};
use crate::{builtins, llbc_ast::*, names::Name};

pub struct Transform;
impl LlbcPass for Transform {
    fn transform_ctx(&self, ctx: &mut TransformCtx) {
        // Collect the functions that were generated by the `panic!` macro.
        let mut panic_fns = HashSet::new();
        ctx.for_each_fun_decl(|_ctx, decl, body| {
            if let Ok(body) = body {
                let body = body.as_structured().unwrap();
                // If the whole body is only a call to this specific panic function.
                if let [st] = body.body.statements.as_slice()
                    && let RawStatement::Abort(AbortKind::Panic(name)) = &st.content
                {
                    if name.equals_ref_name(builtins::EXPLICIT_PANIC_NAME) {
                        // FIXME: also check that the name of the function is
                        // `panic_cold_explicit`?
                        panic_fns.insert(decl.def_id);
                    }
                }
            }
        });

        let panic_name = Name::from_path(builtins::EXPLICIT_PANIC_NAME);
        let panic_statement = RawStatement::Abort(AbortKind::Panic(panic_name));

        // Replace each call to one such function with a `Panic`.
        ctx.for_each_structured_body(|_ctx, body| {
            body.body
                .visit_statements(|st: &mut Statement| match &mut st.content {
                    RawStatement::Call(Call {
                        func:
                            FnOperand::Regular(FnPtr {
                                func: FunIdOrTraitMethodRef::Fun(FunId::Regular(fun_id)),
                                ..
                            }),
                        ..
                    }) if panic_fns.contains(fun_id) => {
                        st.content = panic_statement.clone();
                    }
                    _ => {}
                });
        });

        // Remove these functions from the context.
        for id in &panic_fns {
            ctx.translated.fun_decls.remove(*id);
        }
    }
}
