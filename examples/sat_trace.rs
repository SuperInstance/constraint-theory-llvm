//! Mini CDCL SAT Solver — generates traces for constraint-theory-llvm
//!
//! Usage: cargo run --example sat_trace

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Literal(i64);

impl Literal {
    pub fn var(&self) -> usize { self.0.unsigned_abs() as usize }
    pub fn sign(&self) -> bool { self.0 > 0 }
}

impl std::fmt::Debug for Literal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.sign() { write!(f, "x{}", self.var()) } else { write!(f, "!x{}", self.var()) }
    }
}

#[derive(Clone)]
pub struct Clause { literals: Vec<Literal> }
impl Clause {
    pub fn new(lits: Vec<Literal>) -> Self { Self { literals: lits } }
}

#[derive(Clone)]
pub struct Formula { pub clauses: Vec<Clause>, pub num_vars: usize }
impl Formula {
    pub fn new(n: usize) -> Self { Self { clauses: Vec::new(), num_vars: n } }
    pub fn add(&mut self, c: Clause) { self.clauses.push(c); }
}

#[derive(Clone)]
pub struct Assignment { vals: Vec<Option<bool>>, levels: Vec<usize> }
impl Assignment {
    pub fn new(n: usize) -> Self { Self { vals: vec![None; n+1], levels: vec![0; n+1] } }
    pub fn get(&self, v: usize) -> Option<bool> { self.vals.get(v).copied().flatten() }
    pub fn set(&mut self, lit: Literal, lvl: usize) {
        let v = lit.var();
        if v < self.vals.len() { self.vals[v] = Some(lit.sign()); self.levels[v] = lvl; }
    }
    pub fn unassign_from(&mut self, lvl: usize) {
        for i in 0..self.vals.len() { if self.levels[i] >= lvl { self.vals[i] = None; } }
    }
    pub fn count(&self) -> usize { self.vals.iter().filter(|v| v.is_some()).count() }
    pub fn is_sat(&self, f: &Formula) -> bool {
        !f.clauses.is_empty() && f.clauses.iter().all(|c| c.literals.iter().any(|&l| self.get(l.var()) == Some(l.sign())))
    }
}

#[derive(Clone)]
pub enum Step { Decide(usize, Literal), Propagate(Literal), Conflict(usize), Learn(Vec<Literal>), Backtrack(usize) }

#[derive(Clone)]
pub struct Trace { steps: Vec<Step>, learned: Vec<Vec<Literal>> }
impl Trace {
    pub fn new() -> Self { Self { steps: Vec::new(), learned: Vec::new() } }
    pub fn decide(&mut self, lvl: usize, lit: Literal) { self.steps.push(Step::Decide(lvl, lit)); }
    pub fn propagate(&mut self, lit: Literal) { self.steps.push(Step::Propagate(lit)); }
    pub fn conflict(&mut self, lvl: usize) { self.steps.push(Step::Conflict(lvl)); }
    pub fn learn(&mut self, lits: Vec<Literal>) { self.steps.push(Step::Learn(lits.clone())); self.learned.push(lits); }
    pub fn backtrack(&mut self, lvl: usize) { self.steps.push(Step::Backtrack(lvl)); }
    pub fn decisions(&self) -> usize { self.steps.iter().filter(|s| matches!(s, Step::Decide(_, _))).count() }
    pub fn learned_clauses(&self) -> &[Vec<Literal>] { &self.learned }
    pub fn program(&self) -> Vec<Literal> {
        self.steps.iter().filter_map(|s| match s { Step::Decide(_, l) => Some(*l), _ => None }).collect()
    }
}
impl Default for Trace { fn default() -> Self { Self::new() } }

pub struct Solver { formula: Formula, assign: Assignment, trace: Trace, level: usize }
impl Solver {
    pub fn new(nvars: usize) -> Self { Self { formula: Formula::new(nvars), assign: Assignment::new(nvars), trace: Trace::new(), level: 0 } }
    pub fn add_clause(&mut self, lits: Vec<Literal>) { self.formula.add(Clause::new(lits)); }

    pub fn solve(&mut self) -> (bool, Trace) {
        self.propagate();
        if self.assign.count() >= self.formula.num_vars { return (true, self.trace.clone()); }
        let mut restarts = 0;
        loop {
            if restarts > 100 { return (false, self.trace.clone()); }
            // Check conflicts
            let conflict = self.formula.clauses.iter().find(|c| {
                c.literals.iter().all(|l| self.assign.get(l.var()) == Some(!l.sign()))
            });
            if let Some(c) = conflict {
                self.trace.conflict(self.level);
                if self.level == 0 { return (false, self.trace.clone()); }
                let lit = c.literals[0];
                self.trace.learn(vec![lit]);
                self.formula.add(Clause::new(vec![lit]));
                self.level = self.level.saturating_sub(1);
                self.assign.unassign_from(self.level + 1);
                self.trace.backtrack(self.level);
                restarts += 1;
            } else {
                self.level += 1;
                let lit = Literal(self.level as i64);
                self.assign.set(lit, self.level);
                self.trace.decide(self.level, lit);
                self.propagate();
                if self.assign.count() >= self.formula.num_vars { return (true, self.trace.clone()); }
            }
        }
    }

    fn propagate(&mut self) {
        loop {
            let mut made_progress = false;
            for c in &self.formula.clauses {
                let unassigned: Vec<_> = c.literals.iter().filter(|l| self.assign.get(l.var()).is_none()).collect();
                if unassigned.len() == 1 {
                    let l = unassigned[0];
                    if self.assign.get(l.var()).is_none() {
                        self.assign.set(*l, self.level);
                        self.trace.propagate(*l);
                        made_progress = true;
                    }
                }
            }
            if !made_progress { break; }
        }
    }
}

fn main() {
    println!("=== Mini CDCL SAT Solver + Trace Generator ===\n");

    let mut s1 = Solver::new(4);
    s1.add_clause(vec![Literal(1), Literal(-2), Literal(3)]);
    s1.add_clause(vec![Literal(-1), Literal(2), Literal(4)]);
    s1.add_clause(vec![Literal(2), Literal(-3), Literal(4)]);
    s1.add_clause(vec![Literal(1), Literal(2), Literal(-4)]);
    println!("Formula 1: 4 vars, 4 clauses");
    let (sat, t) = s1.solve();
    println!("{} | {} decisions, {} learned", if sat { "SAT" } else { "UNSAT" }, t.decisions(), t.learned_clauses().len());
    println!("Decision program: {:?}", t.program());

    println!("\n---\nFormula 2: 8 vars, 12 clauses");
    let mut s2 = Solver::new(8);
    s2.add_clause(vec![Literal(1), Literal(2), Literal(3)]);
    s2.add_clause(vec![Literal(-1), Literal(4), Literal(5)]);
    s2.add_clause(vec![Literal(2), Literal(-3), Literal(6)]);
    s2.add_clause(vec![Literal(7), Literal(8), Literal(-2)]);
    s2.add_clause(vec![Literal(-4), Literal(-5), Literal(1)]);
    s2.add_clause(vec![Literal(3), Literal(6), Literal(-7)]);
    s2.add_clause(vec![Literal(-1), Literal(-8), Literal(2)]);
    s2.add_clause(vec![Literal(4), Literal(5), Literal(-6)]);
    s2.add_clause(vec![Literal(-2), Literal(-3), Literal(7)]);
    s2.add_clause(vec![Literal(1), Literal(-4), Literal(8)]);
    s2.add_clause(vec![Literal(-6), Literal(-7), Literal(3)]);
    s2.add_clause(vec![Literal(2), Literal(4), Literal(-8)]);
    let (sat, t) = s2.solve();
    println!("{} | {} decisions, {} learned", if sat { "SAT" } else { "UNSAT" }, t.decisions(), t.learned_clauses().len());
    let p = t.program();
    println!("Decision program ({} total): {:?}", p.len(), &p[..p.len().min(10)]);

    println!("\n=== LLVM IR Emission ===");
    println!("Use constraint_theory_llvm::LLVMEmitter to convert traces to AVX-512 IR");
}