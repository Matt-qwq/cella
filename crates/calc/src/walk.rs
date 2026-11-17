//! A preorder walk of a formula tree that keeps its pending nodes on the stack.
//!
//! [`cella_formula::Expr::walk`] allocates a vector per call, and building the dependency graph
//! walks every formula (twice: its precedents, and whether it may spill). On a million-formula
//! sheet that is a million allocations for a tree of a handful of nodes. The pending nodes of a
//! formula fit in an array here; only a tree nested past `INLINE` — a chain a machine wrote —
//! spills to a vector, and then the cost is no worse than `Expr::walk`.

use cella_formula::Expr;

/// Pending nodes kept inline. A written formula has a few (the depth of its expression); a chain
/// of ten thousand `+ A1` terms is the case that spills.
const INLINE: usize = 24;

/// Pending nodes of a walk, last in first out.
pub(crate) struct Stack<'a> {
    slots: [Option<&'a Expr>; INLINE],
    depth: usize,
    spill: Vec<&'a Expr>,
}

impl<'a> Stack<'a> {
    pub(crate) fn new(root: &'a Expr) -> Self {
        let mut stack = Stack { slots: [None; INLINE], depth: 0, spill: Vec::new() };
        stack.push(root);
        stack
    }

    pub(crate) fn push(&mut self, e: &'a Expr) {
        if let Some(slot) = self.slots.get_mut(self.depth) {
            *slot = Some(e);
            self.depth += 1;
        } else {
            self.spill.push(e);
        }
    }

    pub(crate) fn pop(&mut self) -> Option<&'a Expr> {
        // Once the array is full every further node goes to the spill, so its top is the newest.
        if let Some(e) = self.spill.pop() {
            return Some(e);
        }
        let top = self.depth.checked_sub(1)?;
        self.depth = top;
        self.slots.get_mut(top)?.take()
    }
}

/// `f` for every node of `root`, parents before children, left before right — the order
/// [`Expr::walk`] visits them.
pub(crate) fn walk<'a>(root: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    let mut stack = Stack::new(root);
    while let Some(node) = stack.pop() {
        f(node);
        match node {
            Expr::Array(rows) => {
                for e in rows.iter().flatten().rev() {
                    stack.push(e);
                }
            }
            Expr::Unary(_, e) | Expr::Paren(e) => stack.push(e),
            Expr::Binary(_, a, b) => {
                stack.push(b);
                stack.push(a);
            }
            Expr::Call(_, args) => {
                for a in args.iter().rev() {
                    stack.push(a);
                }
            }
            Expr::Invoke(c, args) => {
                for a in args.iter().rev() {
                    stack.push(a);
                }
                stack.push(c);
            }
            _ => {}
        }
    }
}
