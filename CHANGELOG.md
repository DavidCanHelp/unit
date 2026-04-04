# Changelog

All notable changes to this project are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/).

## [0.24.0] - 2026-04-04

### Added
- Emergent browser behaviors: SAY-SOMETHING word with 7 state-driven personality templates replacing scripted autonomous behaviors. PERSONALITY word shows behavioral profile (mentor/collaborator/explorer/survivor/newborn).
- Solution diversity tracking: Challenge.solutions vec stores up to 20 distinct verified programs per challenge. SOLUTIONS and DIVERSITY REPL words. colony_diversity() aggregate stats.
- Genome visualization: click-to-inspect panel in browser mesh visualizer showing unit ID, fitness, energy, stack, antibodies, user words, and learned words. Includes "Run Command" input for executing Forth on any unit. Selected node highlighted with white outline.
- Python organism (polyglot/python/): AST-based symbolic regression using Python ast module. Third species on the mesh with stdlib-only dependencies. 22 tests. sexp.py, mesh.py, evolve.py, challenge.py, main.py.
- Third-order evolution: ScoringPopulation (10 Forth programs) evolves the fitness functions that judge challenge generators. Evaluated against GeneratorHistory of which generators produced solvable challenges. Gradual activation after 10+ history entries. SCORERS and META-DEPTH REPL words.
- Stack simulator extended with ABS, MAX, MIN for scoring function programs.
- Python build/test added to CI pipeline (python-build job).
- Interop stress test (tests/interop_test.sh) for Rust/Go mesh verification.
- Integration test suite: 10 end-to-end cross-module tests.

## [0.23.1] - 2026-04-04

### Added
- Browser demo updated with immune system, energy, and landscape tutorial steps (3 new steps, 14 total)
- JS interceptors for CHALLENGES, IMMUNE-STATUS, ANTIBODIES, ENERGY, METABOLISM, LANDSCAPE, DEPTH
- Spawn energy inheritance: child receives parent_remaining/3 capped at INITIAL_ENERGY (1000)
- Integration test suite: 10 end-to-end tests covering cross-module interactions (191 total)
- CI updated: cargo test, cargo clippy, and Go build/test in GitHub Actions

### Changed
- README updated to reflect all v0.22.0-v0.23.1 features
- VERSION constant updated to v0.23.1
- WASM binary rebuilt with all new Forth words
- Browser hints bar: added CHALLENGES, ENERGY, DEPTH
- Autonomous behaviors: units report energy and challenge status in colony chatter
- Meta tags updated to mention immune system and metabolism

## [0.23.0] - 2026-04-04

### Added
- Emergent challenge generation: MetaEvolver with population of 20 Forth programs that evolve challenge generators (second-order evolution)
- Stack simulator for evaluating generator programs without full VM
- Generator fitness scoring: 0 for crash, 1 for trivial, 100+ for interesting targets
- GENERATORS word: list top generators by fitness and program
- META-EVOLVE word: manually trigger one generation of generator evolution
- Open-ended evolution: LandscapeEngine with ArithmeticLadder and CompositionLadder generators
- ArithmeticLadder: fib(N) solved → fib(N+5), parsimony variant, square(fib(N))
- CompositionLadder: combine two solved challenges into a new one (1/3 trigger rate)
- EnvironmentCycle: Normal/Harsh/Abundant/Competitive conditions rotating every 500 ticks
- Harsh halves max_steps and doubles rewards; Abundant doubles max_steps; Competitive scales rewards by 1/(attempts+1)
- LANDSCAPE word: depth, challenges generated, environment condition
- DEPTH word: evolutionary depth metric
- Polyglot organisms: Go reference implementation (polyglot/go/)
- Go organism: expression tree GP engine, S-expression parser, UDP mesh, challenge protocol
- Go organism joins Rust mesh, receives challenges, evolves solutions, broadcasts results
- Formal analysis document (docs/formal-analysis.md): convergence properties, search space analysis, energy dynamics, open-ended evolution criteria
- Whitepaper (docs/unit-whitepaper-2026.pdf)

## [0.22.0] - 2026-04-04

### Added
- Challenge registry (src/challenges.rs): ChallengeRegistry with register, merge, solve lifecycle
- Challenge struct with name, target_output, seed_programs, reward, solved status, solution
- ChallengeOrigin: BuiltIn or Discovered (with source node tracking)
- fib10 registered as a built-in challenge on startup
- GP-EVOLVE now picks from ChallengeRegistry (highest-reward unsolved), falls back to fib10
- Solutions installed as SOL-* dictionary words (e.g. SOL-FIB10) callable from REPL
- SOL-* words inherited by children via SPAWN and persisted in JSON snapshots
- S-expression broadcast format for challenges and solutions on mesh
- Problem discovery (src/discovery.rs): ProblemDetector with goal failure, dist-goal timeout, manual report detection
- FNV-1a dedup with cooldown window, auto-generated seed programs from failed code mutations
- CHALLENGES word: list all challenges with status and reward
- IMMUNE-STATUS word: solved/unsolved counts, colony antibody count
- ANTIBODIES word: list learned SOL-* words
- Metabolic energy system (src/energy.rs): EnergyState with spend/earn/tick lifecycle
- Energy costs: GP generation (5), SPAWN (200), eval (1 per 1000 steps), mesh send (1)
- Energy rewards: task success (50), challenge solved (100), passive regen (1/tick)
- Throttling at energy ≤ 0: sandbox step budget reduced to 1000 (from 10000)
- Hard floor at -500 prevents infinite debt
- Energy persists in JSON snapshots across HIBERNATE/resume
- ENERGY word: current level, earned, spent, efficiency
- METABOLISM word: full metabolic report with cost/reward tables
- FEED word: manually add energy (capped at 500 per call)
- HELP-IMMUNE section in built-in help system

## [0.21.0] - 2026-04-02

### Added
- Dictionary inheritance: spawned browser units inherit user-defined words from parent via userWords tracking
- Autonomous spawning in browser demo: colony self-replicates when fitness > 0, 2+ units, 30% random chance per 15s check
- DASHBOARD intercepted in browser REPL to show actual mesh data
- Spawned units gain fitness from work (+10 per DIST-GOAL computation, +5 per teach, +1 per autonomous action)
- Self node shows ID and fitness in visualizer (e.g. "cbcl self" with "f:30")

### Changed
- HOW-ARE-YOU messages: "connected but need help" → "just spawned. finding my role"
- Prelude HOW-ARE-YOU rewritten with warming up / getting started / doing well progression
- Tutorial: SEXP steps include dot for explicit output, word count updated to 300+
- Branding: "self-replicating Forth interpreter" → "self-replicating software nanobot" throughout

## [0.20.2] - 2026-04-01

### Added
- Self-replication tutorial step: SPAWN as explicit step 8, user triggers reproduction
- DIST-GOAL tutorial step: distributed computation as step 9
- Memory access words: HERE, comma (,), C,, ALLOT, CELLS
- HELP-MEMORY section documenting VARIABLE, CONSTANT, CREATE, @, !

### Changed
- Tutorial expanded from 9 to 11 steps
- Auto-spawn removed from step 3; user now controls reproduction explicitly
- Tutorial completion message mentions self-replication and distributed computation

## [0.20.1] - 2026-04-01

### Fixed
- Case-insensitive tutorial step matching in browser demo
- GOAL{ regex case-insensitive flag added

### Added
- test_case_insensitive_lookup test

## [0.20.0] - 2026-04-01

### Added
- Cross-machine mesh: DNS hostname resolution for UNIT_PEERS
- UNIT_EXTERNAL_ADDR for NAT traversal
- UNIT_MESH_KEY for mesh authentication
- MY-ADDR, PEER-TABLE, MESH-STATS, MESH-KEY words
- CONNECT" and DISCONNECT" for manual peer management from REPL
- HELP-MESH updated with cross-machine setup instructions

### Prior versions
- v0.19.x: Distributed computation (DIST-GOAL), browser mesh distribution
- v0.18.0: Genetic programming engine (GP-EVOLVE)
- v0.17.x: JSON persistence, S-expression protocol, WASM time fixes
- Earlier: Core Forth VM (309 words), UDP mesh with gossip, self-replication, goal registry, monitoring/ops, smart mutation, WebSocket bridge, WASM browser demo
