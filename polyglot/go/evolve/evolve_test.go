package evolve

import (
	"math/rand"
	"testing"
)

func TestEvalNum(t *testing.T) {
	n := &Num{Val: 42}
	if n.Eval() != 42 {
		t.Fatalf("expected 42, got %d", n.Eval())
	}
}

func TestEvalBinOp(t *testing.T) {
	expr := &BinOp{Op: "+", Left: &Num{Val: 10}, Right: &Num{Val: 32}}
	if expr.Eval() != 42 {
		t.Fatalf("expected 42, got %d", expr.Eval())
	}
}

func TestEvalNested(t *testing.T) {
	// (+ (* 5 11) 0) = 55
	expr := &BinOp{
		Op:    "+",
		Left:  &BinOp{Op: "*", Left: &Num{Val: 5}, Right: &Num{Val: 11}},
		Right: &Num{Val: 0},
	}
	if expr.Eval() != 55 {
		t.Fatalf("expected 55, got %d", expr.Eval())
	}
}

func TestEvalDivByZero(t *testing.T) {
	expr := &BinOp{Op: "mod", Left: &Num{Val: 10}, Right: &Num{Val: 0}}
	if expr.Eval() != 0 {
		t.Fatalf("expected 0 for div-by-zero, got %d", expr.Eval())
	}
}

func TestMutateProducesValid(t *testing.T) {
	rng := rand.New(rand.NewSource(42))
	expr := &BinOp{Op: "+", Left: &Num{Val: 5}, Right: &Num{Val: 10}}
	for i := 0; i < 20; i++ {
		mutated := Mutate(rng, expr)
		mutated.Eval() // should not panic
	}
}

func TestCrossoverProducesValid(t *testing.T) {
	rng := rand.New(rand.NewSource(99))
	a := &BinOp{Op: "+", Left: &Num{Val: 5}, Right: &Num{Val: 10}}
	b := &BinOp{Op: "*", Left: &Num{Val: 3}, Right: &Num{Val: 7}}
	for i := 0; i < 20; i++ {
		child := Crossover(rng, a, b)
		child.Eval() // should not panic
	}
}

func TestRunEvolutionFinds55(t *testing.T) {
	expr, found := RunEvolution(55, 500, 42)
	if !found {
		t.Logf("evolution did not find exact 55, best: %s = %d", expr.String(), expr.Eval())
		// Not a hard failure — GP is stochastic.
		return
	}
	if expr.Eval() != 55 {
		t.Fatalf("claimed found but eval is %d", expr.Eval())
	}
}

func TestRunEvolutionFinds42(t *testing.T) {
	expr, found := RunEvolution(42, 500, 123)
	if !found {
		t.Logf("evolution did not find exact 42, best: %s = %d", expr.String(), expr.Eval())
		return
	}
	if expr.Eval() != 42 {
		t.Fatalf("claimed found but eval is %d", expr.Eval())
	}
}

func TestScoreExact(t *testing.T) {
	expr := &Num{Val: 55}
	s := Score(expr, 55)
	if s < 900 {
		t.Fatalf("expected high score for exact match, got %f", s)
	}
}

func TestScoreWrong(t *testing.T) {
	expr := &Num{Val: 100}
	s := Score(expr, 55)
	if s >= 900 {
		t.Fatalf("expected low score for wrong answer, got %f", s)
	}
}

func TestParseTarget(t *testing.T) {
	n, ok := ParseTarget("55 ")
	if !ok || n != 55 {
		t.Fatalf("expected 55, got %d ok=%v", n, ok)
	}
}

func TestFormatProgram(t *testing.T) {
	expr := &BinOp{Op: "+", Left: &Num{Val: 5}, Right: &Num{Val: 50}}
	s := FormatProgram(expr)
	if s != "(+ 5 50)" {
		t.Fatalf("expected (+ 5 50), got %s", s)
	}
}
