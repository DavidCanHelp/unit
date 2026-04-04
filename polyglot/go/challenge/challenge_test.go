package challenge

import (
	"testing"

	"github.com/DavidCanHelp/unit/polyglot/go/sexp"
)

func TestOnChallenge(t *testing.T) {
	store := NewStore()
	msg, _ := sexp.Parse(`(challenge :id 42 :name "fib10" :desc "compute fib 10" :target "55 " :reward 100 :seeds ("seed1" "seed2"))`)
	store.OnChallenge(msg)

	ch := store.Get(42)
	if ch == nil {
		t.Fatal("challenge not stored")
	}
	if ch.Name != "fib10" {
		t.Fatalf("expected fib10, got %s", ch.Name)
	}
	if ch.Reward != 100 {
		t.Fatalf("expected reward 100, got %d", ch.Reward)
	}
	if len(ch.Seeds) != 2 {
		t.Fatalf("expected 2 seeds, got %d", len(ch.Seeds))
	}
}

func TestOnSolution(t *testing.T) {
	store := NewStore()
	store.Add(&Challenge{ID: 42, Name: "test", TargetOutput: "55 "})

	msg, _ := sexp.Parse(`(solution :challenge-id 42 :program "(+ 5 50)" :solver "aabb")`)
	store.OnSolution(msg)

	ch := store.Get(42)
	if !ch.Solved {
		t.Fatal("challenge should be solved")
	}
	if ch.Solution != "(+ 5 50)" {
		t.Fatalf("expected (+ 5 50), got %s", ch.Solution)
	}
}

func TestGetUnsolved(t *testing.T) {
	store := NewStore()
	store.Add(&Challenge{ID: 1, Name: "low", Reward: 50, TargetOutput: "10 "})
	store.Add(&Challenge{ID: 2, Name: "high", Reward: 200, TargetOutput: "99 "})
	store.Add(&Challenge{ID: 3, Name: "done", Reward: 300, Solved: true})

	unsolved := store.GetUnsolved()
	if len(unsolved) != 2 {
		t.Fatalf("expected 2 unsolved, got %d", len(unsolved))
	}
	if unsolved[0].Name != "high" {
		t.Fatalf("expected highest reward first, got %s", unsolved[0].Name)
	}
}

func TestTryEvolveSolution(t *testing.T) {
	ch := &Challenge{ID: 1, Name: "test", TargetOutput: "55 ", Reward: 100}
	program, found := TryEvolveSolution(ch, 42)
	if found {
		t.Logf("found solution: %s", program)
	} else {
		t.Logf("did not find exact solution, best: %s", program)
	}
	// Not a hard failure — GP is stochastic.
}

func TestCount(t *testing.T) {
	store := NewStore()
	store.Add(&Challenge{ID: 1, Name: "a"})
	store.Add(&Challenge{ID: 2, Name: "b", Solved: true})
	total, solved := store.Count()
	if total != 2 || solved != 1 {
		t.Fatalf("expected 2/1, got %d/%d", total, solved)
	}
}

func TestFormatSolution(t *testing.T) {
	s := FormatSolution(42, "(+ 5 50)", "aabb")
	if s == "" {
		t.Fatal("empty output")
	}
	parsed, err := sexp.Parse(s)
	if err != nil {
		t.Fatalf("not valid sexp: %v", err)
	}
	if parsed.MsgType() != "solution" {
		t.Fatalf("expected solution, got %s", parsed.MsgType())
	}
}
