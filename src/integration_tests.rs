// integration_tests.rs — End-to-end tests for v0.22.0-v0.23.0 systems
//
// Tests the immune system, energy, landscape, and meta-evolution
// working together across module boundaries.

#[cfg(test)]
mod tests {
    use crate::challenges::*;
    use crate::discovery::*;
    use crate::energy::*;
    use crate::evolve;
    use crate::landscape::*;
    use crate::mesh::NodeId;

    fn test_node() -> NodeId {
        [0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44]
    }

    // -----------------------------------------------------------------------
    // 1. Immune system lifecycle
    // -----------------------------------------------------------------------

    #[test]
    fn test_immune_lifecycle() {
        let mut reg = ChallengeRegistry::new(&test_node());
        let fib = fib10_as_challenge();
        let id = reg.register_builtin(&fib.name, &fib.target_output, fib.seed_programs.clone());

        // Convert to FitnessChallenge for GP.
        let fc = reg.to_fitness_challenge(id).unwrap();
        assert_eq!(fc.name, "fib10");
        assert_eq!(fc.target_output, "55 ");

        // Simulate a GP solve.
        let solution = "0 1 10 0 DO OVER + SWAP LOOP DROP .";
        let score = evolve::score_candidate("55 ", true, "55 ", 11);
        assert!(score >= 800.0); // fitness=890

        // Mark solved.
        assert!(reg.mark_solved(id, solution, test_node()));
        assert!(reg.get_challenge(id).unwrap().solved);
        assert_eq!(reg.get_challenge(id).unwrap().solution.as_deref(), Some(solution));

        // Feed to landscape — should generate harder challenges.
        let mut engine = LandscapeEngine::new();
        let solved = reg.get_challenge(id).unwrap().clone();
        let all_solved: Vec<&Challenge> = vec![&solved];
        let new = engine.on_challenge_solved(&solved, solution, &all_solved);
        assert!(new.len() >= 2, "expected at least 2 child challenges, got {}", new.len());

        // Should contain fib15 and parsimony challenge.
        assert!(new.iter().any(|c| c.name.contains("fib15")));
        assert!(new.iter().any(|c| c.name.contains("short")));

        // Depth should have increased.
        assert!(engine.depth() > 0);
    }

    // -----------------------------------------------------------------------
    // 2. Energy + evolution interaction
    // -----------------------------------------------------------------------

    #[test]
    fn test_energy_evolution_interaction() {
        let mut e = EnergyState::new();
        assert_eq!(e.energy, INITIAL_ENERGY); // 1000

        // Spend GP_GENERATION_COST repeatedly.
        let mut gens = 0;
        while e.can_afford(GP_GENERATION_COST) {
            assert!(e.spend(GP_GENERATION_COST, "gp-gen"));
            gens += 1;
        }
        // With 1000 energy and cost 5, should get 300 gens
        // (1000 + 500 floor = 1500 / 5 = 300)
        assert!(gens > 200, "expected many generations, got {}", gens);

        // Should be at or below floor.
        assert!(e.is_throttled());
        assert!(!e.can_afford(GP_GENERATION_COST));

        // Earn enough to recover from throttled state.
        // Energy is near -500 (hard floor), need to earn past 0 threshold.
        e.earn(CHALLENGE_SOLVE_REWARD, "challenge"); // +100
        e.earn(CHALLENGE_SOLVE_REWARD, "challenge"); // +100
        e.earn(CHALLENGE_SOLVE_REWARD, "challenge"); // +100
        e.earn(CHALLENGE_SOLVE_REWARD, "challenge"); // +100
        e.earn(CHALLENGE_SOLVE_REWARD, "challenge"); // +100
        // Now at about -500 + 500 = 0, may need one more.
        e.earn(TASK_REWARD, "task"); // +50, should push above 0
        assert!(!e.is_throttled());
        assert!(e.energy > 0);
        assert!(e.can_afford(GP_GENERATION_COST));
    }

    // -----------------------------------------------------------------------
    // 3. Discovery pipeline
    // -----------------------------------------------------------------------

    #[test]
    fn test_discovery_pipeline() {
        let mut det = ProblemDetector::new();

        // Detect a goal failure.
        det.detect_goal_failure(
            42, 7,
            "10 0 DO I SQUARE . LOOP",
            "unknown word SQUARE",
            Some("0 1 4 9 16 25 36 49 64 81 "),
        );

        // Drain and convert to challenge params.
        let problems = det.drain_pending();
        assert_eq!(problems.len(), 1);
        // Drain again — should be empty.
        assert_eq!(det.drain_pending().len(), 0);

        let (name, desc, target, _test_input, seeds, reward) =
            ProblemDetector::problem_to_challenge_params(&problems[0]);
        assert!(name.starts_with("auto-"));
        assert!(desc.contains("goal task failed"));
        assert_eq!(target, "0 1 4 9 16 25 36 49 64 81 ");
        assert!(seeds.len() >= 3); // original + 2 mutations
        assert_eq!(seeds[0], "10 0 DO I SQUARE . LOOP");
        assert_eq!(reward, 50); // goal failure reward

        // Register in ChallengeRegistry.
        let mut reg = ChallengeRegistry::new(&test_node());
        let id = reg.register_discovered(
            &name, &desc, &target, None, seeds, test_node(), reward,
        );
        let ch = reg.get_challenge(id).unwrap();
        assert!(!ch.solved);
        assert_eq!(ch.reward, 50);
        assert!(ch.name.starts_with("auto-"));
    }

    // -----------------------------------------------------------------------
    // 4. Landscape depth progression
    // -----------------------------------------------------------------------

    #[test]
    fn test_landscape_depth_progression() {
        let mut engine = LandscapeEngine::new();

        // Solve fib10.
        let fib10 = Challenge {
            id: 1, name: "fib10".into(), description: "".into(),
            target_output: "55 ".into(), test_input: None, max_steps: 10000,
            seed_programs: vec![], origin: ChallengeOrigin::BuiltIn,
            reward: 100, solved: true,
            solution: Some("0 1 10 0 DO OVER + SWAP LOOP DROP .".into()),
            solver: Some(test_node()), attempts: 1,
        };
        let gen1 = engine.on_challenge_solved(
            &fib10, "0 1 10 0 DO OVER + SWAP LOOP DROP .", &[&fib10],
        );
        let depth1 = engine.depth();
        assert!(depth1 > 0, "depth should increase after fib10");

        // Find fib15 in generated challenges.
        let fib15_ch = gen1.iter().find(|c| c.name == "fib15");
        assert!(fib15_ch.is_some(), "should generate fib15");
        assert_eq!(fib15_ch.unwrap().target_output, "610 ");

        // Simulate solving fib15.
        let mut fib15 = fib15_ch.unwrap().clone();
        fib15.id = 2;
        fib15.solved = true;
        fib15.solution = Some("0 1 15 0 DO OVER + SWAP LOOP DROP .".into());
        fib15.solver = Some(test_node());
        let all_solved = vec![&fib10, &fib15];
        let gen2 = engine.on_challenge_solved(
            &fib15, "0 1 15 0 DO OVER + SWAP LOOP DROP .", &all_solved,
        );

        // Should generate fib20.
        let fib20 = gen2.iter().find(|c| c.name == "fib20");
        assert!(fib20.is_some(), "should generate fib20");
        assert_eq!(fib20.unwrap().target_output, "6765 ");

        // Verify Fibonacci targets are correct.
        assert_eq!(fib(10), 55);
        assert_eq!(fib(15), 610);
        assert_eq!(fib(20), 6765);
    }

    // -----------------------------------------------------------------------
    // 5. Meta-evolution
    // -----------------------------------------------------------------------

    #[test]
    fn test_meta_evolution() {
        let mut rng = crate::features::mutation::SimpleRng::new(42);
        let mut pop = GeneratorPopulation::new(&mut rng);

        // Verify seed generators are present.
        assert_eq!(pop.genomes.len(), 20);
        assert!(pop.genomes.iter().any(|g| g.program == "5 +"));
        assert!(pop.genomes.iter().any(|g| g.program == "DUP *"));

        // Evaluate against target 55.
        pop.evaluate_all(55);

        // Valid generators should have non-zero fitness.
        let five_plus = pop.genomes.iter().find(|g| g.program == "5 +").unwrap();
        assert!(five_plus.fitness > 0.0, "5 + should score > 0 against 55");

        // Crash generators should score 0.
        let (_, crash_score) = evaluate_generator("DROP DROP DROP", 55);
        assert_eq!(crash_score, 0.0);

        // Run one generation of meta-evolution.
        let gen_before = pop.generation;
        pop.evolve_generators(&mut rng);
        assert_eq!(pop.generation, gen_before + 1);
        assert_eq!(pop.genomes.len(), 20);
        assert!(pop.best.is_some());
    }

    // -----------------------------------------------------------------------
    // 6. Challenge merge convergence
    // -----------------------------------------------------------------------

    #[test]
    fn test_challenge_merge_convergence() {
        let node_a = [0x11; 8];
        let node_b = [0x22; 8];
        let mut reg_a = ChallengeRegistry::new(&node_a);
        let mut reg_b = ChallengeRegistry::new(&node_b);

        // Register on A.
        let id = reg_a.register_builtin("shared-test", "42 ", vec!["42 .".into()]);
        let ch = reg_a.get_challenge(id).unwrap().clone();

        // Merge into B — should appear.
        reg_b.merge_challenge(ch.clone());
        assert!(reg_b.get_challenge(id).is_some());
        assert!(!reg_b.get_challenge(id).unwrap().solved);

        // Mark solved on A.
        reg_a.mark_solved(id, "42 .", node_a);
        let solved = reg_a.get_challenge(id).unwrap().clone();

        // Merge solved into B — should propagate solved status.
        reg_b.merge_challenge(solved);
        assert!(reg_b.get_challenge(id).unwrap().solved);
        assert_eq!(reg_b.get_challenge(id).unwrap().solution.as_deref(), Some("42 ."));
    }

    // -----------------------------------------------------------------------
    // 7. Energy persistence roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_energy_persistence_consistency() {
        let mut e = EnergyState::new();

        // Sequence of operations.
        e.earn(500, "task");
        e.spend(300, "gp");
        e.spend(150, "spawn");
        e.earn(200, "challenge");
        for _ in 0..10 { e.tick(); }

        // Verify consistency.
        assert_eq!(e.total_earned, 500 + 200 + 10); // 500 + 200 + 10 passive
        assert_eq!(e.total_spent, 300 + 150);
        assert!(e.peak_energy >= INITIAL_ENERGY); // started at 1000, earned 500 = 1500 peak
        assert_eq!(e.peak_energy, 1500); // 1000 + 500 = 1500, before any spending
        assert!(!e.is_throttled());

        let expected = INITIAL_ENERGY + 500 - 300 - 150 + 200 + 10;
        assert_eq!(e.energy, expected);

        let eff = e.efficiency();
        assert!((eff - (710.0 / 450.0)).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // 8. Spawn energy inheritance simulation
    // -----------------------------------------------------------------------

    #[test]
    fn test_spawn_energy_inheritance() {
        let mut parent = EnergyState::new();
        parent.energy = 800;

        // Simulate spawn: deduct cost.
        assert!(parent.spend(SPAWN_COST, "spawn"));
        let remaining = parent.energy; // 600
        assert_eq!(remaining, 600);

        // Child gets remaining/3, capped at INITIAL_ENERGY.
        let child_energy = (remaining / 3).min(INITIAL_ENERGY);
        assert_eq!(child_energy, 200); // 600/3 = 200

        // Parent retains remaining.
        assert_eq!(parent.energy, 600);

        // Test with higher energy.
        let mut rich = EnergyState::new();
        rich.energy = 4000;
        rich.spend(SPAWN_COST, "spawn");
        let rich_child = (rich.energy / 3).min(INITIAL_ENERGY);
        assert_eq!(rich_child, 1000); // 3800/3 = 1266, capped at 1000
    }

    // -----------------------------------------------------------------------
    // 9. Environment variation
    // -----------------------------------------------------------------------

    #[test]
    fn test_environment_variation_full_cycle() {
        let mut env = EnvironmentCycle::new();

        // Normal.
        assert_eq!(env.current_condition(), "normal");
        assert_eq!(env.apply_to_max_steps(10000), 10000);
        assert_eq!(env.apply_to_reward(100, 0), 100);

        // Advance to Harsh.
        for _ in 0..500 { env.tick(); }
        assert_eq!(env.current_condition(), "harsh");
        assert_eq!(env.apply_to_max_steps(10000), 5000);
        assert_eq!(env.apply_to_reward(100, 0), 200);

        // Advance to Abundant.
        for _ in 0..500 { env.tick(); }
        assert_eq!(env.current_condition(), "abundant");
        assert_eq!(env.apply_to_max_steps(10000), 20000);
        assert_eq!(env.apply_to_reward(100, 0), 100);

        // Advance to Competitive.
        for _ in 0..500 { env.tick(); }
        assert_eq!(env.current_condition(), "competitive");
        assert_eq!(env.apply_to_reward(100, 0), 100); // 100/(0+1) = 100
        assert_eq!(env.apply_to_reward(100, 3), 25);  // 100/(3+1) = 25
        assert_eq!(env.apply_to_reward(100, 9), 10);  // 100/(9+1) = 10

        // Full cycle back to Normal.
        for _ in 0..500 { env.tick(); }
        assert_eq!(env.current_condition(), "normal");
    }

    // -----------------------------------------------------------------------
    // 10. Cross-module constant consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_constant_consistency() {
        // Verify energy constants are sane.
        assert!(INITIAL_ENERGY > 0);
        assert!(MAX_ENERGY > INITIAL_ENERGY);
        assert!(SPAWN_COST > 0);
        assert!(SPAWN_COST < INITIAL_ENERGY); // must be able to spawn from initial energy
        assert!(GP_GENERATION_COST > 0);
        assert!(GP_GENERATION_COST < SPAWN_COST); // evolution cheaper than reproduction
        assert!(TASK_REWARD > GP_GENERATION_COST); // tasks should pay for some evolution
        assert!(CHALLENGE_SOLVE_REWARD > TASK_REWARD); // solving challenges is the big payoff
        assert!(PASSIVE_REGEN > 0);

        // Verify a new unit can afford at least one spawn.
        let e = EnergyState::new();
        assert!(e.can_afford(SPAWN_COST));

        // Verify a new unit can afford many GP generations.
        assert!(INITIAL_ENERGY / GP_GENERATION_COST > 100);
    }
}
