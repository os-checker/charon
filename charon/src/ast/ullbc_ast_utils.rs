//! Implementations for [crate::ullbc_ast]
use crate::ids::Vector;
use crate::meta::Span;
use crate::ullbc_ast::*;
use take_mut::take;

impl SwitchTargets {
    pub fn get_targets(&self) -> Vec<BlockId> {
        match self {
            SwitchTargets::If(then_tgt, else_tgt) => {
                vec![*then_tgt, *else_tgt]
            }
            SwitchTargets::SwitchInt(_, targets, otherwise) => {
                let mut all_targets = vec![];
                for (_, target) in targets {
                    all_targets.push(*target);
                }
                all_targets.push(*otherwise);
                all_targets
            }
        }
    }
}

impl Statement {
    pub fn new(span: Span, content: RawStatement) -> Self {
        Statement {
            span,
            content,
            comments_before: vec![],
        }
    }
}

impl Terminator {
    pub fn new(span: Span, content: RawTerminator) -> Self {
        Terminator {
            span,
            content,
            comments_before: vec![],
        }
    }
}

impl BlockData {
    pub fn targets(&self) -> Vec<BlockId> {
        match &self.terminator.content {
            RawTerminator::Goto { target } => {
                vec![*target]
            }
            RawTerminator::Switch { targets, .. } => targets.get_targets(),
            RawTerminator::Abort(..) | RawTerminator::Return => {
                vec![]
            }
        }
    }

    /// See [body_transform_operands]
    pub fn transform_operands<F: FnMut(&Span, &mut Vec<Statement>, &mut Operand)>(
        mut self,
        mut f: F,
    ) -> Self {
        // The new vector of statements
        let mut nst = vec![];

        // Explore the operands in the statements
        for mut st in self.statements {
            st.content
                .dyn_visit_in_body_mut(|op: &mut Operand| f(&st.span, &mut nst, op));
            // Add the statement to the vector of statements
            nst.push(st)
        }

        // Explore the terminator
        self.terminator
            .content
            .dyn_visit_in_body_mut(|op: &mut Operand| f(&self.terminator.span, &mut nst, op));

        // Update the vector of statements
        self.statements = nst;

        // Return
        self
    }

    /// Apply a transformer to all the statements, in a bottom-up manner.
    ///
    /// The transformer should:
    /// - mutate the current statement in place
    /// - return the sequence of statements to introduce before the current statement
    pub fn transform<F: FnMut(&mut Statement) -> Vec<Statement>>(&mut self, mut f: F) {
        self.transform_sequences(|slice| {
            let new_statements = f(&mut slice[0]);
            if new_statements.is_empty() {
                vec![]
            } else {
                vec![(0, new_statements)]
            }
        });
    }

    /// Apply a transformer to all the statements, in a bottom-up manner.
    ///
    /// The transformer should:
    /// - mutate the current statements in place
    /// - return a list of `(i, statements)` where `statements` will be inserted before index `i`.
    pub fn transform_sequences<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut [Statement]) -> Vec<(usize, Vec<Statement>)>,
    {
        for i in (0..self.statements.len()).rev() {
            let mut to_insert = f(&mut self.statements[i..]);
            if !to_insert.is_empty() {
                to_insert.sort_by_key(|(i, _)| *i);
                for (j, statements) in to_insert.into_iter().rev() {
                    // Insert the new elements at index `j`. This only modifies `statements[j..]`
                    // so we can keep iterating `j` (and `i`) down as if nothing happened.
                    self.statements.splice(i + j..i + j, statements);
                }
            }
        }
    }
}

impl ExprBody {
    pub fn transform_sequences<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Locals, &mut [Statement]) -> Vec<(usize, Vec<Statement>)>,
    {
        for block in &mut self.body {
            block.transform_sequences(|seq| f(&mut self.locals, seq));
        }
    }

    /// Apply a function to all the statements, in a bottom-up manner.
    pub fn visit_statements<F: FnMut(&mut Statement)>(&mut self, mut f: F) {
        for block in self.body.iter_mut().rev() {
            for st in block.statements.iter_mut().rev() {
                f(st);
            }
        }
    }
}

/// Transform a body by applying a function to its operands, and
/// inserting the statements generated by the operands at the end of the
/// block.
/// Useful to implement a pass on operands (see e.g., [crate::extract_global_assignments]).
///
/// The span argument given to `f` is the span argument of the [Terminator]
/// containing the operand. `f` should explore the operand it receives, and
/// push statements to the vector it receives as input.
pub fn body_transform_operands<F: FnMut(&Span, &mut Vec<Statement>, &mut Operand)>(
    blocks: &mut Vector<BlockId, BlockData>,
    mut f: F,
) {
    for block in blocks.iter_mut() {
        take(block, |b| b.transform_operands(&mut f));
    }
}
