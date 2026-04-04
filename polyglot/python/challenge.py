"""Challenge/solution protocol for the Python organism."""

import logging
import threading

import sexp
import evolve

log = logging.getLogger(__name__)


class Challenge:
    def __init__(self, id, name, target_output, reward=50, seeds=None):
        self.id = id
        self.name = name
        self.target_output = target_output
        self.reward = reward
        self.seeds = seeds or []
        self.solved = False
        self.solution = ''
        self.solver = ''


class ChallengeStore:
    def __init__(self):
        self._challenges = {}
        self._lock = threading.Lock()

    def on_challenge(self, parsed):
        id = sexp.get_keyword(parsed, 'id')
        if not isinstance(id, int):
            return
        name = sexp.get_keyword(parsed, 'name') or ''
        target = sexp.get_keyword(parsed, 'target') or ''
        reward = sexp.get_keyword(parsed, 'reward') or 50
        seeds = sexp.get_keyword(parsed, 'seeds') or []
        if isinstance(seeds, list):
            seeds = [s for s in seeds if isinstance(s, str)]

        with self._lock:
            if id not in self._challenges:
                self._challenges[id] = Challenge(id, name, target, reward, seeds)
                log.info(f"received challenge #{id}: {name} (reward: {reward})")

    def on_solution(self, parsed):
        ch_id = sexp.get_keyword(parsed, 'challenge-id')
        if not isinstance(ch_id, int):
            return
        program = sexp.get_keyword(parsed, 'program') or ''
        solver = sexp.get_keyword(parsed, 'solver') or ''

        with self._lock:
            ch = self._challenges.get(ch_id)
            if ch and not ch.solved:
                ch.solved = True
                ch.solution = program
                ch.solver = solver
                log.info(f"solution received for #{ch_id} from {solver}")

    def get_unsolved(self):
        with self._lock:
            out = [c for c in self._challenges.values() if not c.solved]
            out.sort(key=lambda c: c.reward, reverse=True)
            return out

    def get(self, id):
        with self._lock:
            return self._challenges.get(id)

    def add(self, ch):
        with self._lock:
            self._challenges[ch.id] = ch

    def count(self):
        with self._lock:
            total = len(self._challenges)
            solved = sum(1 for c in self._challenges.values() if c.solved)
            return total, solved


def try_evolve_solution(ch, seed=42):
    """Attempt to evolve a solution using the Python GP engine."""
    target_val, ok = evolve.parse_target(ch.target_output)
    if not ok:
        return '', False
    expr, found = evolve.run_evolution(target_val, max_gens=200, seed=seed)
    return evolve.format_program(expr), found


def format_solution(challenge_id, program, solver_id):
    return f'(solution :challenge-id {challenge_id} :program "{program}" :solver "{solver_id}")'
