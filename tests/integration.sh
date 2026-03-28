#!/usr/bin/env bash
# integration.sh — Comprehensive regression test suite for unit
#
# Tests the native binary from v0.1.0 through v0.6.1.
# Each test pipes Forth commands to the binary and validates output.
#
# Usage: ./tests/integration.sh [path-to-binary]

set -euo pipefail

BINARY="${1:-./target/release/unit}"
PASS=0
FAIL=0
TOTAL=0
FAILURES=""

# Clean up any leftover state.
rm -rf ~/.unit 2>/dev/null || true

# ---------------------------------------------------------------------------
# Test helper
# ---------------------------------------------------------------------------

run_test() {
    local name="$1"
    local input="$2"
    local expected="$3"
    local mode="${4:-contains}"  # "contains", "exact", "regex"

    TOTAL=$((TOTAL + 1))

    # Run the binary with piped input. Merge stderr into stdout so we
    # capture error messages (sandbox blocks, etc.). Suppress boot noise.
    local output
    output=$(echo "$input
BYE" | UNIT_PORT=0 "$BINARY" 2>&1 | \
        grep -v '^unit v' | \
        grep -v '^Mesh node' | \
        grep -v '^auto-claim' | \
        grep -v '^resumed' | \
        grep -v '^restored' | \
        grep -v '^> $' | \
        sed 's/^> //' | \
        sed 's/ ok$//' | \
        tr -s ' ' | \
        sed 's/^ //' | \
        sed '/^$/d')

    local result=0
    case "$mode" in
        contains)
            if echo "$output" | grep -qF -- "$expected"; then
                result=0
            else
                result=1
            fi
            ;;
        exact)
            # Compare first non-empty line against expected.
            local first_line
            first_line=$(echo "$output" | head -1 | sed 's/[[:space:]]*$//')
            if [ "$first_line" = "$expected" ]; then
                result=0
            else
                result=1
            fi
            ;;
        regex)
            if echo "$output" | grep -qE "$expected"; then
                result=0
            else
                result=1
            fi
            ;;
    esac

    if [ $result -eq 0 ]; then
        echo "  PASS  $name"
        PASS=$((PASS + 1))
    else
        echo "  FAIL  $name"
        echo "        input:    $input"
        echo "        expected: $expected ($mode)"
        echo "        got:      $(echo "$output" | head -3)"
        FAIL=$((FAIL + 1))
        FAILURES="$FAILURES\n  FAIL  $name"
    fi
}

# ---------------------------------------------------------------------------
# Ensure binary exists
# ---------------------------------------------------------------------------

if [ ! -x "$BINARY" ]; then
    echo "Binary not found: $BINARY"
    echo "Run: cargo build --release"
    exit 1
fi

echo "=== unit integration test suite ==="
echo "Binary: $BINARY"
echo ""

# ---------------------------------------------------------------------------
# 1. FORTH BASICS
# ---------------------------------------------------------------------------

echo "--- 1. Forth Basics ---"
run_test "add"          "2 3 + ."       "5"     exact
run_test "subtract"     "10 3 - ."      "7"     exact
run_test "multiply"     "6 7 * ."       "42"    exact
run_test "divide"       "20 4 / ."      "5"     exact
run_test "modulo"       "17 5 MOD ."    "2"     exact
run_test "greater-true" "5 3 > ."       "-1"    exact
run_test "greater-false" "3 5 > ."      "0"     exact
run_test "equal"        "5 5 = ."       "-1"    exact
run_test "and"          "-1 0 AND ."    "0"     exact
run_test "or"           "0 -1 OR ."     "-1"    exact
run_test "not"          "-1 NOT ."      "0"     exact

# ---------------------------------------------------------------------------
# 2. STACK OPERATIONS
# ---------------------------------------------------------------------------

echo "--- 2. Stack Operations ---"
run_test "dup"      "5 DUP . ."             "5 5"       exact
run_test "drop"     "1 2 DROP ."            "1"         exact
run_test "swap"     "1 2 SWAP . ."          "1 2"       exact
run_test "over"     "1 2 OVER . . ."        "1 2 1"     exact
run_test "rot"      "1 2 3 ROT . . ."       "1 3 2"     exact
run_test "nip"      "1 2 NIP ."             "2"         exact
run_test "tuck"     "1 2 TUCK . . ."        "2 1 2"     exact
run_test "2dup"     "1 2 2DUP . . . ."      "2 1 2 1"   exact
run_test "dot-s"    "1 2 3 .S"              "<3> 1 2 3"  contains

# ---------------------------------------------------------------------------
# 3. WORD DEFINITIONS
# ---------------------------------------------------------------------------

echo "--- 3. Word Definitions ---"
run_test "square"   ": SQUARE DUP * ; 7 SQUARE ."                          "49"    exact
run_test "cube"     ": CUBE DUP DUP * * ; 3 CUBE ."                       "27"    exact
run_test "recurse"  ": FACT DUP 1 > IF DUP 1 - RECURSE * ELSE DROP 1 THEN ; 5 FACT ." "120" exact
run_test "constant" "42 CONSTANT ANSWER ANSWER ."                          "42"    exact
run_test "variable" "VARIABLE X 99 X ! X @ ."                             "99"    exact

# ---------------------------------------------------------------------------
# 4. CONTROL FLOW
# ---------------------------------------------------------------------------

echo "--- 4. Control Flow ---"
run_test "if-then"              "1 IF 42 . THEN"                            "42"            contains
run_test "if-else-false"        "0 IF 1 . ELSE 2 . THEN"                   "2"             contains
run_test "if-else-true"         "1 IF 1 . ELSE 2 . THEN"                   "1 "            contains
run_test "do-loop"              "5 0 DO I . LOOP"                           "0 1 2 3 4"     contains
run_test "do-loop-sum"          "0 10 0 DO I + LOOP ."                      "45"            exact
run_test "nested-do"            "3 0 DO 2 0 DO I . LOOP LOOP"              "0 1 0 1 0 1"   contains
run_test "nested-do-j"          "2 0 DO 2 0 DO J . LOOP LOOP"              "0 0 1 1"       contains
run_test "begin-until"          ": CD 5 BEGIN DUP . 1 - DUP 0 = UNTIL DROP ; CD" "5 4 3 2 1" contains
run_test "begin-while-repeat"   ": WH 5 BEGIN DUP 0 > WHILE DUP . 1 - REPEAT DROP ; WH" "5 4 3 2 1" contains
run_test "compiled-if-else"     ": T1 5 3 > IF 1 . ELSE 2 . THEN ; T1"     "1"             contains
run_test "compiled-if-else-f"   ": T2 3 5 > IF 1 . ELSE 2 . THEN ; T2"     "2"             contains

# ---------------------------------------------------------------------------
# 5. STRINGS AND I/O
# ---------------------------------------------------------------------------

echo "--- 5. Strings and I/O ---"
run_test "dot-quote"    ': T ." hello" ; T'     "hello"     contains
run_test "emit"         "65 EMIT"               "A"         contains
run_test "type"         ': T 72 64000 ! 105 64001 ! 64000 2 TYPE ; T' "Hi" contains

# ---------------------------------------------------------------------------
# 6. PRELUDE WORDS
# ---------------------------------------------------------------------------

echo "--- 6. Prelude Words ---"
run_test "abs-neg"      "-7 ABS ."     "7"     exact
run_test "abs-pos"      "7 ABS ."      "7"     exact
run_test "min"          "3 7 MIN ."    "3"     exact
run_test "max"          "3 7 MAX ."    "7"     exact
run_test "negate"       "5 NEGATE ."   "-5"    exact
run_test "1+"           "5 1+ ."       "6"     exact
run_test "1-"           "5 1- ."       "4"     exact
run_test "2*"           "6 2* ."       "12"    exact
run_test "2/"           "6 2/ ."       "3"     exact
run_test "0=-true"      "0 0= ."       "-1"    exact
run_test "0=-false"     "5 0= ."       "0"     exact
run_test "0<-true"      "-3 0< ."      "-1"    exact
run_test "0<-false"     "3 0< ."       "0"     exact
run_test "<>-true"      "3 5 <> ."     "-1"    exact
run_test "<>-false"     "5 5 <> ."     "0"     exact
run_test "true"         "TRUE ."       "-1"    exact
run_test "false"        "FALSE ."      "0"     exact
run_test "invert"       "-1 INVERT ."  "0"     exact

# ---------------------------------------------------------------------------
# 7. INTROSPECTION
# ---------------------------------------------------------------------------

echo "--- 7. Introspection ---"
run_test "words"    "WORDS"             "DUP"       contains
run_test "words2"   "WORDS"             "DROP"      contains
run_test "see"      ": SQ DUP * ; SEE SQ"  "DUP"   contains
run_test "see-nip"  "SEE NIP"           "SWAP DROP" contains

# ---------------------------------------------------------------------------
# 8. MESH (single node)
# ---------------------------------------------------------------------------

echo "--- 8. Mesh ---"
run_test "mesh-status"  "MESH-STATUS"   "--- mesh status ---"   contains
run_test "peers"        "PEERS ."       "0"                     exact
run_test "load"         "LOAD ."        "[0-9]"                 regex
run_test "capacity"     "CAPACITY ."    "100"                   contains
run_test "id-type"      "ID TYPE"       "[0-9a-f]"              regex

# ---------------------------------------------------------------------------
# 9. GOALS (single node)
# ---------------------------------------------------------------------------

echo "--- 9. Goals ---"
run_test "goal-desc"    '5 GOAL" test goal" DROP GOALS'     "test goal"     contains
run_test "goal-exec"    "5 GOAL{ 6 7 * } DROP GOALS"       "[exec]"        contains
run_test "goal-claim"   "5 GOAL{ 2 3 + } DROP"             "stack: 5"      contains
run_test "report"       "5 GOAL{ 1 } DROP REPORT"          "goals:"        contains
run_test "goal-result"  "5 GOAL{ 6 7 * } GOAL-RESULT"      "42"            contains

# ---------------------------------------------------------------------------
# 10. TASK DECOMPOSITION
# ---------------------------------------------------------------------------

echo "--- 10. Task Decomposition ---"
run_test "split"        "5 GOAL{ 100 5 SPLIT DO I LOOP } DROP GOALS"   "split"     contains
run_test "progress"     "5 GOAL{ 1 } DROP PROGRESS"                     "done"      contains
run_test "fork"         '5 GOAL" forkme" 3 FORK GOALS'                  "forkme"    contains

# ---------------------------------------------------------------------------
# 11. PERSISTENCE
# ---------------------------------------------------------------------------

echo "--- 11. Persistence ---"
run_test "save"         "SAVE"          "saved"         contains
run_test "snapshot"     "SNAPSHOT"      "snapshot:"     contains
run_test "snapshots"    "SNAPSHOTS"     "[0-9]"         regex
run_test "auto-save"    "AUTO-SAVE"     "auto-save:"    contains

# ---------------------------------------------------------------------------
# 12. FITNESS
# ---------------------------------------------------------------------------

echo "--- 12. Fitness ---"
run_test "fitness"      "FITNESS ."         "[0-9]"         regex
run_test "leaderboard"  "LEADERBOARD"       "leaderboard"   contains

# ---------------------------------------------------------------------------
# 13. SECURITY
# ---------------------------------------------------------------------------

echo "--- 13. Security ---"
run_test "sandbox-on"       "SANDBOX-ON"                                "sandbox: ON"       contains
run_test "sandbox-blocks"   'SANDBOX-ON FILE-DELETE" /tmp/x" .'        "blocked"           contains
run_test "sandbox-off"      "SANDBOX-ON SANDBOX-OFF"                    "sandbox: OFF"      contains
run_test "shell-blocked"    'SHELL" echo hi" DROP DROP .'               "disabled"          contains

# ---------------------------------------------------------------------------
# 14. SPAWN
# ---------------------------------------------------------------------------

echo "--- 14. Spawn ---"
run_test "package-size" "PACKAGE-SIZE ."    "[0-9]"     regex
run_test "generation"   "GENERATION ."      "0"         exact
run_test "children"     "CHILDREN"          "no children"   contains
run_test "family"       "FAMILY"            "gen: 0"    contains
run_test "quarantine"   "QUARANTINE"        "quarantine" contains

# ---------------------------------------------------------------------------
# 15. MUTATION
# ---------------------------------------------------------------------------

echo "--- 15. Mutation ---"
run_test "mutations-empty"  "MUTATIONS"     "no mutations"  contains
run_test "mutate"           "MUTATE MUTATIONS"  "["         contains
run_test "undo"             "MUTATE UNDO-MUTATE"  "undone"  contains

# ---------------------------------------------------------------------------
# 16. HOST I/O
# ---------------------------------------------------------------------------

echo "--- 16. Host I/O ---"
run_test "timestamp"    "TIMESTAMP ."       "[0-9]"         regex
run_test "env"          'ENV" HOME" TYPE'   "/"             contains
run_test "file-exists"  'FILE-EXISTS" /tmp" .'  "-1"        exact
run_test "file-exists-no" 'FILE-EXISTS" /nonexistent_xyz" .' "0" exact
run_test "io-log"       'ENV" HOME" DROP DROP IO-LOG'   "ENV HOME"  contains

# ---------------------------------------------------------------------------
# 17. EVAL
# ---------------------------------------------------------------------------

echo "--- 17. Eval ---"
run_test "eval"     'EVAL" 2 3 + ."'   "5"     contains

# ---------------------------------------------------------------------------
# 18. COMBINED / REGRESSION
# ---------------------------------------------------------------------------

echo "--- 18. Regression ---"
run_test "define-and-use"   ": DBL 2 * ; 21 DBL ."         "42"    exact
run_test "nested-calls"     ": A 1 + ; : B A A ; 5 B ."    "7"     exact
run_test "memory"           "42 1000 ! 1000 @ ."            "42"    exact
run_test "deep-recurse"     ": FACT DUP 1 > IF DUP 1 - RECURSE * ELSE DROP 1 THEN ; 10 FACT ." "3628800" exact
run_test "loop-in-def"      ": SUM 0 SWAP 0 DO I + LOOP ; 10 SUM ."  "45"    exact
run_test "string-in-def"    ': MSG ." hello world" ; MSG'   "hello world"   contains

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

rm -rf ~/.unit 2>/dev/null || true

# ---------------------------------------------------------------------------
# 19. CLI ARGUMENTS
# ---------------------------------------------------------------------------

echo "--- 19. CLI Arguments ---"

# Direct CLI tests (not piped input).
cli_test() {
    local name="$1" result="$2"
    TOTAL=$((TOTAL + 1))
    if [ "$result" -eq 0 ]; then
        echo "  PASS  $name"
        PASS=$((PASS + 1))
    else
        echo "  FAIL  $name"
        FAIL=$((FAIL + 1))
        FAILURES="$FAILURES\n  FAIL  $name"
    fi
}

"$BINARY" --version 2>&1 | grep -qF 'unit v'
cli_test "cli-version" $?

"$BINARY" --help 2>&1 | grep -qF 'USAGE'
cli_test "cli-help" $?

"$BINARY" --quiet --eval "2 3 + ." 2>/dev/null | grep -qF '5'
cli_test "cli-eval" $?

"$BINARY" --quiet --eval ": SQ DUP * ; 7 SQ ." 2>/dev/null | grep -qF '49'
cli_test "cli-eval-word" $?

"$BINARY" --quiet --no-mesh --no-prelude --eval "BLARG" 2>/dev/null | grep -qF "error: unknown word"
cli_test "cli-eval-error" $?

QUIET_OUT=$(echo 'BYE' | "$BINARY" --quiet --no-mesh --no-prelude 2>/dev/null)
echo "$QUIET_OUT" | grep -qvF 'seed online'
cli_test "cli-quiet" $?

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

echo ""
echo "==========================================="
echo "  PASSED: $PASS"
echo "  FAILED: $FAIL"
echo "  TOTAL:  $TOTAL"
echo "==========================================="

if [ $FAIL -gt 0 ]; then
    echo ""
    echo "Failures:"
    echo -e "$FAILURES"
    exit 1
else
    echo ""
    echo "All tests passed."
    exit 0
fi
