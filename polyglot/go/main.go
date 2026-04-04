// unit-go: a Go organism that joins the Rust unit mesh.
//
// Speaks S-expressions over UDP. Evolves arithmetic expression trees
// to solve challenges broadcast by Rust units. Different language,
// different mutation strategy, same mesh protocol.
package main

import (
	"flag"
	"fmt"
	"log"
	"net"
	"time"

	"github.com/DavidCanHelp/unit/polyglot/go/challenge"
	"github.com/DavidCanHelp/unit/polyglot/go/mesh"
	"github.com/DavidCanHelp/unit/polyglot/go/sexp"
)

func main() {
	port := flag.Int("port", 4201, "UDP port to listen on")
	peer := flag.String("peer", "", "seed peer address (e.g. 127.0.0.1:4200)")
	id := flag.String("id", "", "8-char hex node ID (random if omitted)")
	flag.Parse()

	m, err := mesh.NewMesh(*id, *port)
	if err != nil {
		log.Fatalf("mesh: %v", err)
	}
	defer m.Close()

	store := challenge.NewStore()
	fitness := int64(0)
	energy := int64(1000)
	evolveCount := int64(0)

	// Handle incoming messages.
	m.OnMsg = func(s sexp.SExp, addr *net.UDPAddr) {
		switch s.MsgType() {
		case "challenge":
			store.OnChallenge(s)
		case "solution":
			store.OnSolution(s)
		}
	}

	m.Listen()

	fmt.Printf("unit-go v0.1.0 | node %s | port %d\n", m.ID, m.Port)

	// Announce to seed peer.
	if *peer != "" {
		if err := m.Announce(*peer); err != nil {
			log.Printf("announce: %v", err)
		} else {
			fmt.Printf("announced to %s\n", *peer)
		}
	}

	// Main loop.
	gossipTicker := time.NewTicker(3 * time.Second)
	evolveTicker := time.NewTicker(10 * time.Second)
	statusTicker := time.NewTicker(30 * time.Second)
	defer gossipTicker.Stop()
	defer evolveTicker.Stop()
	defer statusTicker.Stop()

	for {
		select {
		case <-gossipTicker.C:
			m.GossipTick(fitness, energy)
			// Re-announce to seed peer to maintain connection.
			if *peer != "" {
				m.Announce(*peer)
			}

		case <-evolveTicker.C:
			unsolved := store.GetUnsolved()
			if len(unsolved) == 0 {
				continue
			}
			ch := unsolved[0]
			evolveCount++
			log.Printf("[evolve] attempting challenge #%d: %s (target: %s)",
				ch.ID, ch.Name, ch.TargetOutput)
			program, found := challenge.TryEvolveSolution(ch, evolveCount*7+1)
			if found {
				log.Printf("[evolve] SOLVED #%d: %s", ch.ID, program)
				fitness += ch.Reward
				energy += 100
				// Broadcast solution.
				sol := challenge.FormatSolution(ch.ID, program, m.ID)
				m.SendToAll(sol)
				// Mark locally.
				ch.Solved = true
				ch.Solution = program
				ch.Solver = m.ID
			} else {
				log.Printf("[evolve] no solution yet for #%d (best: %s)",
					ch.ID, program)
				energy -= 5 // evolution cost
			}

		case <-statusTicker.C:
			total, solved := store.Count()
			log.Printf("[status] peers=%d challenges=%d/%d fitness=%d energy=%d",
				m.PeerCount(), solved, total, fitness, energy)
		}
	}
}
