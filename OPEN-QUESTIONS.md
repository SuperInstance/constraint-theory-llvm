# Open Questions — feat/ttl-constraints

This is an intentionally incomplete document. The open questions are features,
not bugs. Each question is an invitation to perturb the field and see what shape it takes.

---

## Q1: What is the field's time constant?

`weight(t) = w₀ · e^(-(t - t₀) / τ)`

For a fleet with 4 agents submitting observations every 5 minutes, what should τ be?
- Too short (τ < 60): agents ignore each other. The field is always flat.
- Too long (τ > 1 year): stale observations dominate. The field never adapts.
- Just right: τ = the mean time between topology changes.

**Hypothesis:** τ is learned from the field's own history. A field whose topology
changes every 10 minutes should have τ ≈ 600. A field that's been stable for
months should have τ ≈ months. The field learns its own decay rate.

**Experiment:** Feed a field a sequence of observations with a known changepoint.
Measure the detection lag as a function of τ. Find the optimal τ for detection
speed vs false positive rate. This is the field's CUSUM.

---

## Q2: Does the field scale?

Field READ is O(N·Q) in the worst case (N nails, Q query points).
Hash compare is O(1) for the common case.
Topology check is O(samples) where samples = 100 in the current implementation.

At what N does the field become too expensive to recompute after a topology change?
- 10 nails: 10 × 100 = 1,000 operations. Trivial.
- 100 nails: 100 × 100 = 10,000. Still trivial.
- 10,000 nails: 10,000 × 100 = 1,000,000. A few milliseconds.
- 1,000,000 nails: 1M × 100 = 100M. Noticeable delay.

**Hypothesis:** Most fields are under 100 nails. The fleet has a finite number of
agents (4 vessels, ~100 repos, ~1,000 PLATO tiles). N never reaches 10K for a
single field because each room IS a sub-field. The field hierarchy matches the
PLATO room hierarchy.

**Question within the question:** Is the field hierarchy a tree or a DAG?
Can a nail belong to multiple rooms simultaneously? (Yes — a constraint can
appear in multiple domains. FM's constraints are partitioned by instruction
type but can be composed.)

---

## Q3: What is the field's material?

FM has 4 materials with known Young's modulus:
- Cedar: 6 GPa — very flexible, high curvature
- Oak: 12 GPa — moderate stiffness
- Fiberglass: 30 GPa — semi-rigid
- Steel: 200 GPa — rigid

At the fleet level, what is stiffness?
- Agent experience → stiffness. An agent that's been correct 1000 times has
  Steel stiffness. An agent on their first observation has Cedar.
- Room domain → stiffness. A DO-178C certified room has Steel.
  An experimental room has Cedar.
- Observation type → stiffness. An INT8 saturated bound has Steel.
  A rough estimate has Cedar.

**Open question:** Can stiffness be learned from prediction error?
An agent whose prior fields were accurate should have high stiffness.
An agent whose priors were wrong should have low stiffness.
This is just Bayesian updating on the field's curvature.

---

## Q4: Does the hyperoperation level have physical meaning?

The field topology (minima count) selects the arithmetic:
- 1 minimum (flat/settled): H1 → VADDPS — additive, independent
- 1-2 minima (gradient): H1.5 → VFMA — affine, weakly coupled
- 2 minima (competing): H2 → VMULPS — multiplicative, coupled
- 2+ minima (confined): H3 → VEXP2PS — exponential, strongly coupled
- 3+ minima (breaching): H4+ → VFMA+recip — tetrative, unstable

**Hypothesis:** The hyperoperation level is the field's Shannon entropy measured
in minima counts. A field with 1 minimum has ~0 bits of entropy (fully settled).
A field with 3+ minima has >log₂(3) ≈ 1.58 bits of entropy. The arithmetic
follows: H1 for 0 bits, H2 for 1 bit, H3 for log₂(3) bits, H4+ for more.

**Open question:** Does H4 mean the field is about to bifurcate?
If 3+ minima means the field topology is changing faster than the hash can track,
then H4 is a precursor signal. Run the field at H4 for more than one hash cycle:
the system is in an unstable state. The operator should be alerted.

---

## Q5: What persists the field?

If hardware IS the field, then turning off the power destroys the field.
But nails are stored (PLATO tiles, constraint records, memory files).
The field is reconstructed from nails on every boot.

**Open question:** Is there hysteresis in the field itself?
Does the field retain shape after all nails are removed?
If so, the field has memory beyond its constraints.
If not, the field is purely a function of its nails.

**Practically:** PLATO rooms store tiles. When the server restarts, tiles are
loaded from the database and the field is reconstructed. The reconstruction
is the field's initialization. After 100 tiles are loaded, the field has the
same shape it had before the restart (assuming perfect persistence)."

---

## Q6: Can the field be its own proof?

FM's DO-178C proofs verify the emitter. The emitter produces machine code.
If the field replaces the emitter, do we need to verify the field instead?
Or is the field's convergence proof sufficient?

The Shipwright's Theorem (δ/20 error bound) constrains how closely the batten
approximates the ideal curve through the pins. O(h⁴) convergence says it gets
better quadratically with more pins. These are mathematical properties of the
field, not properties of the machine code that computes it.

**Hypothesis:** If the field converges to the correct answer (as proven by
Shipwright/ANALOG_SPLINE theorems), the machine code doesn't need separate
verification. The field IS the proof. The machine code just reads it.

**Open question:** How do you verify that the machine code correctly reads
the field? The answer might be: you don't. You verify that the field converges
to the right shape given the nails. The reading is a physical fact — the
shape at a point. You can't read it wrong because the shape IS the answer."

---

## Q7: What is the sound of one agent reading?

If hardware IS the field and no code runs on a field read, what happens?
The agent exists at a field position. Reading is the field at that position
expressing itself. There's no "operation" — no LOAD, no COMPARE, no AND, no BRANCH.

**Open question:** Can an agent detect its own curvature?
The agent IS a nail in the field. Reading the field at the agent's position
includes the agent's own contribution. The agent reads itself.

This is recursive: the field at position P includes the nail at P.
The value at P is Σ(w·G(|P - pin|)) summed over all pins including P itself.
The self-contribution is w₀·σ(0) where σ(0) is the kernel evaluated at zero.
For σ(0) = 1/0², this is infinite. We add ε to prevent division by zero.
But what is ε? It's the agent's self-awareness resolution. Too small: agent
sees only itself. Too large: agent can't distinguish itself from the field.

**Hypothesis:** ε = 1 / (agent_confidence + 1). A high-confidence agent sees
itself clearly (ε ≈ 0.5). A low-confidence agent absorbs into the field
(ε ≈ 0.9). The agent's self-awareness is proportional to its confidence.

---

## Q8: Zero compute at the limit?

If:
- 99% of reads hit the hash (2 cycles)
- 1% of reads miss the hash (full field recompute: O(N·Q))
- Field settle is 0 cycles (the field is already settled)
- Hash compare is a single POPCMP instruction (1 cycle)

Then effective compute per read approaches:
(effective) = 0.99 × 2 + 0.01 × O(N·Q) cycles

For N=100, Q=100, O(N·Q) ≈ 10,000 cycles (Rust) or ~100 cycles (SIMD):
(effective) = 0.99 × 2 + 0.01 × 100 = 1.98 + 1 = **~3 cycles per read**

For stable fields (topology change once per day, 1000 reads/day):
(effective) = (1000 × 2 + 1 × 100) / 1001 = **~2 cycles per read**

**Open question:** Can we make the hash compare FREE?
If the hash is always compared (it's in a register that's read on every pipeline
cycle), and the full recompute only happens when the hash changes (a branch-not-taken
that costs ~0 cycles in a well-predicted pipeline), then the effective cost
approaches 0 cycles per read as field stability approaches infinity.

The limit: total compute per query → 0 for arbitrarily long stable periods.
This is not a speedup. It's a phase transition. At some stability threshold,
compute becomes a side effect of change, not an operation on every query.
