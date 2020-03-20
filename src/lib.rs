#[macro_use]
extern crate log;

mod helpers;
mod lu;
mod ordering;
mod solver;
mod sparse;

use solver::Solver;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Variable(usize);

impl Variable {
    pub fn idx(&self) -> usize {
        self.0
    }
}

pub struct LinearExpr {
    vars: Vec<usize>,
    coeffs: Vec<f64>,
}

impl LinearExpr {
    pub fn empty() -> Self {
        Self {
            vars: vec![],
            coeffs: vec![],
        }
    }

    pub fn add(&mut self, var: Variable, coeff: f64) {
        self.vars.push(var.0);
        self.coeffs.push(coeff);
    }
}

pub struct LinearTerm(Variable, f64);

impl From<(Variable, f64)> for LinearTerm {
    fn from(term: (Variable, f64)) -> Self {
        LinearTerm(term.0, term.1)
    }
}

impl<'a> From<&'a (Variable, f64)> for LinearTerm {
    fn from(term: &'a (Variable, f64)) -> Self {
        LinearTerm(term.0, term.1)
    }
}

impl<I: IntoIterator<Item = impl Into<LinearTerm>>> From<I> for LinearExpr {
    fn from(iter: I) -> Self {
        let mut expr = LinearExpr::empty();
        for term in iter {
            let LinearTerm(var, coeff) = term.into();
            expr.add(var, coeff);
        }
        expr
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RelOp {
    Eq,
    Le,
    Ge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Infeasible,
    Unbounded,
}

type CsVec = sprs::CsVecI<f64, usize>;

pub struct Problem {
    obj: Vec<f64>,
    constraints: Vec<(CsVec, RelOp, f64)>,
}

impl Problem {
    pub fn new() -> Self {
        Problem {
            obj: vec![],
            constraints: vec![],
        }
    }

    pub fn add_var(&mut self, obj_coeff: f64) -> Variable {
        let var = Variable(self.obj.len());
        self.obj.push(obj_coeff);
        var
    }

    pub fn add_constraint(&mut self, expr: impl Into<LinearExpr>, rel_op: RelOp, bound: f64) {
        let expr = expr.into();
        self.constraints.push((
            CsVec::new(self.obj.len(), expr.vars, expr.coeffs),
            rel_op,
            bound,
        ));
    }

    pub fn solve(&self) -> Result<Solution, Error> {
        let mut solver = Solver::new(self.obj.clone(), self.constraints.clone());
        solver.optimize()?;
        Ok(Solution {
            num_vars: self.obj.len(),
            solver,
        })
    }
}

#[derive(Clone)]
pub struct Solution {
    num_vars: usize,
    solver: solver::Solver,
}

impl Solution {
    pub fn objective(&self) -> f64 {
        self.solver.cur_obj_val
    }

    pub fn get(&self, var: Variable) -> &f64 {
        self.solver.get_value(var.idx())
    }

    pub fn iter(&self) -> impl Iterator<Item = (Variable, &f64)> {
        (0..self.num_vars).map(move |idx| (Variable(idx), self.solver.get_value(idx)))
    }

    pub fn set_var(mut self, var: Variable, val: f64) -> Result<Self, Error> {
        assert!(var.idx() < self.num_vars);
        self.solver.set_var(var.idx(), val)?;
        Ok(self)
    }

    /// Return true if the var was really unset.
    pub fn unset_var(mut self, var: Variable) -> Result<(Self, bool), Error> {
        assert!(var.idx() < self.num_vars);
        let res = self.solver.unset_var(var.idx())?;
        Ok((self, res))
    }

    pub fn add_constraint(
        mut self,
        expr: impl Into<LinearExpr>,
        rel_op: RelOp,
        bound: f64,
    ) -> Result<Self, Error> {
        let LinearExpr { vars, mut coeffs } = expr.into();
        let (coeffs, bound) = match rel_op {
            RelOp::Le => (coeffs, bound),
            RelOp::Ge => {
                for c in &mut coeffs {
                    *c = -*c;
                }
                (coeffs, -bound)
            }
            RelOp::Eq => unimplemented!(),
        };
        let coeffs_csvec = CsVec::new(self.solver.num_vars, vars, coeffs);
        self.solver.add_le_constraint(coeffs_csvec, bound)?;
        Ok(self)
    }

    // TODO: remove_constraint

    pub fn add_gomory_cut(mut self, var: Variable) -> Result<Self, Error> {
        assert!(var.idx() < self.num_vars);
        self.solver.add_gomory_cut(var.idx())?;
        Ok(self)
    }
}

impl std::ops::Index<Variable> for Solution {
    type Output = f64;

    fn index(&self, var: Variable) -> &Self::Output {
        self.get(var)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimize() {
        let mut problem = Problem::new();
        let v1 = problem.add_var(-3.0);
        let v2 = problem.add_var(-4.0);
        problem.add_constraint(&[(v1, 1.0)], RelOp::Ge, 10.0);
        problem.add_constraint(&[(v2, 1.0)], RelOp::Ge, 5.0);
        problem.add_constraint(&[(v1, 1.0), (v2, 1.0)], RelOp::Le, 20.0);
        problem.add_constraint(&[(v2, 4.0), (v1, -1.0)], RelOp::Le, 20.0);

        let sol = problem.solve().unwrap();
        assert_eq!(sol[v1], 12.0);
        assert_eq!(sol[v2], 8.0);
        assert_eq!(sol.objective(), -68.0);
    }

    #[test]
    fn set_unset_var() {
        let mut problem = Problem::new();
        let v1 = problem.add_var(2.0);
        let v2 = problem.add_var(1.0);
        problem.add_constraint(&[(v1, 1.0), (v2, 1.0)], RelOp::Le, 4.0);
        problem.add_constraint(&[(v1, 1.0), (v2, 1.0)], RelOp::Ge, 2.0);

        let orig_sol = problem.solve().unwrap();

        {
            let mut sol = orig_sol.clone().set_var(v1, 3.0).unwrap();
            assert_eq!(sol[v1], 3.0);
            assert_eq!(sol[v2], 0.0);
            assert_eq!(sol.objective(), 6.0);

            sol = sol.unset_var(v1).unwrap().0;
            assert_eq!(sol[v1], 0.0);
            assert_eq!(sol[v2], 2.0);
            assert_eq!(sol.objective(), 2.0);
        }

        {
            let mut sol = orig_sol.clone().set_var(v2, 3.0).unwrap();
            assert_eq!(sol[v1], 0.0);
            assert_eq!(sol[v2], 3.0);
            assert_eq!(sol.objective(), 3.0);

            sol = sol.unset_var(v2).unwrap().0;
            assert_eq!(sol[v1], 0.0);
            assert_eq!(sol[v2], 2.0);
            assert_eq!(sol.objective(), 2.0);
        }
    }

    #[test]
    fn add_constraint() {
        let mut problem = Problem::new();
        let v1 = problem.add_var(2.0);
        let v2 = problem.add_var(1.0);
        problem.add_constraint(&[(v1, 1.0), (v2, 1.0)], RelOp::Le, 4.0);
        problem.add_constraint(&[(v1, 1.0), (v2, 1.0)], RelOp::Ge, 2.0);

        let orig_sol = problem.solve().unwrap();

        {
            let sol = orig_sol
                .clone()
                .add_constraint(&[(v1, -1.0), (v2, 1.0)], RelOp::Le, 0.0)
                .unwrap();

            assert_eq!(sol[v1], 1.0);
            assert_eq!(sol[v2], 1.0);
            assert_eq!(sol.objective(), 3.0);
        }

        {
            let sol = orig_sol
                .clone()
                .set_var(v2, 1.5)
                .unwrap()
                .add_constraint(&[(v1, -1.0), (v2, 1.0)], RelOp::Le, 0.0)
                .unwrap();
            assert_eq!(sol[v1], 1.5);
            assert_eq!(sol[v2], 1.5);
            assert_eq!(sol.objective(), 4.5);
        }

        {
            let sol = orig_sol
                .clone()
                .add_constraint(&[(v1, -1.0), (v2, 1.0)], RelOp::Ge, 3.0)
                .unwrap();

            assert_eq!(sol[v1], 0.0);
            assert_eq!(sol[v2], 3.0);
            assert_eq!(sol.objective(), 3.0);
        }
    }

    #[test]
    fn gomory_cut() {
        let mut problem = Problem::new();
        let v1 = problem.add_var(0.0);
        let v2 = problem.add_var(-1.0);
        problem.add_constraint(&[(v1, 3.0), (v2, 2.0)], RelOp::Le, 6.0);
        problem.add_constraint(&[(v1, -3.0), (v2, 2.0)], RelOp::Le, 0.0);

        let mut sol = problem.solve().unwrap();
        assert_eq!(sol[v1], 1.0);
        assert_eq!(sol[v2], 1.5);
        assert_eq!(sol.objective(), -1.5);

        sol = sol.add_gomory_cut(v2).unwrap();
        assert!(f64::abs(sol[v1] - 2.0 / 3.0) < 1e-8);
        assert_eq!(sol[v2], 1.0);
        assert_eq!(sol.objective(), -1.0);

        sol = sol.add_gomory_cut(v1).unwrap();
        assert!(f64::abs(sol[v1] - 1.0) < 1e-8);
        assert_eq!(sol[v2], 1.0);
        assert_eq!(sol.objective(), -1.0);
    }
}
