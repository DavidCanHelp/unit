\ Independent integer search kernel. COUNT-PRIMES takes [start,end).
\ Install this exact source on every worker before submitting work.
\ Trial division for the small nonnegative ranges used by the experiment.
: PRIME?
  DUP 2 < IF DROP 0 ELSE
    DUP 2 = IF DROP 1 ELSE
      DUP 2 MOD 0= IF DROP 0 ELSE
        3 BEGIN 2DUP DUP * >= WHILE
          2DUP MOD 0= IF DROP DUP 1+ ELSE 2 + THEN
        REPEAT
        2DUP < IF 2DROP 0 ELSE 2DROP 1 THEN
      THEN
    THEN
  THEN
;
: COUNT-PRIMES
  2DUP >= IF 2DROP 0 ELSE
    0 ROT ROT SWAP DO I PRIME? + LOOP
  THEN
;
