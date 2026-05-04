//! CDCL Trace — the execution trace of the CDCL solver
//!
//! FM's 35.9B/s AVX-512 engine checks constraints at memory bandwidth speed.
//! But it's STATELESS — each check is independent.
//!
//! The CDCL trace captures the DECISION TREE of the solver:
//! - Decisions: branching choices
//! - Propagations: unit clause propagation (deterministic)
//! - Conflicts: constraint violations
//! - Backtracks: learning and reversing
//!
//! Compiling this trace to AVX-512 = learned constraints run at full memory bandwidth.

use serde::{Deserialize, Serialize};

/// A recorded event in the CDCL solver execution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TraceEvent {
    /// A decision (branch) — the only non-deterministic step
    Decide(Decision),
    /// Unit clause propagation — deterministic constraint narrowing
    Propagate(Propagation),
    /// Conflict detected — unsatisfiable assignment
    Conflict(Conflict),
    /// Backtrack to a level — learning happened here
    Backtrack(Backtrack),
    /// A learnt clause was added to the clause database
    Learn { clause_id: usize, literals: Vec<i64> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub level: usize,
    pub literal: i64,  // Lit: positive = true, negative = false
    pub reason: Option<usize>,  // clause that forced this decision
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Propagation {
    pub literal: i64,
    pub antecedent: usize,  // clause index that forced this
    pub level: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conflict {
    pub level: usize,
    pub conflicting_clause: usize,
    pub analysis: Vec<i64>,  // conflict clause literals (for learning)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Backtrack {
    pub target_level: usize,
    pub learnt_literals: Vec<i64>,
}

/// Complete execution trace of a CDCL solver
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CDCLTrace {
    pub events: Vec<TraceEvent>,
    pub num_vars: i64,
    pub decisions: usize,
    pub propagations: usize,
    pub conflicts: usize,
    pub backtracks: usize,
}

impl CDCLTrace {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            num_vars: 0,
            decisions: 0,
            propagations: 0,
            conflicts: 0,
            backtracks: 0,
        }
    }

    pub fn add_decide(&mut self, level: usize, literal: i64, reason: Option<usize>) {
        self.decisions += 1;
        self.events.push(TraceEvent::Decide(Decision {
            level,
            literal,
            reason,
        }));
    }

    pub fn add_propagate(&mut self, literal: i64, antecedent: usize, level: usize) {
        self.propagations += 1;
        self.events.push(TraceEvent::Propagate(Propagation {
            literal,
            antecedent,
            level,
        }));
    }

    pub fn add_conflict(&mut self, level: usize, clause: usize, analysis: Vec<i64>) {
        self.conflicts += 1;
        self.events.push(TraceEvent::Conflict(Conflict {
            level,
            conflicting_clause: clause,
            analysis,
        }));
    }

    pub fn add_backtrack(&mut self, target: usize, learnt: Vec<i64>) {
        self.backtracks += 1;
        self.events.push(TraceEvent::Backtrack(Backtrack {
            target_level: target,
            learnt_literals: learnt,
        }));
    }

    /// The trace tells us which decisions mattered and which constraints drove propagation.
    /// This is the "learned program" — compile this to AVX-512 for stateless execution.
    pub fn decision_depth(&self) -> usize {
        self.events.iter()
            .filter(|e| matches!(e, TraceEvent::Decide(_)))
            .count()
    }

    /// The conflict clauses are the "learned knowledge" — compile these to AVX-512.
    pub fn learned_clauses(&self) -> Vec<&[i64]> {
        self.events.iter()
            .filter_map(|e| match e {
                TraceEvent::Learn { literals, .. } => Some(literals.as_slice()),
                _ => None,
            })
            .collect()
    }

    /// Compile trace to a decision program (sequence of decisions to try first)
    pub fn decision_program(&self) -> Vec<i64> {
        self.events.iter()
            .filter_map(|e| match e {
                TraceEvent::Decide(d) => Some(d.literal),
                _ => None,
            })
            .collect()
    }
}

impl Default for CDCLTrace {
    fn default() -> Self {
        Self::new()
    }
}
