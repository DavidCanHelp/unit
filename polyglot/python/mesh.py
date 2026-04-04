"""UDP mesh networking for the unit protocol."""

import os
import socket
import threading
import logging

import sexp

log = logging.getLogger(__name__)


class Peer:
    def __init__(self, id, addr):
        self.id = id
        self.addr = addr
        self.fitness = 0
        self.energy = 0


class UDPMesh:
    def __init__(self, node_id=None, port=4202):
        if node_id is None:
            node_id = os.urandom(4).hex()
        self.id = node_id
        self.port = port
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(('0.0.0.0', port))
        self.sock.settimeout(1.0)
        self.peers = {}  # id -> Peer
        self.on_message = None  # callback(parsed, addr)
        self._lock = threading.Lock()

    def listen(self):
        t = threading.Thread(target=self._recv_loop, daemon=True)
        t.start()

    def _recv_loop(self):
        while True:
            try:
                data, addr = self.sock.recvfrom(65536)
                msg = data.decode('utf-8', errors='replace')
                parsed = sexp.parse(msg)
                self._handle(parsed, addr)
                if self.on_message:
                    self.on_message(parsed, addr)
            except socket.timeout:
                continue
            except Exception:
                continue

    def _handle(self, parsed, addr):
        mt = sexp.msg_type(parsed)
        if mt in ('peer-announce', 'peer-status'):
            peer_id = sexp.get_keyword(parsed, 'id')
            if not peer_id or peer_id == self.id:
                return
            with self._lock:
                if peer_id not in self.peers:
                    self.peers[peer_id] = Peer(peer_id, addr)
                    log.info(f"discovered peer {peer_id} @ {addr}")
                p = self.peers[peer_id]
                p.addr = addr
                f = sexp.get_keyword(parsed, 'fitness')
                if isinstance(f, int):
                    p.fitness = f
                e = sexp.get_keyword(parsed, 'energy')
                if isinstance(e, int):
                    p.energy = e

    def send(self, addr, msg):
        self.sock.sendto(msg.encode(), addr)

    def send_to_all(self, msg):
        with self._lock:
            for p in self.peers.values():
                self.sock.sendto(msg.encode(), p.addr)

    def announce(self, peer_addr):
        addr = _resolve(peer_addr)
        if addr:
            msg = f'(peer-announce :id "{self.id}" :port {self.port})'
            self.send(addr, msg)

    def gossip_tick(self, fitness, energy):
        with self._lock:
            n = len(self.peers)
        msg = f'(peer-status :id "{self.id}" :peers {n} :fitness {fitness} :energy {energy})'
        self.send_to_all(msg)

    def peer_count(self):
        with self._lock:
            return len(self.peers)

    def close(self):
        self.sock.close()


def _resolve(addr_str):
    try:
        host, port = addr_str.rsplit(':', 1)
        return (host, int(port))
    except Exception:
        return None
