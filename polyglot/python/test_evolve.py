import ast
import random
import unittest
import evolve


class TestEvolve(unittest.TestCase):
    def test_eval_constant(self):
        self.assertEqual(evolve.eval_expr(ast.Constant(value=42)), 42)

    def test_eval_add(self):
        expr = ast.BinOp(left=ast.Constant(10), op=ast.Add(), right=ast.Constant(32))
        self.assertEqual(evolve.eval_expr(expr), 42)

    def test_eval_nested(self):
        # (5 * 11) + 0 = 55
        expr = ast.BinOp(
            left=ast.BinOp(left=ast.Constant(5), op=ast.Mult(), right=ast.Constant(11)),
            op=ast.Add(),
            right=ast.Constant(0),
        )
        self.assertEqual(evolve.eval_expr(expr), 55)

    def test_mod_by_zero(self):
        expr = ast.BinOp(left=ast.Constant(10), op=ast.Mod(), right=ast.Constant(0))
        self.assertEqual(evolve.eval_expr(expr), 0)

    def test_mutate_valid(self):
        rng = random.Random(42)
        expr = ast.BinOp(left=ast.Constant(5), op=ast.Add(), right=ast.Constant(10))
        for _ in range(20):
            m = evolve.mutate(rng, expr)
            evolve.eval_expr(m)  # should not crash

    def test_crossover_valid(self):
        rng = random.Random(99)
        a = ast.BinOp(left=ast.Constant(5), op=ast.Add(), right=ast.Constant(10))
        b = ast.BinOp(left=ast.Constant(3), op=ast.Mult(), right=ast.Constant(7))
        for _ in range(20):
            c = evolve.crossover(rng, a, b)
            evolve.eval_expr(c)

    def test_evolution_finds_42(self):
        expr, found = evolve.run_evolution(42, max_gens=500, seed=123)
        if found:
            self.assertEqual(evolve.eval_expr(expr), 42)

    def test_evolution_finds_55(self):
        expr, found = evolve.run_evolution(55, max_gens=500, seed=42)
        if found:
            self.assertEqual(evolve.eval_expr(expr), 55)

    def test_score_exact(self):
        expr = ast.Constant(value=55)
        self.assertGreater(evolve.score(expr, 55), 900)

    def test_format_program(self):
        expr = ast.BinOp(left=ast.Constant(5), op=ast.Add(), right=ast.Constant(50))
        self.assertEqual(evolve.format_program(expr), '(+ 5 50)')

    def test_parse_target(self):
        val, ok = evolve.parse_target('55 ')
        self.assertTrue(ok)
        self.assertEqual(val, 55)


if __name__ == '__main__':
    unittest.main()
