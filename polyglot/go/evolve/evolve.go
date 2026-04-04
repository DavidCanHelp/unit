// Package evolve implements a simple GP engine using arithmetic expression trees.
package evolve

import (
	"fmt"
	"math/rand"
	"strings"
)

// Expr is an arithmetic expression tree node.
type Expr interface {
	Eval() int64
	Depth() int
	Size() int
	Clone() Expr
	String() string
}

// Num is a constant integer.
type Num struct{ Val int64 }

func (n *Num) Eval() int64    { return n.Val }
func (n *Num) Depth() int     { return 1 }
func (n *Num) Size() int      { return 1 }
func (n *Num) Clone() Expr    { return &Num{Val: n.Val} }
func (n *Num) String() string { return fmt.Sprintf("%d", n.Val) }

// BinOp is a binary operation.
type BinOp struct {
	Op          string
	Left, Right Expr
}

func (b *BinOp) Eval() int64 {
	l, r := b.Left.Eval(), b.Right.Eval()
	switch b.Op {
	case "+":
		return l + r
	case "-":
		return l - r
	case "*":
		return l * r
	case "mod":
		if r == 0 {
			return 0
		}
		return l % r
	}
	return 0
}

func (b *BinOp) Depth() int {
	ld, rd := b.Left.Depth(), b.Right.Depth()
	if ld > rd {
		return ld + 1
	}
	return rd + 1
}

func (b *BinOp) Size() int  { return 1 + b.Left.Size() + b.Right.Size() }
func (b *BinOp) Clone() Expr {
	return &BinOp{Op: b.Op, Left: b.Left.Clone(), Right: b.Right.Clone()}
}
func (b *BinOp) String() string {
	return fmt.Sprintf("(%s %s %s)", b.Op, b.Left.String(), b.Right.String())
}

var ops = []string{"+", "-", "*", "mod"}

// RandomExpr generates a random expression tree up to the given depth.
func RandomExpr(rng *rand.Rand, maxDepth int) Expr {
	if maxDepth <= 1 || rng.Intn(3) == 0 {
		return &Num{Val: int64(rng.Intn(101))}
	}
	return &BinOp{
		Op:    ops[rng.Intn(len(ops))],
		Left:  RandomExpr(rng, maxDepth-1),
		Right: RandomExpr(rng, maxDepth-1),
	}
}

// Mutate returns a mutated copy of the expression.
func Mutate(rng *rand.Rand, expr Expr) Expr {
	clone := expr.Clone()
	switch rng.Intn(3) {
	case 0: // Change a constant
		mutateNum(rng, clone)
	case 1: // Change an operator
		mutateOp(rng, clone)
	case 2: // Replace a subtree
		return replaceRandom(rng, clone, RandomExpr(rng, 2))
	}
	return clone
}

func mutateNum(rng *rand.Rand, expr Expr) {
	switch e := expr.(type) {
	case *Num:
		e.Val += int64(rng.Intn(21)) - 10
	case *BinOp:
		if rng.Intn(2) == 0 {
			mutateNum(rng, e.Left)
		} else {
			mutateNum(rng, e.Right)
		}
	}
}

func mutateOp(rng *rand.Rand, expr Expr) {
	if b, ok := expr.(*BinOp); ok {
		if rng.Intn(2) == 0 && b.Left.Depth() > 1 {
			mutateOp(rng, b.Left)
		} else if b.Right.Depth() > 1 {
			mutateOp(rng, b.Right)
		} else {
			b.Op = ops[rng.Intn(len(ops))]
		}
	}
}

func replaceRandom(rng *rand.Rand, expr Expr, replacement Expr) Expr {
	if rng.Intn(3) == 0 {
		return replacement
	}
	if b, ok := expr.(*BinOp); ok {
		if rng.Intn(2) == 0 {
			b.Left = replaceRandom(rng, b.Left, replacement)
		} else {
			b.Right = replaceRandom(rng, b.Right, replacement)
		}
	}
	return expr
}

// Crossover combines two expression trees.
func Crossover(rng *rand.Rand, a, b Expr) Expr {
	ac := a.Clone()
	bc := b.Clone()
	// Pick a random subtree from b and insert into a.
	donor := pickRandom(rng, bc)
	return replaceRandom(rng, ac, donor)
}

func pickRandom(rng *rand.Rand, expr Expr) Expr {
	if b, ok := expr.(*BinOp); ok && rng.Intn(3) > 0 {
		if rng.Intn(2) == 0 {
			return pickRandom(rng, b.Left)
		}
		return pickRandom(rng, b.Right)
	}
	return expr.Clone()
}

// Candidate is a GP individual.
type Candidate struct {
	Expr    Expr
	Fitness float64
}

// Score evaluates a candidate against a target output value.
func Score(expr Expr, targetVal int64) float64 {
	result := expr.Eval()
	if result == targetVal {
		return 1000.0 - float64(expr.Size())*10.0
	}
	diff := result - targetVal
	if diff < 0 {
		diff = -diff
	}
	if diff > 1000 {
		return 0.0
	}
	return 1.0 / (1.0 + float64(diff))
}

// RunEvolution runs the GP engine against a target value.
// Returns the best expression and whether it found an exact solution.
func RunEvolution(targetVal int64, maxGens int, seed int64) (Expr, bool) {
	rng := rand.New(rand.NewSource(seed))
	popSize := 30
	elites := 5

	// Initialize population.
	pop := make([]Candidate, popSize)
	for i := range pop {
		pop[i] = Candidate{Expr: RandomExpr(rng, 4)}
	}

	for gen := 0; gen < maxGens; gen++ {
		// Evaluate.
		for i := range pop {
			pop[i].Fitness = Score(pop[i].Expr, targetVal)
		}

		// Sort by fitness descending.
		sortPop(pop)

		// Check for winner.
		if pop[0].Fitness >= 900.0 {
			return pop[0].Expr, true
		}

		// Next generation.
		next := make([]Candidate, 0, popSize)
		for i := 0; i < elites && i < len(pop); i++ {
			next = append(next, Candidate{Expr: pop[i].Expr.Clone()})
		}
		for len(next) < popSize {
			parent := tournamentSelect(rng, pop)
			var child Expr
			if rng.Intn(5) == 0 {
				other := tournamentSelect(rng, pop)
				child = Crossover(rng, parent.Expr, other.Expr)
			} else {
				child = Mutate(rng, parent.Expr)
			}
			// Limit tree size.
			if child.Size() > 20 {
				child = RandomExpr(rng, 3)
			}
			next = append(next, Candidate{Expr: child})
		}
		pop = next
	}

	// Return best found.
	for i := range pop {
		pop[i].Fitness = Score(pop[i].Expr, targetVal)
	}
	sortPop(pop)
	return pop[0].Expr, pop[0].Fitness >= 900.0
}

func tournamentSelect(rng *rand.Rand, pop []Candidate) Candidate {
	best := pop[rng.Intn(len(pop))]
	for i := 0; i < 2; i++ {
		c := pop[rng.Intn(len(pop))]
		if c.Fitness > best.Fitness {
			best = c
		}
	}
	return best
}

func sortPop(pop []Candidate) {
	// Simple insertion sort (no imports needed).
	for i := 1; i < len(pop); i++ {
		j := i
		for j > 0 && pop[j].Fitness > pop[j-1].Fitness {
			pop[j], pop[j-1] = pop[j-1], pop[j]
			j--
		}
	}
}

// FormatProgram returns a serializable representation of an expression.
func FormatProgram(expr Expr) string {
	return expr.String()
}

// ParseTarget extracts a numeric target from a Forth-style output string like "55 ".
func ParseTarget(s string) (int64, bool) {
	s = strings.TrimSpace(s)
	n, err := fmt.Sscanf(s, "%d", new(int64))
	if n == 1 && err == nil {
		var val int64
		fmt.Sscanf(s, "%d", &val)
		return val, true
	}
	return 0, false
}
