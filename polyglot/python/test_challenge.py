import unittest
import sexp
import challenge


class TestChallenge(unittest.TestCase):
    def test_on_challenge(self):
        store = challenge.ChallengeStore()
        parsed = sexp.parse('(challenge :id 42 :name "fib10" :target "55 " :reward 100 :seeds ("a"))')
        store.on_challenge(parsed)
        ch = store.get(42)
        self.assertIsNotNone(ch)
        self.assertEqual(ch.name, 'fib10')
        self.assertEqual(ch.reward, 100)

    def test_on_solution(self):
        store = challenge.ChallengeStore()
        store.add(challenge.Challenge(42, 'test', '55 '))
        parsed = sexp.parse('(solution :challenge-id 42 :program "(+ 5 50)" :solver "aa")')
        store.on_solution(parsed)
        ch = store.get(42)
        self.assertTrue(ch.solved)

    def test_get_unsolved(self):
        store = challenge.ChallengeStore()
        store.add(challenge.Challenge(1, 'low', '10 ', 50))
        store.add(challenge.Challenge(2, 'high', '99 ', 200))
        store.add(challenge.Challenge(3, 'done', '1 ', 300))
        store.get(3).solved = True
        unsolved = store.get_unsolved()
        self.assertEqual(len(unsolved), 2)
        self.assertEqual(unsolved[0].name, 'high')

    def test_count(self):
        store = challenge.ChallengeStore()
        store.add(challenge.Challenge(1, 'a', '10 '))
        store.add(challenge.Challenge(2, 'b', '20 '))
        store.get(2).solved = True
        total, solved = store.count()
        self.assertEqual(total, 2)
        self.assertEqual(solved, 1)

    def test_format_solution(self):
        s = challenge.format_solution(42, '(+ 5 50)', 'aabb')
        parsed = sexp.parse(s)
        self.assertEqual(sexp.msg_type(parsed), 'solution')

    def test_try_evolve(self):
        ch = challenge.Challenge(1, 'test', '55 ', 100)
        program, found = challenge.try_evolve_solution(ch, seed=42)
        self.assertIsInstance(program, str)
        self.assertTrue(len(program) > 0)


if __name__ == '__main__':
    unittest.main()
