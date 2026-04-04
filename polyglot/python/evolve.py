"""GP engine using Python AST nodes for symbolic regression."""

import ast
import random
import operator

OPS = {
    ast.Add: operator.add,
    ast.Sub: operator.sub,
    ast.Mult: operator.mul,
    ast.Mod: lambda a, b: a % b if b != 0 else 0,
}
OP_NODES = [ast.Add, ast.Sub, ast.Mult, ast.Mod]


def random_expr(rng, max_depth=4):
    """Generate a random AST expression tree."""
    if max_depth <= 1 or rng.random() < 0.3:
        return ast.Constant(value=rng.randint(0, 100))
    op = rng.choice(OP_NODES)()
    return ast.BinOp(
        left=random_expr(rng, max_depth - 1),
        op=op,
        right=random_expr(rng, max_depth - 1),
    )


def eval_expr(node):
    """Safely evaluate an AST expression tree."""
    if isinstance(node, ast.Constant):
        return node.value
    if isinstance(node, ast.BinOp):
        left = eval_expr(node.left)
        right = eval_expr(node.right)
        fn = OPS.get(type(node.op))
        if fn is None:
            return 0
        try:
            result = fn(left, right)
            if abs(result) > 10**9:
                return 0
            return result
        except Exception:
            return 0
    return 0


def tree_size(node):
    """Count nodes in the tree."""
    if isinstance(node, ast.Constant):
        return 1
    if isinstance(node, ast.BinOp):
        return 1 + tree_size(node.left) + tree_size(node.right)
    return 1


def mutate(rng, node):
    """Return a mutated copy of the expression tree."""
    node = _deep_copy(node)
    choice = rng.randint(0, 2)
    if choice == 0:
        _mutate_constant(rng, node)
    elif choice == 1:
        _mutate_op(rng, node)
    else:
        return _replace_random(rng, node, random_expr(rng, 2))
    return node


def _mutate_constant(rng, node):
    if isinstance(node, ast.Constant):
        node.value += rng.randint(-10, 10)
    elif isinstance(node, ast.BinOp):
        if rng.random() < 0.5:
            _mutate_constant(rng, node.left)
        else:
            _mutate_constant(rng, node.right)


def _mutate_op(rng, node):
    if isinstance(node, ast.BinOp):
        if rng.random() < 0.5 and isinstance(node.left, ast.BinOp):
            _mutate_op(rng, node.left)
        elif isinstance(node.right, ast.BinOp):
            _mutate_op(rng, node.right)
        else:
            node.op = rng.choice(OP_NODES)()


def _replace_random(rng, node, replacement):
    if rng.random() < 0.3:
        return replacement
    if isinstance(node, ast.BinOp):
        if rng.random() < 0.5:
            node.left = _replace_random(rng, node.left, replacement)
        else:
            node.right = _replace_random(rng, node.right, replacement)
    return node


def crossover(rng, a, b):
    """Combine two expression trees."""
    a = _deep_copy(a)
    b = _deep_copy(b)
    donor = _pick_random(rng, b)
    return _replace_random(rng, a, donor)


def _pick_random(rng, node):
    if isinstance(node, ast.BinOp) and rng.random() > 0.3:
        if rng.random() < 0.5:
            return _pick_random(rng, node.left)
        return _pick_random(rng, node.right)
    return _deep_copy(node)


def _deep_copy(node):
    if isinstance(node, ast.Constant):
        return ast.Constant(value=node.value)
    if isinstance(node, ast.BinOp):
        return ast.BinOp(
            left=_deep_copy(node.left),
            op=type(node.op)(),
            right=_deep_copy(node.right),
        )
    return node


def score(expr, target_val):
    """Score a candidate against a target value."""
    result = eval_expr(expr)
    if result == target_val:
        return 1000.0 - tree_size(expr) * 10.0
    diff = abs(result - target_val)
    if diff > 1000:
        return 0.0
    return 1.0 / (1.0 + diff)


def format_program(expr):
    """Serialize expression to a string."""
    if isinstance(expr, ast.Constant):
        return str(expr.value)
    if isinstance(expr, ast.BinOp):
        op_str = {ast.Add: '+', ast.Sub: '-', ast.Mult: '*', ast.Mod: '%'}.get(type(expr.op), '?')
        return f'({op_str} {format_program(expr.left)} {format_program(expr.right)})'
    return '0'


def run_evolution(target_val, max_gens=200, seed=42):
    """Run GP evolution. Returns (best_expr, found_exact)."""
    rng = random.Random(seed)
    pop_size, elites = 30, 5
    pop = [{'expr': random_expr(rng, 4), 'fitness': 0.0} for _ in range(pop_size)]

    for _ in range(max_gens):
        for c in pop:
            c['fitness'] = score(c['expr'], target_val)
        pop.sort(key=lambda c: c['fitness'], reverse=True)
        if pop[0]['fitness'] >= 900.0:
            return pop[0]['expr'], True

        nxt = [{'expr': _deep_copy(c['expr']), 'fitness': 0.0} for c in pop[:elites]]
        while len(nxt) < pop_size:
            parent = _tournament(rng, pop)
            if rng.random() < 0.2:
                other = _tournament(rng, pop)
                child = crossover(rng, parent['expr'], other['expr'])
            else:
                child = mutate(rng, parent['expr'])
            if tree_size(child) > 20:
                child = random_expr(rng, 3)
            nxt.append({'expr': child, 'fitness': 0.0})
        pop = nxt

    for c in pop:
        c['fitness'] = score(c['expr'], target_val)
    pop.sort(key=lambda c: c['fitness'], reverse=True)
    return pop[0]['expr'], pop[0]['fitness'] >= 900.0


def _tournament(rng, pop):
    best = rng.choice(pop)
    for _ in range(2):
        c = rng.choice(pop)
        if c['fitness'] > best['fitness']:
            best = c
    return best


def parse_target(s):
    """Parse a Forth-style target output like '55 '."""
    try:
        return int(s.strip()), True
    except ValueError:
        return 0, False
