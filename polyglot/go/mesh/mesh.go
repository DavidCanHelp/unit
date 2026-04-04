// Package mesh implements UDP mesh networking for the unit protocol.
package mesh

import (
	"crypto/rand"
	"fmt"
	"log"
	"net"
	"sync"

	"github.com/DavidCanHelp/unit/polyglot/go/sexp"
)

// Peer represents a known mesh peer.
type Peer struct {
	ID      string
	Addr    *net.UDPAddr
	Fitness int64
	Energy  int64
}

// Mesh manages UDP mesh communication.
type Mesh struct {
	ID    string
	Port  int
	conn  *net.UDPConn
	mu    sync.Mutex
	peers map[string]*Peer // keyed by ID
	OnMsg func(sexp.SExp, *net.UDPAddr)
}

// NewMesh creates a mesh node. If id is empty, generates a random one.
func NewMesh(id string, port int) (*Mesh, error) {
	if id == "" {
		b := make([]byte, 4)
		rand.Read(b)
		id = fmt.Sprintf("%02x%02x%02x%02x", b[0], b[1], b[2], b[3])
	}

	addr, err := net.ResolveUDPAddr("udp", fmt.Sprintf("0.0.0.0:%d", port))
	if err != nil {
		return nil, err
	}
	conn, err := net.ListenUDP("udp", addr)
	if err != nil {
		return nil, err
	}

	return &Mesh{
		ID:    id,
		Port:  port,
		conn:  conn,
		peers: make(map[string]*Peer),
	}, nil
}

// Listen starts receiving UDP messages in a goroutine.
func (m *Mesh) Listen() {
	go func() {
		buf := make([]byte, 65536)
		for {
			n, addr, err := m.conn.ReadFromUDP(buf)
			if err != nil {
				continue
			}
			msg := string(buf[:n])
			parsed, err := sexp.Parse(msg)
			if err != nil {
				continue
			}
			m.handleMessage(parsed, addr)
			if m.OnMsg != nil {
				m.OnMsg(parsed, addr)
			}
		}
	}()
}

func (m *Mesh) handleMessage(s sexp.SExp, addr *net.UDPAddr) {
	switch s.MsgType() {
	case "peer-announce", "peer-status":
		idNode := s.GetKeyword("id")
		if idNode == nil {
			return
		}
		peerID, ok := idNode.AsStr()
		if !ok {
			return
		}
		if peerID == m.ID {
			return // ignore self
		}
		m.mu.Lock()
		p, exists := m.peers[peerID]
		if !exists {
			p = &Peer{ID: peerID, Addr: addr}
			m.peers[peerID] = p
			log.Printf("[mesh] discovered peer %s @ %s", peerID, addr)
		}
		p.Addr = addr
		if f := s.GetKeyword("fitness"); f != nil {
			p.Fitness, _ = f.AsInt()
		}
		if e := s.GetKeyword("energy"); e != nil {
			p.Energy, _ = e.AsInt()
		}
		m.mu.Unlock()
	}
}

// Send sends a raw S-expression string to a specific address.
func (m *Mesh) Send(addr *net.UDPAddr, msg string) {
	m.conn.WriteToUDP([]byte(msg), addr)
}

// SendToAll broadcasts an S-expression string to all known peers.
func (m *Mesh) SendToAll(msg string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for _, p := range m.peers {
		m.conn.WriteToUDP([]byte(msg), p.Addr)
	}
}

// Announce sends a peer-announce to a specific address.
func (m *Mesh) Announce(peerAddr string) error {
	addr, err := net.ResolveUDPAddr("udp", peerAddr)
	if err != nil {
		return err
	}
	msg := fmt.Sprintf(`(peer-announce :id "%s" :port %d)`, m.ID, m.Port)
	m.Send(addr, msg)
	return nil
}

// GossipTick broadcasts peer-status to all known peers.
func (m *Mesh) GossipTick(fitness, energy int64) {
	m.mu.Lock()
	peerCount := len(m.peers)
	m.mu.Unlock()
	msg := fmt.Sprintf(
		`(peer-status :id "%s" :peers %d :fitness %d :energy %d)`,
		m.ID, peerCount, fitness, energy,
	)
	m.SendToAll(msg)
}

// PeerCount returns the number of known peers.
func (m *Mesh) PeerCount() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.peers)
}

// Close shuts down the mesh.
func (m *Mesh) Close() {
	m.conn.Close()
}
