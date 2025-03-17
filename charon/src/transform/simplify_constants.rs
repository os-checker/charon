//! The MIR constant expressions lead to a lot of duplication: there are
//! for instance constant ADTs which duplicate the "regular" aggregated
//! ADTs in the operands, constant references, etc. This reduces the number
//! of cases to handle and eases the function translation in Aeneas.
//!
//! This pass removes all those occurrences so that only the
//! [ConstantExpression::Literal]. It does so by introducing intermediate statements.
//!
//! A small remark about the intermediate statements we introduce for the globals:
//! we do so because, when evaluating the code in "concrete" mode, it allows to
//! handle the globals like function calls.

use itertools::Itertools;
use std::assert_matches::assert_matches;

use crate::transform::TransformCtx;
use crate::ullbc_ast::*;

use super::ctx::UllbcPass;

/// If the constant value is a constant ADT, push `Assign::Aggregate` statements
/// to the vector of statements, that bind new variables to the ADT parts and
/// the variable assigned to the complete ADT.
///
/// Goes fom e.g. `f(T::A(x, y))` to `let a = T::A(x, y); f(a)`.
/// The function is recursively called on the aggregate fields (e.g. here x and y).
fn transform_constant_expr(
    span: &Span,
    val: ConstantExpr,
    new_var: &mut impl FnMut(Rvalue, Ty) -> Place,
) -> Operand {
    match val.value {
        RawConstantExpr::Literal(_)
        | RawConstantExpr::Var(_)
        | RawConstantExpr::RawMemory(..)
        | RawConstantExpr::TraitConst(..)
        | RawConstantExpr::FnPtr(..) => {
            // Nothing to do
            // TODO: for trait const: might come from a top-level impl, so we might
            // want to introduce an intermediate statement to be able to evaluate
            // it as a function call, like for globals.
            Operand::Const(val)
        }
        RawConstantExpr::Global(global_ref) => {
            Operand::Move(new_var(Rvalue::Global(global_ref), val.ty.clone()))
        }
        RawConstantExpr::Ref(box bval) => {
            match bval.value {
                RawConstantExpr::Global(global_ref) => Operand::Move(new_var(
                    Rvalue::GlobalRef(global_ref, RefKind::Shared),
                    val.ty,
                )),
                _ => {
                    // Recurse on the borrowed value
                    let bval_ty = bval.ty.clone();
                    let bval = transform_constant_expr(span, bval, new_var);

                    // Evaluate the referenced value
                    let bvar = new_var(Rvalue::Use(bval), bval_ty);

                    // Borrow the value
                    let ref_var = new_var(Rvalue::Ref(bvar, BorrowKind::Shared), val.ty);

                    Operand::Move(ref_var)
                }
            }
        }
        RawConstantExpr::MutPtr(box bval) => {
            match bval.value {
                RawConstantExpr::Global(global_ref) => {
                    Operand::Move(new_var(Rvalue::GlobalRef(global_ref, RefKind::Mut), val.ty))
                }
                _ => {
                    // Recurse on the borrowed value
                    let bval_ty = bval.ty.clone();
                    let bval = transform_constant_expr(span, bval, new_var);

                    // Evaluate the referenced value
                    let bvar = new_var(Rvalue::Use(bval), bval_ty);

                    // Borrow the value
                    let ref_var = new_var(Rvalue::RawPtr(bvar, RefKind::Mut), val.ty);

                    Operand::Move(ref_var)
                }
            }
        }
        RawConstantExpr::Adt(variant, fields) => {
            let fields = fields
                .into_iter()
                .map(|x| transform_constant_expr(span, x, new_var))
                .collect();

            // Build an `Aggregate` rvalue.
            let rval = {
                let (adt_kind, generics) = val.ty.kind().as_adt().unwrap();
                let aggregate_kind = AggregateKind::Adt(*adt_kind, variant, None, generics.clone());
                Rvalue::Aggregate(aggregate_kind, fields)
            };
            let var = new_var(rval, val.ty);

            Operand::Move(var)
        }
        RawConstantExpr::Array(fields) => {
            let fields = fields
                .into_iter()
                .map(|x| transform_constant_expr(span, x, new_var))
                .collect_vec();

            let len = ConstGeneric::Value(Literal::Scalar(ScalarValue::Usize(fields.len() as u64)));
            let (adt_kind, generics) = val.ty.kind().as_adt().unwrap();
            assert_matches!(
                *adt_kind.as_builtin().unwrap(),
                BuiltinTy::Array | BuiltinTy::Slice
            );
            let ty = generics.types[0].clone();
            let rval = if fields.len() >= 2
                && let Ok(op) = fields.iter().dedup().exactly_one()
            {
                // If all the values are the same one, use an array repeat expression.
                Rvalue::Repeat(op.clone(), ty.clone(), len)
            } else {
                // Build an `Aggregate` rvalue.
                Rvalue::Aggregate(AggregateKind::Array(ty, len), fields)
            };
            let var = new_var(rval, val.ty);

            Operand::Move(var)
        }
    }
}

fn transform_operand(span: &Span, locals: &mut Locals, nst: &mut Vec<Statement>, op: &mut Operand) {
    // Transform the constant operands (otherwise do nothing)
    take_mut::take(op, |op| {
        if let Operand::Const(val) = op {
            let mut new_var = |rvalue, ty| {
                if let Rvalue::Use(Operand::Move(place)) = rvalue {
                    place
                } else {
                    let var = locals.new_var(None, ty);
                    nst.push(Statement::new(
                        *span,
                        RawStatement::Assign(var.clone(), rvalue),
                    ));
                    var
                }
            };
            transform_constant_expr(span, val, &mut new_var)
        } else {
            op
        }
    })
}

pub struct Transform;
impl UllbcPass for Transform {
    fn transform_body(&self, _ctx: &mut TransformCtx, body: &mut ExprBody) {
        for block in body.body.iter_mut() {
            block.transform_operands(|span, nst, op| {
                transform_operand(span, &mut body.locals, nst, op)
            })
        }
    }
}
