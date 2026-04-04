#!/usr/bin/env python3
"""unit-python: a Python organism that joins the Rust unit mesh.

Speaks S-expressions over UDP. Evolves solutions using AST-based
symbolic regression. Different language, different mutation strategy,
same mesh protocol.
"""

import argparse
import logging
import time

import sexp
import mesh
import challenge

logging.basicConfig(level=logging.INFO, format='%(message)s')
log = logging.getLogger(__name__)


def main():
    parser = argparse.ArgumentParser(description='unit-python organism')
    parser.add_argument('--port', type=int, default=4202, help='UDP port')
    parser.add_argument('--peer', type=str, default='', help='seed peer addr')
    parser.add_argument('--id', type=str, default='', help='8-char hex node ID')
    args = parser.parse_args()

    m = mesh.UDPMesh(node_id=args.id or None, port=args.port)
    store = challenge.ChallengeStore()
    fitness = 0
    energy = 1000
    evolve_count = 0

    def on_msg(parsed, addr):
        mt = sexp.msg_type(parsed)
        if mt == 'challenge':
            store.on_challenge(parsed)
        elif mt == 'solution':
            store.on_solution(parsed)

    m.on_message = on_msg
    m.listen()

    print(f'unit-python v0.1.0 | node {m.id} | port {m.port}')

    if args.peer:
        m.announce(args.peer)
        print(f'announced to {args.peer}')

    gossip_time = time.time()
    evolve_time = time.time()
    status_time = time.time()

    while True:
        now = time.time()

        if now - gossip_time >= 3:
            m.gossip_tick(fitness, energy)
            if args.peer:
                m.announce(args.peer)
            gossip_time = now

        if now - evolve_time >= 10:
            unsolved = store.get_unsolved()
            if unsolved:
                ch = unsolved[0]
                evolve_count += 1
                log.info(f'[evolve] attempting challenge #{ch.id}: {ch.name}')
                program, found = challenge.try_evolve_solution(ch, seed=evolve_count * 7 + 1)
                if found:
                    log.info(f'[evolve] SOLVED #{ch.id}: {program}')
                    fitness += ch.reward
                    energy += 100
                    sol = challenge.format_solution(ch.id, program, m.id)
                    m.send_to_all(sol)
                    ch.solved = True
                    ch.solution = program
                    ch.solver = m.id
                else:
                    log.info(f'[evolve] no solution yet for #{ch.id}')
                    energy -= 5
            evolve_time = now

        if now - status_time >= 30:
            total, solved = store.count()
            log.info(f'[status] peers={m.peer_count()} challenges={solved}/{total} fitness={fitness} energy={energy}')
            status_time = now

        time.sleep(0.5)


if __name__ == '__main__':
    main()
