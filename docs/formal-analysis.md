# Formal Analysis of unit's Evolutionary and Distributed Systems

David Liedle · DavidCanHelp · April 2026

This document characterizes the formal properties of unit's evolutionary
computation, distributed coordination, and metabolic systems. It serves
as a companion to the unit whitepaper and is written for readers familiar
with artificial life, evolutionary computation, and distributed systems.

---

## 1. Convergence Properties

### 1.1 Genetic Programming Engine

Unit's GP engine uses tournament selection (size 3) with elitism (top 5
preserved per generation) over a population of 50 candidate Forth programs.

**Monotonic best-fitness guarantee.** Because the top 5 candidates are
copied unchanged into each subsequent generation, the best fitness score
in the population is non-decreasing:

    best(g+1) ≥ best(g)  ∀g

This is a standard property of elitist evolutionary algorithms. The
guarantee holds per-challenge: when the GP engine switches to a new
challenge, the best score resets to the new population's initial fitness.

**Convergence rate.** With tournament size k=3 and population size N=50,
selection pressure is moderate. The probability that the best individual
is selected in a single tournament is:

    P(best selected) = 1 - ((N-1)/N)^k = 1 - (49/50)^3 ≈ 0.059

Over a full generation (N tournaments), the expected number of copies of
the best individual is approximately N × P ≈ 2.9, plus the 5 elite copies,
yielding roughly 8 copies per generation. This creates strong but not
overwhelming convergence pressure, leaving room for exploration via
mutation and crossover.

The 80/20 mutation-to-crossover ratio (crossover triggers with probability
0.2) favors local search over recombination, which is appropriate for
Forth's concatenative structure where small token changes often produce
meaningful behavioral variation.

### 1.2 Mesh Gossip Protocol

Unit's peer discovery operates via a gossip protocol over UDP. When node A
knows about node B, and node B knows about node C, A eventually discovers
C through B's peer list sharing.

**Propagation time.** In a mesh of N nodes where each node gossips with
all known peers at a fixed interval t, a new piece of information (peer
address, challenge, solution) propagates to all nodes in O(log N) gossip
rounds. This follows from the standard analysis of epidemic gossip
protocols: if each informed node tells all its peers, the number of
informed nodes roughly doubles each round until saturation.

For unit's specific implementation, propagation time is bounded by:

    T_propagate ≤ t × ⌈log₂(N)⌉

where t is the gossip interval. With a typical gossip interval of 5
seconds and a 16-node mesh, full propagation takes approximately 20
seconds.

**Challenge propagation** follows the same gossip dynamics. A challenge
broadcast by one node reaches all nodes in O(log N) rounds. Solutions
propagate identically.

### 1.3 Challenge Registry Convergence

The ChallengeRegistry uses merge semantics inspired by conflict-free
replicated data types (CRDTs). The merge rule is:

- If a challenge ID is unknown, accept it.
- If a challenge ID exists and the incoming version is solved while the
  local version is unsolved, accept the update.
- Otherwise, keep the local version.

The solved status forms a monotonic join-semilattice:

    unsolved → solved  (irreversible)

This guarantees eventual consistency: regardless of message ordering or
network partitions, all nodes will eventually agree on which challenges
are solved. No conflict resolution is needed because the solved state can
only advance forward, never retreat.

The GoalRegistry uses an analogous lattice for goal status:

    Pending → Active → Completed
                    → Failed

Both status orderings are encoded as u8 values with merge-by-maximum
semantics, ensuring convergence under arbitrary message reordering.

---

## 2. Search Space and Evolutionary Capacity

### 2.1 Program Space

The GP engine operates over Forth programs represented as token sequences.
The vocabulary V consists of 30 tokens: integers 0–10 plus 20, 55;
arithmetic operators +, -, *; stack operations DUP, DROP, SWAP, OVER, ROT;
comparisons <, >, =; control flow IF, THEN, ELSE, DO, LOOP, I; and output
(the `.` word).

For programs of length L tokens, the search space is:

    |S(L)| = |V|^L = 30^L

With a maximum program length of 30 tokens (enforced by crossover
truncation), the total search space is:

    |S| = Σ(L=1 to 30) 30^L ≈ 30^30 ≈ 2.06 × 10^44

This is vastly larger than exhaustive search can cover. The GP engine's
effectiveness depends on the fitness landscape being navigable by local
mutations — a property that Forth's concatenative structure supports.

### 2.2 Mutation Robustness

Forth's concatenative nature means programs are flat sequences of tokens
with no nested syntactic structure. This has a critical consequence for
evolvability: most mutations produce programs that execute without
crashing.

Consider the five mutation operators:

1. **Token swap**: Reorders two tokens. The result is always syntactically
   valid Forth (though it may produce a runtime error like stack underflow).

2. **Token insert**: Adds a random token. May create stack imbalance but
   the program still parses and begins execution.

3. **Token delete**: Removes a token. Same considerations as insert.

4. **Token replace**: Substitutes one token for another. Always produces
   parseable Forth.

5. **Double mutation**: Two replacements. Same properties as single replace.

Contrast this with tree-structured languages (Lisp, most functional
languages) where a random character insertion almost certainly produces a
parse error. In Forth, the parse-to-execute rate under random mutation is
high — we estimate >95% of single-token mutations produce programs that
begin execution (though many will encounter runtime errors like stack
underflow before completing).

This mutation robustness is the primary reason Forth was chosen as the
cognitive substrate. It creates a smooth fitness landscape where small
mutations produce small behavioral changes — the gradient that evolution
requires.

### 2.3 Lamarckian Dynamics

Unit's immune system introduces a Lamarckian evolutionary dynamic that
departs from strict Darwinian evolution. When a unit solves a challenge,
the solution is installed as a dictionary word (SOL-*) that is inherited
by children via SPAWN. This is inheritance of acquired characteristics:
knowledge gained during the organism's lifetime is passed to offspring.

In biological terms, this is analogous to horizontal gene transfer in
bacteria, where organisms share genetic material directly rather than
only through vertical inheritance. The mesh broadcast of solutions
functions as a colony-wide horizontal gene transfer mechanism.

The consequence is accelerated adaptation. In Darwinian evolution, a
beneficial mutation must spread through differential reproduction over
many generations. In unit's Lamarckian system, a beneficial solution
spreads to the entire colony in O(log N) gossip rounds — potentially
within seconds.

This acceleration comes with a trade-off: Lamarckian inheritance can
propagate suboptimal solutions that happen to pass verification but
are not globally optimal. The parsimony pressure in the fitness function
(rewarding shorter programs) partially mitigates this by favoring
efficient solutions, but does not guarantee optimality.

### 2.4 Go Organism: Alternative Search Strategy

The polyglot Go organism uses arithmetic expression trees rather than
Forth token sequences. This creates a fundamentally different search
space: binary trees with operators {+, -, *, mod} and integer leaves.

For trees of depth d, the space is approximately:

    |T(d)| ≈ (4 × |constants|)^(2^d - 1)

Expression tree mutation (subtree replacement) operates on a structured
representation, meaning mutations tend to preserve more semantic
structure than Forth token mutations. However, expression trees are less
robust to arbitrary mutation — a subtree swap can dramatically change
program behavior.

The coexistence of Forth organisms and expression-tree organisms on the
same mesh creates niche differentiation: Forth organisms excel at
problems requiring sequential computation (loops, accumulation), while
expression-tree organisms excel at problems expressible as closed-form
arithmetic. This mirrors biological ecosystems where different species
occupy different ecological niches.

---

## 3. Resource Budget Dynamics

### 3.1 Energy Equilibrium

Each unit maintains an energy budget governed by earnings and expenditures.
The steady-state energy level E* for a unit can be characterized by the
balance equation:

    dE/dt = R_passive + R_tasks + R_challenges - C_eval - C_mesh - C_gp - C_spawn

Where:
- R_passive = 1 per tick (passive regeneration)
- R_tasks = 50 per successful task completion (rate: r_task tasks/tick)
- R_challenges = 100 + reward per challenge solved (rate: r_solve solves/tick)
- C_eval = steps/1000 per evaluation (rate: r_eval evals/tick)
- C_mesh = 1 per message sent (rate: r_msg messages/tick)
- C_gp = 5 per GP generation (rate: r_gen generations/tick)
- C_spawn = 200 per replication event (rate: r_spawn spawns/tick)

At equilibrium (dE/dt = 0):

    E* is stable when total earnings = total costs

A unit that solves challenges faster than it spends energy on evolution
will accumulate energy and approach the cap (5000). A unit that runs
expensive GP evolution without finding solutions will drain toward zero.

### 3.2 Throttling as Negative Feedback

When energy drops to or below zero, the unit enters a throttled state
where sandbox evaluation is limited to 1000 steps (down from 10000).
This creates a natural negative feedback loop:

1. Energy drops below zero → throttled
2. Throttled → reduced computation → reduced energy expenditure
3. Reduced expenditure + passive regen → energy recovers
4. Energy rises above zero → throttling lifted
5. Full computation resumes

This feedback loop prevents permanent starvation. The system oscillates
around the starvation threshold with decreasing amplitude, eventually
settling at a low but positive energy level. The oscillation period
depends on the passive regeneration rate and the unit's computation load.

Formally, the throttled state reduces C_eval by a factor of 10 (1000
vs 10000 step budget), which is typically the dominant cost. If passive
regen exceeds the throttled-state costs:

    R_passive > C_eval_throttled + C_mesh + C_gp_throttled

then recovery is guaranteed. With R_passive = 1/tick and typical
throttled costs < 1/tick, recovery occurs within a bounded number of
ticks.

### 3.3 Spawn Economics

Replication costs 200 energy. Currently, the child inherits the parent's
full energy state via snapshot serialization — the spawn cost is deducted
from the parent but the child starts with a copy of the parent's remaining
energy. This means spawning is metabolically inexpensive for the child but
costly for the parent.

A future refinement would split energy between parent and child (e.g.
child receives parent_energy / 3, parent keeps the rest), making
reproduction a genuine resource investment where both parties start in a
more constrained metabolic state.

The minimum viable energy for spawning is 200 (the cost). Spawning at
minimum leaves the parent near zero energy, likely triggering throttling.
The optimal spawning strategy is to accumulate significantly above the
cost threshold before replicating — a behavior that should emerge
naturally from the energy dynamics without explicit programming.

---

## 4. Open-Ended Evolution Analysis

### 4.1 Depth as a Complexity Metric

The LandscapeEngine tracks evolutionary depth: the maximum number of
challenge generations produced by the solve-then-generate cycle. Depth
increases when a solved challenge generates child challenges with higher
difficulty than the parent.

Depth is a necessary but not sufficient condition for open-ended
evolution. Bedau et al. [9] identify several criteria for open-endedness:

1. **Ongoing generation of novel entities**: Satisfied — each solved
   challenge generates new challenges with different targets and
   constraints. The ArithmeticLadder produces fib(N+5) sequences and
   parsimony variants; the CompositionLadder creates combination
   challenges from pairs of solutions.

2. **Increasing complexity**: Partially satisfied — depth increases
   monotonically and difficulty levels increase along the arithmetic
   ladder. However, complexity is currently measured by target magnitude
   and program length, which are proxy metrics. True complexity measures
   (behavioral complexity, information content) are not yet tracked.

3. **No predetermined ceiling**: Satisfied in principle — the Fibonacci
   sequence is unbounded, and composition challenges can combine
   arbitrarily many prior solutions. In practice, the GP engine's
   ability to solve increasingly difficult challenges will plateau at
   some difficulty level, creating a de facto ceiling.

4. **Emergent dynamics not explicitly programmed**: Partially satisfied —
   the challenge generators are hand-authored (ArithmeticLadder,
   CompositionLadder), which means the types of challenges are
   predetermined even if specific instances are not. True open-endedness
   would require the challenge-generation mechanism itself to evolve.

### 4.2 Environmental Variation

The EnvironmentCycle rotates through four conditions (Normal, Harsh,
Abundant, Competitive) at a fixed interval. This creates temporal
variation in selective pressure:

- **Harsh** environments (halved step budget, doubled rewards) favor
  efficient, compact programs and penalize bloat.
- **Abundant** environments (doubled step budget, normal rewards)
  permit exploration of larger program spaces.
- **Competitive** environments (reward scales as 1/(attempts+1))
  favor early solvers and create temporal urgency.

This cycling prevents the population from over-adapting to any single
condition — a well-known technique in evolutionary computation for
maintaining population diversity (analogous to fluctuating selection
in biology).

The fixed cycle length (500 ticks) means environmental changes are
predictable. Truly open-ended evolution might benefit from stochastic
environment changes, where the timing and nature of environmental
shifts are themselves unpredictable.

### 4.3 Limitations and Future Criteria

Unit does not yet satisfy the strongest definitions of open-ended
evolution. Key gaps:

- **Activity statistics**: Bedau's "class" metric (measuring the
  rate at which genuinely new components appear) is not tracked.
  Adding vocabulary growth rate (new SOL-* words per unit time) and
  solution diversity (distinct programs solving the same challenge)
  would provide stronger evidence.

- **Emergent challenge generation**: The current generators are
  authored, not evolved. A system where units evolve their own
  challenge-generation strategies would be more strongly open-ended.

- **Ecological dynamics**: With polyglot organisms, niche
  differentiation exists in principle (Forth vs. expression trees)
  but competitive dynamics between species are not yet observed
  empirically.

---

## 5. Scalability

### 5.1 Mesh Topology

Unit's UDP gossip protocol has the following scalability properties:

**Peer table**: Each node maintains a list of all known peers. Memory
usage is O(N) where N is the mesh size. With 8-byte node IDs and
associated metadata (address, fitness, energy), each peer entry is
approximately 64 bytes. A 1000-node mesh requires ~64KB of peer table
memory — negligible.

**Gossip bandwidth**: Each gossip round, a node sends a peer-status
message to all known peers. Message size is approximately 100 bytes.
Bandwidth per node per round is:

    B = N × 100 bytes

At 1000 nodes with a 5-second gossip interval, this is 20KB/s per node —
manageable for modern networks. At 10,000 nodes, it rises to 200KB/s,
which may become significant. For meshes beyond this scale, a structured
gossip protocol (partial views, random subsets) would be needed.

**UDP packet size**: S-expression messages must fit within a single UDP
datagram. The practical limit is approximately 1400 bytes (to avoid IP
fragmentation). Most messages (peer-status, challenge, solution) fit
comfortably. Challenge messages with many seed programs could approach
this limit; a chunking protocol would be needed for very large challenges.

### 5.2 Challenge Registry Growth

As evolutionary depth increases, the number of challenges grows. Each
solved challenge can generate 1–3 child challenges. Without pruning,
the registry grows geometrically:

    |C(d)| ≈ Σ(i=0 to d) k^i ≈ k^d / (k-1)

where k is the average branching factor (approximately 2 for the current
generators) and d is the depth. At depth 20, this is approximately
10^6 challenges — each stored with name, description, target, seeds,
and solution. At ~500 bytes per challenge, this is ~500MB.

Pruning strategies for production use:
- Archive solved challenges older than T ticks (keep solution, discard seeds)
- Limit unsolved challenges to the top M by reward (discard low-reward stale challenges)
- Compact composition chains (if A→B→C, archive A when C is solved)

### 5.3 Cross-Species Protocol Overhead

The S-expression wire format adds approximately 30–50% overhead compared
to a binary protocol. For a solution message:

    Binary: challenge_id (8) + program_len (4) + program + solver (8) ≈ 40 bytes
    S-expr: (solution :challenge-id 42 :program "..." :solver "...") ≈ 80 bytes

This overhead is acceptable for the current use case (small programs,
infrequent solutions). The trade-off — human readability, language
independence, no schema versioning — is worthwhile for an organism that
values inspectability (the REPL philosophy).

For high-frequency messages (peer-status every 5 seconds), the overhead
is similarly manageable. A hybrid protocol (binary for high-frequency
gossip, S-expressions for semantic messages) could be considered at scale
but adds protocol complexity.

---

## 6. Summary of Formal Properties

| Property | Status | Evidence |
|---|---|---|
| GP monotonic improvement | Guaranteed | Elitist selection preserves best |
| Gossip convergence | O(log N) rounds | Standard epidemic gossip result |
| Challenge consistency | Eventual | Monotonic solved-status lattice |
| Mutation robustness | >95% executable | Forth concatenative structure |
| Energy stability | Bounded oscillation | Throttling negative feedback loop |
| Spawn viability | Minimum 200 energy | Hard cost threshold |
| Open-ended depth | Monotonically increasing | Solve→generate cycle |
| Cross-species interop | Protocol-level | S-expression wire format |

---

## References

[1] C. Langton, "Artificial Life," in Artificial Life, Addison-Wesley, 1989.

[2] T. S. Ray, "An approach to the synthesis of life," in Artificial Life II, 1992.

[3] C. Ofria and C. O. Wilke, "Avida: A software platform for research in
computational evolutionary biology," Artificial Life, vol. 10, 2004.

[5] B. Agüera y Arcas et al., "Computational Life: How Well-formed,
Self-replicating Programs Emerge from Simple Interaction," arXiv:2406.19108, 2024.

[8] C. Heinemann, "ALIEN: Artificial Life Environment," ALIFE 2024.

[9] M. A. Bedau et al., "Open Problems in Artificial Life," Artificial Life,
vol. 6, pp. 363–376, 2000.
