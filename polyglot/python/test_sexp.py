import unittest
import sexp


class TestSexp(unittest.TestCase):
    def test_parse_peer_announce(self):
        p = sexp.parse('(peer-announce :id "aabb" :port 4201)')
        self.assertEqual(sexp.msg_type(p), 'peer-announce')
        self.assertEqual(sexp.get_keyword(p, 'id'), 'aabb')
        self.assertEqual(sexp.get_keyword(p, 'port'), 4201)

    def test_parse_challenge(self):
        p = sexp.parse('(challenge :id 42 :name "fib10" :target "55 " :reward 100 :seeds ("a" "b"))')
        self.assertEqual(sexp.msg_type(p), 'challenge')
        self.assertEqual(sexp.get_keyword(p, 'name'), 'fib10')
        seeds = sexp.get_keyword(p, 'seeds')
        self.assertEqual(len(seeds), 2)

    def test_parse_solution(self):
        p = sexp.parse('(solution :challenge-id 42 :program "(+ 5 50)" :solver "aa")')
        self.assertEqual(sexp.msg_type(p), 'solution')
        self.assertEqual(sexp.get_keyword(p, 'challenge-id'), 42)

    def test_roundtrip(self):
        p = sexp.parse('(peer-status :id "ab" :peers 2)')
        s = sexp.format_sexp(p)
        p2 = sexp.parse(s)
        self.assertEqual(sexp.msg_type(p2), 'peer-status')

    def test_empty(self):
        with self.assertRaises(ValueError):
            sexp.parse('')


if __name__ == '__main__':
    unittest.main()
