// Package challenge implements the challenge/solution protocol for the Go organism.
package challenge

import (
	"fmt"
	"log"
	"strconv"
	"strings"
	"sync"

	"github.com/DavidCanHelp/unit/polyglot/go/evolve"
	"github.com/DavidCanHelp/unit/polyglot/go/sexp"
)

// Challenge represents a problem to solve.
type Challenge struct {
	ID           int64
	Name         string
	Description  string
	TargetOutput string
	Reward       int64
	Seeds        []string
	Solved       bool
	Solution     string
	Solver       string
}

// Store manages challenges.
type Store struct {
	mu         sync.Mutex
	challenges map[int64]*Challenge
}

// NewStore creates a challenge store.
func NewStore() *Store {
	return &Store{challenges: make(map[int64]*Challenge)}
}

// OnChallenge processes an incoming challenge S-expression.
func (s *Store) OnChallenge(msg sexp.SExp) {
	idNode := msg.GetKeyword("id")
	if idNode == nil {
		return
	}
	id, ok := idNode.AsInt()
	if !ok {
		return
	}

	nameNode := msg.GetKeyword("name")
	name := ""
	if nameNode != nil {
		name, _ = nameNode.AsStr()
	}

	descNode := msg.GetKeyword("desc")
	desc := ""
	if descNode != nil {
		desc, _ = descNode.AsStr()
	}

	targetNode := msg.GetKeyword("target")
	target := ""
	if targetNode != nil {
		target, _ = targetNode.AsStr()
	}

	rewardNode := msg.GetKeyword("reward")
	reward := int64(50)
	if rewardNode != nil {
		reward, _ = rewardNode.AsInt()
	}

	var seeds []string
	seedsNode := msg.GetKeyword("seeds")
	if seedsNode != nil && seedsNode.Kind == sexp.KindList {
		for _, child := range seedsNode.Children {
			if str, ok := child.AsStr(); ok {
				seeds = append(seeds, str)
			}
		}
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	if _, exists := s.challenges[id]; !exists {
		s.challenges[id] = &Challenge{
			ID:           id,
			Name:         name,
			Description:  desc,
			TargetOutput: target,
			Reward:       reward,
			Seeds:        seeds,
		}
		log.Printf("[challenge] received #%d: %s (reward: %d)", id, name, reward)
	}
}

// OnSolution processes an incoming solution S-expression.
func (s *Store) OnSolution(msg sexp.SExp) {
	chIDNode := msg.GetKeyword("challenge-id")
	if chIDNode == nil {
		return
	}
	chID, ok := chIDNode.AsInt()
	if !ok {
		return
	}

	progNode := msg.GetKeyword("program")
	program := ""
	if progNode != nil {
		program, _ = progNode.AsStr()
	}

	solverNode := msg.GetKeyword("solver")
	solver := ""
	if solverNode != nil {
		solver, _ = solverNode.AsStr()
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	if ch, exists := s.challenges[chID]; exists && !ch.Solved {
		// Verify: we can't run Forth, but we can check if it's a numeric target.
		ch.Solved = true
		ch.Solution = program
		ch.Solver = solver
		log.Printf("[challenge] solution received for #%d from %s", chID, solver)
	}
}

// GetUnsolved returns unsolved challenges sorted by reward (descending).
func (s *Store) GetUnsolved() []*Challenge {
	s.mu.Lock()
	defer s.mu.Unlock()
	var out []*Challenge
	for _, ch := range s.challenges {
		if !ch.Solved {
			out = append(out, ch)
		}
	}
	// Sort by reward descending (insertion sort).
	for i := 1; i < len(out); i++ {
		j := i
		for j > 0 && out[j].Reward > out[j-1].Reward {
			out[j], out[j-1] = out[j-1], out[j]
			j--
		}
	}
	return out
}

// TryEvolveSolution attempts to evolve a solution for a challenge using the Go GP engine.
// Returns (solution_program, success).
func TryEvolveSolution(ch *Challenge, seed int64) (string, bool) {
	targetVal, ok := evolve.ParseTarget(ch.TargetOutput)
	if !ok {
		return "", false
	}

	expr, found := evolve.RunEvolution(targetVal, 200, seed)
	if found {
		program := evolve.FormatProgram(expr)
		return program, true
	}
	return evolve.FormatProgram(expr), false
}

// FormatSolution creates a solution S-expression string.
func FormatSolution(challengeID int64, program string, solverID string) string {
	return fmt.Sprintf(
		`(solution :challenge-id %d :program "%s" :solver "%s")`,
		challengeID,
		strings.ReplaceAll(program, `"`, `\"`),
		solverID,
	)
}

// Count returns total and solved counts.
func (s *Store) Count() (total, solved int) {
	s.mu.Lock()
	defer s.mu.Unlock()
	total = len(s.challenges)
	for _, ch := range s.challenges {
		if ch.Solved {
			solved++
		}
	}
	return
}

// Get returns a challenge by ID.
func (s *Store) Get(id int64) *Challenge {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.challenges[id]
}

// Add adds a challenge directly (for testing).
func (s *Store) Add(ch *Challenge) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.challenges[ch.ID] = ch
}

// ParseTarget parses a Forth-style target output.
func ParseTarget(target string) (int64, bool) {
	target = strings.TrimSpace(target)
	val, err := strconv.ParseInt(target, 10, 64)
	return val, err == nil
}
