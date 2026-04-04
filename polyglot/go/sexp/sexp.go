// Package sexp implements a minimal S-expression parser for the unit mesh protocol.
package sexp

import (
	"fmt"
	"strconv"
	"strings"
)

// SExp represents an S-expression node.
type SExp struct {
	Kind     Kind
	Atom     string
	Str      string
	Int      int64
	Children []SExp
}

type Kind int

const (
	KindAtom Kind = iota
	KindStr
	KindInt
	KindList
)

// Parse parses an S-expression string into a tree.
func Parse(input string) (SExp, error) {
	input = strings.TrimSpace(input)
	if len(input) == 0 {
		return SExp{}, fmt.Errorf("empty input")
	}
	node, _, err := parseExpr(input, 0)
	return node, err
}

func parseExpr(input string, pos int) (SExp, int, error) {
	pos = skipWhitespace(input, pos)
	if pos >= len(input) {
		return SExp{}, pos, fmt.Errorf("unexpected end of input")
	}

	switch input[pos] {
	case '(':
		return parseList(input, pos)
	case '"':
		return parseString(input, pos)
	default:
		return parseAtomOrInt(input, pos)
	}
}

func parseList(input string, pos int) (SExp, int, error) {
	pos++ // skip '('
	var children []SExp
	for {
		pos = skipWhitespace(input, pos)
		if pos >= len(input) {
			return SExp{}, pos, fmt.Errorf("unterminated list")
		}
		if input[pos] == ')' {
			pos++
			return SExp{Kind: KindList, Children: children}, pos, nil
		}
		child, newPos, err := parseExpr(input, pos)
		if err != nil {
			return SExp{}, newPos, err
		}
		children = append(children, child)
		pos = newPos
	}
}

func parseString(input string, pos int) (SExp, int, error) {
	pos++ // skip opening quote
	var sb strings.Builder
	for pos < len(input) {
		if input[pos] == '\\' && pos+1 < len(input) {
			sb.WriteByte(input[pos+1])
			pos += 2
			continue
		}
		if input[pos] == '"' {
			pos++
			return SExp{Kind: KindStr, Str: sb.String()}, pos, nil
		}
		sb.WriteByte(input[pos])
		pos++
	}
	return SExp{}, pos, fmt.Errorf("unterminated string")
}

func parseAtomOrInt(input string, pos int) (SExp, int, error) {
	start := pos
	for pos < len(input) && input[pos] != ' ' && input[pos] != '\t' &&
		input[pos] != ')' && input[pos] != '(' && input[pos] != '"' {
		pos++
	}
	token := input[start:pos]
	if n, err := strconv.ParseInt(token, 10, 64); err == nil {
		return SExp{Kind: KindInt, Int: n}, pos, nil
	}
	return SExp{Kind: KindAtom, Atom: token}, pos, nil
}

func skipWhitespace(input string, pos int) int {
	for pos < len(input) && (input[pos] == ' ' || input[pos] == '\t' ||
		input[pos] == '\n' || input[pos] == '\r') {
		pos++
	}
	return pos
}

// GetKeyword extracts the value following a :keyword in a list.
// e.g. for (foo :bar 42), GetKeyword("bar") returns the SExp for 42.
func (s *SExp) GetKeyword(name string) *SExp {
	if s.Kind != KindList {
		return nil
	}
	key := ":" + name
	for i, child := range s.Children {
		if child.Kind == KindAtom && child.Atom == key && i+1 < len(s.Children) {
			return &s.Children[i+1]
		}
	}
	return nil
}

// MsgType returns the first atom of a list S-expression (the message type).
func (s *SExp) MsgType() string {
	if s.Kind == KindList && len(s.Children) > 0 && s.Children[0].Kind == KindAtom {
		return s.Children[0].Atom
	}
	return ""
}

// AsInt returns the integer value if this is an int node.
func (s *SExp) AsInt() (int64, bool) {
	if s.Kind == KindInt {
		return s.Int, true
	}
	return 0, false
}

// AsStr returns the string value if this is a string node.
func (s *SExp) AsStr() (string, bool) {
	if s.Kind == KindStr {
		return s.Str, true
	}
	return "", false
}

// Format serializes an SExp back to a string.
func Format(s SExp) string {
	switch s.Kind {
	case KindAtom:
		return s.Atom
	case KindStr:
		return fmt.Sprintf(`"%s"`, strings.ReplaceAll(s.Str, `"`, `\"`))
	case KindInt:
		return strconv.FormatInt(s.Int, 10)
	case KindList:
		parts := make([]string, len(s.Children))
		for i, c := range s.Children {
			parts[i] = Format(c)
		}
		return "(" + strings.Join(parts, " ") + ")"
	}
	return ""
}
