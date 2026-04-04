package sexp

import "testing"

func TestParsePeerAnnounce(t *testing.T) {
	s, err := Parse(`(peer-announce :id "aabbccdd" :port 4201)`)
	if err != nil {
		t.Fatal(err)
	}
	if s.MsgType() != "peer-announce" {
		t.Fatalf("expected peer-announce, got %s", s.MsgType())
	}
	id := s.GetKeyword("id")
	if id == nil {
		t.Fatal("missing :id")
	}
	str, ok := id.AsStr()
	if !ok || str != "aabbccdd" {
		t.Fatalf("expected aabbccdd, got %v", id)
	}
	port := s.GetKeyword("port")
	if port == nil {
		t.Fatal("missing :port")
	}
	n, ok := port.AsInt()
	if !ok || n != 4201 {
		t.Fatalf("expected 4201, got %v", port)
	}
}

func TestParsePeerStatus(t *testing.T) {
	s, err := Parse(`(peer-status :id "aabb" :peers 2 :fitness 10 :energy 800)`)
	if err != nil {
		t.Fatal(err)
	}
	if s.MsgType() != "peer-status" {
		t.Fatalf("expected peer-status, got %s", s.MsgType())
	}
	f := s.GetKeyword("fitness")
	if f == nil {
		t.Fatal("missing :fitness")
	}
	n, _ := f.AsInt()
	if n != 10 {
		t.Fatalf("expected 10, got %d", n)
	}
}

func TestParseChallenge(t *testing.T) {
	input := `(challenge :id 42 :name "fib10" :desc "compute fib 10" :target "55 " :reward 100 :seeds ("0 1 10 0 DO OVER + SWAP LOOP DROP ." "0 ."))`
	s, err := Parse(input)
	if err != nil {
		t.Fatal(err)
	}
	if s.MsgType() != "challenge" {
		t.Fatalf("expected challenge, got %s", s.MsgType())
	}
	name := s.GetKeyword("name")
	if name == nil {
		t.Fatal("missing :name")
	}
	str, _ := name.AsStr()
	if str != "fib10" {
		t.Fatalf("expected fib10, got %s", str)
	}
	seeds := s.GetKeyword("seeds")
	if seeds == nil || seeds.Kind != KindList {
		t.Fatal("missing or invalid :seeds")
	}
	if len(seeds.Children) != 2 {
		t.Fatalf("expected 2 seeds, got %d", len(seeds.Children))
	}
}

func TestParseSolution(t *testing.T) {
	s, err := Parse(`(solution :challenge-id 42 :program "0 1 10 0 DO OVER + SWAP LOOP DROP ." :solver "aabb")`)
	if err != nil {
		t.Fatal(err)
	}
	if s.MsgType() != "solution" {
		t.Fatalf("expected solution, got %s", s.MsgType())
	}
	prog := s.GetKeyword("program")
	if prog == nil {
		t.Fatal("missing :program")
	}
}

func TestRoundTrip(t *testing.T) {
	input := `(peer-status :id "aabb" :peers 2 :fitness 10)`
	s, err := Parse(input)
	if err != nil {
		t.Fatal(err)
	}
	output := Format(s)
	s2, err := Parse(output)
	if err != nil {
		t.Fatalf("re-parse failed: %v", err)
	}
	if s2.MsgType() != "peer-status" {
		t.Fatalf("round-trip lost msg type")
	}
}

func TestEmptyInput(t *testing.T) {
	_, err := Parse("")
	if err == nil {
		t.Fatal("expected error for empty input")
	}
}
