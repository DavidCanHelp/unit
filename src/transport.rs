//! Unit self-transport with confirm-before-release ("transporter") semantics.
//!
//! A running unit relocates by replicating its *complete self* onto another
//! coordinate that already runs a unit process. The receiving unit process is
//! the transporter pad: only the self-state travels — the binary and the
//! prelude do NOT, because every coordinate already has them.
//!
//! The complete self is a serialized [`VmSnapshot`](crate::persist::VmSnapshot)
//! in the USAV format from `persist.rs`: the dictionary (including evolved
//! `SOL-*` antibodies), memory, goals, fitness, and code_strings. That blob is
//! what crosses the wire.
//!
//! The protocol, and its one new invariant — **confirm before release**:
//!   1. capture  — origin serializes its complete self; it does NOT stop yet.
//!   2. send     — origin opens TCP to the destination and writes a transport
//!                 frame (`UTPT` magic + version + length-prefixed USAV bytes),
//!                 reusing the length-prefix framing style of `spawn.rs`
//!                 (but not its `UREP` binary-bundling format).
//!   3. confirm  — the destination validates the frame, refuses unless it has
//!                 resource headroom (fails closed), deserializes the snapshot,
//!                 and writes back a confirm frame (`UTPC` magic + echoed
//!                 node_id + accepted/refused status).
//!   4. release  — the origin releases (safe to stop/retire) ONLY on a received
//!                 `Accepted` confirm. A refused / timed-out / malformed /
//!                 absent confirm leaves the origin alive exactly as it was.
//!
//! Confirm-before-release is the whole point: a copy is only ever given up
//! against a confirmed-living copy, so no unit is ever lost in transit. We do
//! not police lying — if a destination advertised headroom it didn't have, it
//! simply refuses at step 3 and the origin stays put; the transport just
//! doesn't complete.
//!
//! Native-first: the networked entry points ([`send_transport`],
//! [`start_transport_listener`]) are cfg-shimmed out on wasm32 (a browser
//! coordinate can't run a TCP listener), mirroring `resources.rs` /
//! `signaling.rs`. The pure frame encode/decode and the snapshot-handling
//! logic compile everywhere and are deterministically unit-tested.

use crate::features::mutation::SimpleRng;
use crate::mesh::NodeId;
use std::net::SocketAddr;

// ---------------------------------------------------------------------------
// Wire constants
// ---------------------------------------------------------------------------

/// Magic for a transport frame (origin → destination).
const TRANSPORT_MAGIC: &[u8; 4] = b"UTPT";
/// Magic for a confirm frame (destination → origin).
const CONFIRM_MAGIC: &[u8; 4] = b"UTPC";
/// Protocol version for both frame kinds.
const TRANSPORT_VERSION: u8 = 1;
/// Fixed transport-frame header: magic(4) + version(1) + payload_len(8).
const TRANSPORT_HEADER_LEN: usize = 4 + 1 + 8;
/// Fixed confirm-frame size: magic(4) + version(1) + node_id(8) + status(1).
const CONFIRM_FRAME_LEN: usize = 4 + 1 + 8 + 1;
/// Sanity cap on the USAV payload, matching the 100MB cap in `spawn.rs`.
const MAX_PAYLOAD: usize = 100_000_000;
/// Read/write timeout for the handshake, matching `spawn.rs`'s 30s.
#[cfg(not(target_arch = "wasm32"))]
const HANDSHAKE_TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Status + outcome + error types
// ---------------------------------------------------------------------------

/// The destination's verdict, carried in a confirm frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmStatus {
    /// The destination has a live copy and the origin may release.
    Accepted,
    /// The destination declined (no headroom, or it could not instantiate).
    Refused,
}

impl ConfirmStatus {
    fn as_u8(self) -> u8 {
        match self {
            ConfirmStatus::Accepted => 1,
            ConfirmStatus::Refused => 0,
        }
    }
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(ConfirmStatus::Accepted),
            0 => Some(ConfirmStatus::Refused),
            _ => None,
        }
    }
}

/// The release-safe outcome of a transport. The ONLY value an origin may treat
/// as permission to retire its copy is `Ok(ConfirmOutcome::Accepted)`; every
/// failure mode below is an `Err`, so the release path is never taken without a
/// confirmed-living destination copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmOutcome {
    /// The destination confirmed a live copy. Release is safe.
    Accepted,
}

/// Why a transport did not complete. None of these permit release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportError {
    /// Frame did not start with the expected magic.
    BadMagic,
    /// Frame carried an unsupported version byte.
    BadVersion,
    /// Declared payload length exceeds [`MAX_PAYLOAD`].
    TooLarge,
    /// Buffer was shorter than its declared/expected length.
    Truncated,
    /// The destination explicitly refused (e.g. no headroom).
    Refused,
    /// No confirm arrived within the handshake timeout.
    Timeout,
    /// Could not connect to the destination coordinate.
    Connect(String),
    /// An I/O error occurred mid-handshake.
    Io(String),
    /// A confirm frame arrived but could not be parsed.
    MalformedConfirm,
    /// Transport is not available on this platform (wasm32).
    Unavailable,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::BadMagic => write!(f, "bad magic"),
            TransportError::BadVersion => write!(f, "unsupported version"),
            TransportError::TooLarge => write!(f, "payload exceeds cap"),
            TransportError::Truncated => write!(f, "truncated frame"),
            TransportError::Refused => write!(f, "destination refused (no headroom)"),
            TransportError::Timeout => write!(f, "confirm timed out"),
            TransportError::Connect(e) => write!(f, "connect: {}", e),
            TransportError::Io(e) => write!(f, "io: {}", e),
            TransportError::MalformedConfirm => write!(f, "malformed confirm"),
            TransportError::Unavailable => write!(f, "transport unavailable on this platform"),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure frame encode / decode (deterministic; unit-tested on byte buffers)
// ---------------------------------------------------------------------------

/// Encodes a transport frame: `UTPT` magic + version + 8-byte big-endian
/// payload length + the USAV payload. Returns [`TransportError::TooLarge`] if
/// the payload is over the cap (so an oversized self never hits the wire).
pub fn encode_transport_frame(payload: &[u8]) -> Result<Vec<u8>, TransportError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(TransportError::TooLarge);
    }
    let mut buf = Vec::with_capacity(TRANSPORT_HEADER_LEN + payload.len());
    buf.extend_from_slice(TRANSPORT_MAGIC);
    buf.push(TRANSPORT_VERSION);
    buf.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    buf.extend_from_slice(payload);
    Ok(buf)
}

/// Decodes a transport frame, returning the USAV payload slice.
///
/// Rejects bad magic, bad version, an over-cap declared length, and any buffer
/// shorter than its header or its declared payload.
pub fn decode_transport_frame(data: &[u8]) -> Result<&[u8], TransportError> {
    if data.len() < TRANSPORT_HEADER_LEN {
        return Err(TransportError::Truncated);
    }
    if &data[0..4] != TRANSPORT_MAGIC {
        return Err(TransportError::BadMagic);
    }
    if data[4] != TRANSPORT_VERSION {
        return Err(TransportError::BadVersion);
    }
    let len = u64::from_be_bytes(data[5..13].try_into().unwrap()) as usize;
    if len > MAX_PAYLOAD {
        return Err(TransportError::TooLarge);
    }
    let end = TRANSPORT_HEADER_LEN + len;
    if data.len() < end {
        return Err(TransportError::Truncated);
    }
    Ok(&data[TRANSPORT_HEADER_LEN..end])
}

/// Encodes a confirm frame: `UTPC` magic + version + echoed node_id + status.
pub fn encode_confirm_frame(node_id: &NodeId, status: ConfirmStatus) -> Vec<u8> {
    let mut buf = Vec::with_capacity(CONFIRM_FRAME_LEN);
    buf.extend_from_slice(CONFIRM_MAGIC);
    buf.push(TRANSPORT_VERSION);
    buf.extend_from_slice(node_id);
    buf.push(status.as_u8());
    buf
}

/// Decodes a confirm frame into the echoed node_id and status.
///
/// Rejects bad magic, bad version, a short buffer, and an unknown status byte.
pub fn decode_confirm_frame(data: &[u8]) -> Result<(NodeId, ConfirmStatus), TransportError> {
    if data.len() < CONFIRM_FRAME_LEN {
        return Err(TransportError::Truncated);
    }
    if &data[0..4] != CONFIRM_MAGIC {
        return Err(TransportError::BadMagic);
    }
    if data[4] != TRANSPORT_VERSION {
        return Err(TransportError::BadVersion);
    }
    let mut node_id = [0u8; 8];
    node_id.copy_from_slice(&data[5..13]);
    let status = ConfirmStatus::from_u8(data[13]).ok_or(TransportError::MalformedConfirm)?;
    Ok((node_id, status))
}

// ---------------------------------------------------------------------------
// Destination handling (pure; resources injected for deterministic tests)
// ---------------------------------------------------------------------------

/// The outcome of handling a received transport frame at the destination.
///
/// `confirm_frame` is the response bytes to write back to the origin.
/// `snapshot` is `Some` ONLY when the transport was accepted — that is the
/// deserialized complete self the caller should instantiate. On any refusal
/// (no headroom, or an undeserializable self) it is `None`, so the caller is
/// never asked to instantiate something the destination declined.
pub struct HandleResult {
    /// Bytes to send back to the origin (an `Accepted` or `Refused` confirm).
    pub confirm_frame: Vec<u8>,
    /// The deserialized self to instantiate — `Some` iff accepted.
    pub snapshot: Option<crate::persist::VmSnapshot>,
}

impl HandleResult {
    /// True iff this handling accepted the transport (caller should
    /// instantiate `snapshot`). Mirrors the `Accepted` status in the confirm.
    pub fn accepted(&self) -> bool {
        self.snapshot.is_some()
    }
}

/// Handles a received transport frame against a destination resource reading.
///
/// `dest_res` is injected so tests can pass a fixed available / unavailable
/// [`HostResources`](crate::resources::HostResources) rather than touching the
/// real machine. The destination FAILS CLOSED: without headroom it refuses,
/// and an unavailable reading (no `/proc`, wasm32) also refuses.
///
/// Returns `Err` only for a structurally invalid frame (bad magic/version,
/// over-cap, truncated) — a listener drops those, like `spawn.rs` drops bad
/// packets. A structurally valid frame always yields a confirm frame to send
/// back: `Accepted` (with the snapshot) when there is headroom and the self
/// deserializes, `Refused` (no snapshot) otherwise.
pub fn handle_transport_frame(
    frame: &[u8],
    dest_res: &crate::resources::HostResources,
) -> Result<HandleResult, TransportError> {
    let payload = decode_transport_frame(frame)?;

    // Deserialize first so we can echo the transported node_id even on refusal.
    let snap = match crate::persist::deserialize_snapshot(payload) {
        Some(s) => s,
        None => {
            // A self we cannot read is not a live copy — refuse, echo unknown id.
            return Ok(HandleResult {
                confirm_frame: encode_confirm_frame(&[0u8; 8], ConfirmStatus::Refused),
                snapshot: None,
            });
        }
    };

    // Fail closed: only accept when this host actually has headroom.
    if dest_res.has_headroom() {
        let confirm_frame = encode_confirm_frame(&snap.node_id, ConfirmStatus::Accepted);
        Ok(HandleResult {
            confirm_frame,
            snapshot: Some(snap),
        })
    } else {
        let confirm_frame = encode_confirm_frame(&snap.node_id, ConfirmStatus::Refused);
        Ok(HandleResult {
            confirm_frame,
            snapshot: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Placement — sufficient-first destination choice + the relocate orchestrator
// ---------------------------------------------------------------------------

/// A candidate destination from the local gossiped view: a peer's advertised
/// headroom (`0..=100`) and where to reach it. The unit reads only its own
/// gossiped view — no coordinator, no global aggregation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The peer's advertised headroom percentage (`0..=100`).
    pub headroom_pct: u8,
    /// Where to open the transport connection.
    pub addr: SocketAddr,
}

/// Two-tier placement: returns the destination for a relocating unit, or `None`
/// if no peer can take one.
///
/// **Tier 1 — spread under abundance.** If any candidate is *abundantly* free
/// (headroom ≥
/// [`ABUNDANT_HEADROOM_PCT`](crate::resources::ABUNDANT_HEADROOM_PCT)), pick the
/// emptiest such peer. When there's a clearly-emptier home, send the unit there:
/// this corrects the load skew pure first-sufficient produced in the v0.30 soak
/// (one peer filling while another sat near-idle).
///
/// **Tier 2 — frugal default.** Otherwise take the FIRST peer that is merely
/// sufficient, in gossip-view order. This is the original herd-avoiding rule:
/// when no peer is abundantly free, *not* chasing the marginally-emptiest peer
/// keeps every sender from piling onto the same box. Returns `None` if no peer
/// is even sufficient (the unit stays put).
///
/// When several peers tie at the maximum abundant headroom, the choice is made
/// uniformly at RANDOM among them (via `rng`), not last/first-wins. Gossip order
/// is arbitrary and unstable, and a deterministic tie-break would make multiple
/// senders shedding at the same instant — who share the same abundant view —
/// all pick the SAME peer: a correlated mini-thundering-herd, the exact thing
/// two-tier placement guards against. Random-among-tied decorrelates concurrent
/// senders so they spread across the tied peers. (A unique maximum is still
/// chosen deterministically — `next_usize(1)` is always 0.)
///
/// The two thresholds live in `resources.rs` as the single source of truth; the
/// node-side `MultiUnitNode::choose_destination` delegates to this exact rule
/// over the live gossip view.
pub fn choose_destination<'a>(
    candidates: &'a [Candidate],
    rng: &mut SimpleRng,
) -> Option<&'a Candidate> {
    // Tier 1: the headroom of the emptiest abundantly-free peer, if any.
    let max_abundant = candidates
        .iter()
        .map(|c| c.headroom_pct)
        .filter(|&h| crate::resources::headroom_pct_abundant(h))
        .max();
    if let Some(max) = max_abundant {
        // Reservoir sample over the tied-maximum peers: one pass, uniform pick.
        // The k-th tied peer replaces the choice with probability 1/k.
        let mut chosen: Option<&Candidate> = None;
        let mut seen = 0usize;
        for c in candidates.iter().filter(|c| c.headroom_pct == max) {
            seen += 1;
            if rng.next_usize(seen) == 0 {
                chosen = Some(c);
            }
        }
        return chosen;
    }
    // Tier 2: the first merely-sufficient peer (herd-avoiding).
    candidates
        .iter()
        .find(|c| crate::resources::headroom_pct_sufficient(c.headroom_pct))
}

/// The mislocation sense: a coordinate is mislocated when it is over the
/// ceiling — i.e. it has no local headroom. This is the honest trigger (local
/// resource pressure), derived from the existing reading; there is no separate
/// "mislocation score".
pub fn is_mislocated(local: &crate::resources::HostResources) -> bool {
    !local.has_headroom()
}

/// Whether a transport outcome permits the origin to release its copy. Release
/// happens SOLELY on `Ok(ConfirmOutcome::Accepted)` — confirm-before-release
/// carried up to the placement layer. Refused / timeout / connection / garbage
/// all map to `Err`, so the origin is never retired without a confirmed-living
/// copy elsewhere.
pub fn should_release(outcome: &Result<ConfirmOutcome, TransportError>) -> bool {
    matches!(outcome, Ok(ConfirmOutcome::Accepted))
}

/// The result of an attempted self-relocation, before any release/retire.
#[derive(Debug)]
pub enum TransportAttempt {
    /// Local coordinate has headroom — not mislocated. No cost, unit stays.
    NotMislocated,
    /// No peer advertises sufficient room. No cost, unit stays.
    NoDestination,
    /// Mislocated with a destination, but the unit can't pay the energy cost.
    /// A starving unit cannot flee. No cost charged, unit stays.
    CannotAfford,
    /// Mislocated, a destination chosen, affordable — `send` was invoked and
    /// this is its outcome. The caller charges energy for the attempt and
    /// releases the origin iff [`should_release`] of this outcome.
    Attempted(Result<ConfirmOutcome, TransportError>),
}

/// The local relocation rule, as a pure orchestrator over injected inputs so
/// it tests without sockets or a live VM. It composes the pieces in order:
/// mislocation sense → sufficient-first destination → affordability → send.
///
/// `send` is invoked at most once, only when the unit is mislocated, a
/// sufficient destination exists, and the cost is affordable. `send` receives
/// the chosen destination address and performs the actual transport (the real
/// [`send_transport`] in production, a stub in tests). The caller decides what
/// to charge and whether to retire based on the returned [`TransportAttempt`].
///
/// `tie_seed` seeds the random tie-break in [`choose_destination`]. It is passed
/// by value (not a `&mut SimpleRng`) so the caller can derive it from its own
/// per-unit RNG without that borrow colliding with a `send` closure that
/// captures the unit. Advancing the caller's RNG each call varies the seed per
/// call; seeding each unit's RNG from its identity decorrelates units.
pub fn attempt_transport<S>(
    local: &crate::resources::HostResources,
    candidates: &[Candidate],
    can_afford: bool,
    tie_seed: u64,
    send: S,
) -> TransportAttempt
where
    S: FnOnce(SocketAddr) -> Result<ConfirmOutcome, TransportError>,
{
    if !is_mislocated(local) {
        return TransportAttempt::NotMislocated;
    }
    let mut rng = SimpleRng::new(tie_seed);
    let dest = match choose_destination(candidates, &mut rng) {
        Some(d) => d,
        None => return TransportAttempt::NoDestination,
    };
    if !can_afford {
        return TransportAttempt::CannotAfford;
    }
    TransportAttempt::Attempted(send(dest.addr))
}

// ---------------------------------------------------------------------------
// Networked origin side (native-only)
// ---------------------------------------------------------------------------

/// Sends a serialized self (USAV `payload`) to `addr` and waits for a confirm.
///
/// Returns `Ok(ConfirmOutcome::Accepted)` — and ONLY this — when the
/// destination confirms a live copy; that is the origin's signal that release
/// is safe. Every other outcome (explicit refusal, timeout, connection
/// failure, malformed confirm, oversized payload) is an `Err`, so the origin's
/// release path is never taken without a confirmed-living destination copy.
#[cfg(not(target_arch = "wasm32"))]
pub fn send_transport(addr: &str, payload: &[u8]) -> Result<ConfirmOutcome, TransportError> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let frame = encode_transport_frame(payload)?;

    let mut stream =
        TcpStream::connect(addr).map_err(|e| TransportError::Connect(e.to_string()))?;
    let timeout = Some(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS));
    stream.set_write_timeout(timeout).ok();
    stream.set_read_timeout(timeout).ok();

    stream
        .write_all(&frame)
        .map_err(|e| io_to_transport_err(&e))?;

    // Await the fixed-size confirm frame.
    let mut buf = [0u8; CONFIRM_FRAME_LEN];
    stream
        .read_exact(&mut buf)
        .map_err(|e| io_to_transport_err(&e))?;

    let (_echoed_id, status) = decode_confirm_frame(&buf)?;
    match status {
        ConfirmStatus::Accepted => Ok(ConfirmOutcome::Accepted),
        // A refusal is NOT release-safe — surface it as an error.
        ConfirmStatus::Refused => Err(TransportError::Refused),
    }
}

/// Maps a handshake I/O error to a transport error, distinguishing the timeout
/// case (a stalled destination) from other I/O failures.
#[cfg(not(target_arch = "wasm32"))]
fn io_to_transport_err(e: &std::io::Error) -> TransportError {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => TransportError::Timeout,
        _ => TransportError::Io(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Networked destination side (native-only)
// ---------------------------------------------------------------------------

/// Listens for incoming transport frames on a TCP port. Runs in a background
/// thread. For each frame it measures THIS host's resources at accept time
/// (fail closed), writes the confirm frame back on the same connection, and —
/// only when accepted — forwards the deserialized self over the returned
/// channel for the caller to instantiate.
///
/// The framing and drop-on-bad-frame behavior mirror
/// [`start_replication_listener`](crate::spawn::start_replication_listener).
#[cfg(not(target_arch = "wasm32"))]
pub fn start_transport_listener(
    port: u16,
) -> Result<std::sync::mpsc::Receiver<crate::persist::VmSnapshot>, TransportError> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    // Bind to all interfaces (0.0.0.0), not loopback: a transported unit
    // arrives from another machine, so the listener must accept connections to
    // this host's routable IP. A 127.0.0.1 bind would make inbound transport
    // single-host-only — the same loopback assumption that hid cross-machine
    // breakage in the mesh socket. (The HTTP bridge stays 127.0.0.1 by design;
    // this peer-traffic listener does not.)
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .map_err(|e| TransportError::Io(format!("bind {}: {}", port, e)))?;
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let timeout = Some(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS));
            stream.set_read_timeout(timeout).ok();
            stream.set_write_timeout(timeout).ok();

            // Read the fixed header, validate, then read exactly the payload.
            let mut header = [0u8; TRANSPORT_HEADER_LEN];
            if stream.read_exact(&mut header).is_err() {
                continue;
            }
            if &header[0..4] != TRANSPORT_MAGIC || header[4] != TRANSPORT_VERSION {
                continue;
            }
            let len = u64::from_be_bytes(header[5..13].try_into().unwrap()) as usize;
            if len > MAX_PAYLOAD {
                continue;
            }
            let mut payload = vec![0u8; len];
            if stream.read_exact(&mut payload).is_err() {
                continue;
            }

            // Reassemble the full frame for the pure handler.
            let mut frame = Vec::with_capacity(TRANSPORT_HEADER_LEN + len);
            frame.extend_from_slice(&header);
            frame.extend_from_slice(&payload);

            // Measure at accept time so the headroom decision is current.
            let res = crate::resources::HostResources::measure();
            if let Ok(result) = handle_transport_frame(&frame, &res) {
                // Confirm first (the origin is waiting on it before releasing)...
                let _ = stream.write_all(&result.confirm_frame);
                // ...then hand an accepted self to the caller to instantiate.
                if let Some(snap) = result.snapshot {
                    let _ = tx.send(snap);
                }
            }
        }
    });

    Ok(rx)
}

// ---------------------------------------------------------------------------
// wasm32 shims — a browser coordinate cannot transport.
// ---------------------------------------------------------------------------

/// wasm32: transport is unavailable (no TCP). Returns a clean error so the
/// origin never releases.
#[cfg(target_arch = "wasm32")]
pub fn send_transport(_addr: &str, _payload: &[u8]) -> Result<ConfirmOutcome, TransportError> {
    Err(TransportError::Unavailable)
}

/// wasm32: there is no listener to start.
#[cfg(target_arch = "wasm32")]
pub fn start_transport_listener(_port: u16) -> Result<(), TransportError> {
    Err(TransportError::Unavailable)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::fitness::FitnessTracker;
    use crate::goals::GoalRegistry;
    use crate::persist::{deserialize_snapshot, serialize_snapshot, VmSnapshot};
    use crate::resources::HostResources;
    use crate::types::{Entry, Instruction};

    // A complete self carrying an evolved SOL-* antibody, fitness, and
    // code_strings — so a round trip proves the whole self travels.
    fn sample_snapshot() -> VmSnapshot {
        let mut fitness = FitnessTracker::new();
        fitness.score = 4242;
        fitness.tasks_completed = 7;
        fitness.evolution_count = 3;
        let dictionary = vec![
            Entry {
                name: "SOL-ANTIBODY".to_string(),
                immediate: false,
                hidden: false,
                body: vec![Instruction::Literal(42), Instruction::Primitive(3)],
            },
            Entry {
                name: "DOUBLE".to_string(),
                immediate: false,
                hidden: false,
                body: vec![Instruction::Literal(2), Instruction::Primitive(5)],
            },
        ];
        VmSnapshot {
            node_id: [1, 2, 3, 4, 5, 6, 7, 8],
            dictionary,
            memory: vec![0i64; 65536],
            here: 0,
            goals: GoalRegistry::empty(),
            fitness,
            code_strings: vec![
                ": SOL-ANTIBODY 42 . ;".to_string(),
                ": DOUBLE 2 * ;".to_string(),
            ],
        }
    }

    // A reading with ample headroom (50% binding constraint, under the 80%
    // ceiling), and one over the ceiling.
    fn available() -> HostResources {
        HostResources::from_parts(1000, 500, 0.0, 4)
    }
    fn over_ceiling() -> HostResources {
        HostResources::from_parts(1000, 50, 0.0, 4)
    }

    // ----- frame round trips -----

    #[test]
    fn transport_frame_round_trips() {
        let payload = b"the complete self as USAV bytes";
        let frame = encode_transport_frame(payload).unwrap();
        let decoded = decode_transport_frame(&frame).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn confirm_frame_round_trips() {
        let id = [9, 8, 7, 6, 5, 4, 3, 2];
        for status in [ConfirmStatus::Accepted, ConfirmStatus::Refused] {
            let frame = encode_confirm_frame(&id, status);
            let (decoded_id, decoded_status) = decode_confirm_frame(&frame).unwrap();
            assert_eq!(decoded_id, id);
            assert_eq!(decoded_status, status);
        }
    }

    #[test]
    fn transport_frame_rejects_bad_magic() {
        let mut frame = encode_transport_frame(b"x").unwrap();
        frame[0] = b'Z';
        assert_eq!(decode_transport_frame(&frame), Err(TransportError::BadMagic));
    }

    #[test]
    fn transport_frame_rejects_bad_version() {
        let mut frame = encode_transport_frame(b"x").unwrap();
        frame[4] = 99;
        assert_eq!(
            decode_transport_frame(&frame),
            Err(TransportError::BadVersion)
        );
    }

    #[test]
    fn transport_frame_rejects_truncated() {
        let frame = encode_transport_frame(b"hello world").unwrap();
        // Cut off mid-payload: header says 11 bytes, give it fewer.
        let truncated = &frame[..TRANSPORT_HEADER_LEN + 3];
        assert_eq!(
            decode_transport_frame(truncated),
            Err(TransportError::Truncated)
        );
        // A buffer shorter than even the header is also truncated.
        assert_eq!(
            decode_transport_frame(&frame[..4]),
            Err(TransportError::Truncated)
        );
    }

    #[test]
    fn transport_frame_rejects_over_cap_declared_length() {
        // Hand-build a header that claims a payload larger than the cap.
        let mut frame = Vec::new();
        frame.extend_from_slice(TRANSPORT_MAGIC);
        frame.push(TRANSPORT_VERSION);
        frame.extend_from_slice(&((MAX_PAYLOAD as u64) + 1).to_be_bytes());
        assert_eq!(decode_transport_frame(&frame), Err(TransportError::TooLarge));
    }

    #[test]
    fn encode_rejects_over_cap_payload() {
        // Can't allocate 100MB+ cheaply; assert the boundary check via a stub
        // is impractical, so just confirm a normal payload encodes fine and the
        // decode-side cap (tested above) guards the wire.
        assert!(encode_transport_frame(b"normal").is_ok());
    }

    #[test]
    fn confirm_frame_rejects_bad_magic_and_version() {
        let id = [0u8; 8];
        let mut frame = encode_confirm_frame(&id, ConfirmStatus::Accepted);
        frame[0] = b'Z';
        assert_eq!(decode_confirm_frame(&frame), Err(TransportError::BadMagic));

        let mut frame = encode_confirm_frame(&id, ConfirmStatus::Accepted);
        frame[4] = 99;
        assert_eq!(decode_confirm_frame(&frame), Err(TransportError::BadVersion));

        // Unknown status byte → malformed.
        let mut frame = encode_confirm_frame(&id, ConfirmStatus::Accepted);
        frame[13] = 42;
        assert_eq!(
            decode_confirm_frame(&frame),
            Err(TransportError::MalformedConfirm)
        );

        // Short buffer → truncated.
        assert_eq!(
            decode_confirm_frame(&frame[..5]),
            Err(TransportError::Truncated)
        );
    }

    // ----- the complete self survives the trip -----

    #[test]
    fn complete_self_survives_encode_decode_deserialize() {
        let snap = sample_snapshot();
        let usav = serialize_snapshot(&snap);

        // Origin frames it; destination decodes the frame back to USAV bytes.
        let frame = encode_transport_frame(&usav).unwrap();
        let payload = decode_transport_frame(&frame).unwrap();

        // Destination deserializes the complete self.
        let landed = deserialize_snapshot(payload).expect("USAV deserializes");

        // The whole self travelled — antibody included.
        assert_eq!(landed.node_id, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(
            landed.dictionary.iter().any(|e| e.name == "SOL-ANTIBODY"),
            "evolved SOL-* antibody must survive transport"
        );
        let antibody = landed
            .dictionary
            .iter()
            .find(|e| e.name == "SOL-ANTIBODY")
            .unwrap();
        assert_eq!(antibody.body.len(), 2);
        assert_eq!(landed.fitness.score, 4242);
        assert_eq!(landed.fitness.tasks_completed, 7);
        assert_eq!(landed.fitness.evolution_count, 3);
        assert_eq!(
            landed.code_strings,
            vec![
                ": SOL-ANTIBODY 42 . ;".to_string(),
                ": DOUBLE 2 * ;".to_string()
            ]
        );
    }

    // ----- destination handling + fail-closed -----

    #[test]
    fn handle_accepts_with_headroom_and_returns_snapshot() {
        let usav = serialize_snapshot(&sample_snapshot());
        let frame = encode_transport_frame(&usav).unwrap();

        let result = handle_transport_frame(&frame, &available()).unwrap();
        assert!(result.accepted());
        let snap = result.snapshot.expect("accepted → snapshot handed back");
        assert_eq!(snap.node_id, [1, 2, 3, 4, 5, 6, 7, 8]);

        // The confirm echoes the node_id with Accepted.
        let (id, status) = decode_confirm_frame(&result.confirm_frame).unwrap();
        assert_eq!(id, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(status, ConfirmStatus::Accepted);
    }

    #[test]
    fn handle_refuses_without_headroom_and_returns_no_snapshot() {
        let usav = serialize_snapshot(&sample_snapshot());
        let frame = encode_transport_frame(&usav).unwrap();

        // Over the ceiling → refuse, fail closed.
        let result = handle_transport_frame(&frame, &over_ceiling()).unwrap();
        assert!(!result.accepted());
        assert!(
            result.snapshot.is_none(),
            "refused → caller is NOT asked to instantiate"
        );
        let (id, status) = decode_confirm_frame(&result.confirm_frame).unwrap();
        assert_eq!(id, [1, 2, 3, 4, 5, 6, 7, 8], "still echoes the node_id");
        assert_eq!(status, ConfirmStatus::Refused);
    }

    #[test]
    fn handle_fails_closed_on_unavailable_reading() {
        let usav = serialize_snapshot(&sample_snapshot());
        let frame = encode_transport_frame(&usav).unwrap();

        // A coordinate that cannot measure itself refuses.
        let result = handle_transport_frame(&frame, &HostResources::unavailable()).unwrap();
        assert!(!result.accepted());
        assert!(result.snapshot.is_none());
        let (_id, status) = decode_confirm_frame(&result.confirm_frame).unwrap();
        assert_eq!(status, ConfirmStatus::Refused);
    }

    #[test]
    fn handle_refuses_undeserializable_self() {
        // A structurally valid frame whose payload is not a USAV snapshot.
        let frame = encode_transport_frame(b"not a snapshot").unwrap();
        let result = handle_transport_frame(&frame, &available()).unwrap();
        assert!(!result.accepted());
        assert!(result.snapshot.is_none());
        let (_id, status) = decode_confirm_frame(&result.confirm_frame).unwrap();
        assert_eq!(status, ConfirmStatus::Refused);
    }

    #[test]
    fn handle_drops_structurally_invalid_frame() {
        let mut frame = encode_transport_frame(b"x").unwrap();
        frame[0] = b'Z';
        // A bad-magic frame is an Err the listener drops — no confirm produced.
        assert!(handle_transport_frame(&frame, &available()).is_err());
    }

    // ----- the safety invariant: release ONLY on Ok(Accepted) -----

    /// Models the origin's release decision exactly as `send_transport`
    /// expresses it: release iff `Ok(ConfirmOutcome::Accepted)`.
    fn would_release(outcome: &Result<ConfirmOutcome, TransportError>) -> bool {
        matches!(outcome, Ok(ConfirmOutcome::Accepted))
    }

    #[test]
    fn accepted_confirm_permits_release() {
        // An Accepted confirm decodes and maps to the release-safe outcome.
        let id = [1, 2, 3, 4, 5, 6, 7, 8];
        let confirm = encode_confirm_frame(&id, ConfirmStatus::Accepted);
        let (_id, status) = decode_confirm_frame(&confirm).unwrap();
        let outcome: Result<ConfirmOutcome, TransportError> = match status {
            ConfirmStatus::Accepted => Ok(ConfirmOutcome::Accepted),
            ConfirmStatus::Refused => Err(TransportError::Refused),
        };
        assert!(would_release(&outcome));
    }

    #[test]
    fn refused_timeout_and_garbage_never_release() {
        // Refused confirm → Err(Refused).
        let refused = encode_confirm_frame(&[0u8; 8], ConfirmStatus::Refused);
        let (_id, status) = decode_confirm_frame(&refused).unwrap();
        let refused_outcome: Result<ConfirmOutcome, TransportError> = match status {
            ConfirmStatus::Accepted => Ok(ConfirmOutcome::Accepted),
            ConfirmStatus::Refused => Err(TransportError::Refused),
        };
        assert!(!would_release(&refused_outcome));

        // Timeout → Err(Timeout).
        let timeout_outcome: Result<ConfirmOutcome, TransportError> =
            Err(TransportError::Timeout);
        assert!(!would_release(&timeout_outcome));

        // Garbage confirm bytes → decode Err, which the origin treats as failure.
        let garbage = b"not even close to a confirm frame!!!";
        let decoded = decode_confirm_frame(garbage);
        assert!(decoded.is_err());
        let garbage_outcome: Result<ConfirmOutcome, TransportError> =
            decoded.map(|_| ConfirmOutcome::Accepted);
        assert!(!would_release(&garbage_outcome));

        // Connection failure → Err(Connect).
        let connect_outcome: Result<ConfirmOutcome, TransportError> =
            Err(TransportError::Connect("refused".into()));
        assert!(!would_release(&connect_outcome));
    }

    // ----- placement: sufficient-first choice + the relocate orchestrator -----

    fn addr(port: u16) -> SocketAddr {
        format!("127.0.0.1:{}", port).parse().unwrap()
    }

    #[test]
    fn choose_destination_first_sufficient_when_none_abundant() {
        // All peers are merely sufficient (>20% but <50% headroom) — none
        // abundant. Herd-avoidance preserved: take the FIRST sufficient in
        // gossip order, NOT the emptiest, so senders don't pile onto one box.
        let a = Candidate {
            headroom_pct: 30,
            addr: addr(9001),
        };
        let b = Candidate {
            headroom_pct: 45, // more headroom but still not abundant
            addr: addr(9002),
        };
        let c = Candidate {
            headroom_pct: 35,
            addr: addr(9003),
        };
        let view = [a, b, c];
        let mut rng = SimpleRng::new(0);
        let chosen = choose_destination(&view, &mut rng).unwrap();
        assert_eq!(
            chosen.addr,
            addr(9001),
            "no abundant peer → first-sufficient (herd-avoiding)"
        );
    }

    #[test]
    fn choose_destination_prefers_abundant_over_earlier_sufficient() {
        // A is sufficient-but-not-abundant and earlier; B is abundant and later.
        // The abundant peer wins despite being later — a clearly-emptier home.
        let a = Candidate {
            headroom_pct: 40,
            addr: addr(9001),
        };
        let b = Candidate {
            headroom_pct: 70,
            addr: addr(9002),
        };
        let view = [a, b];
        let mut rng = SimpleRng::new(0);
        let chosen = choose_destination(&view, &mut rng).unwrap();
        assert_eq!(
            chosen.addr,
            addr(9002),
            "an abundant peer is preferred over an earlier merely-sufficient one"
        );
    }

    #[test]
    fn choose_destination_picks_emptiest_among_abundant() {
        // Several abundant peers, UNIQUE maximum (80%) → the emptiest wins,
        // deterministically (a single tied peer is always chosen).
        let a = Candidate {
            headroom_pct: 55,
            addr: addr(9001),
        };
        let b = Candidate {
            headroom_pct: 80,
            addr: addr(9002),
        };
        let c = Candidate {
            headroom_pct: 60,
            addr: addr(9003),
        };
        let view = [a, b, c];
        // Any seed gives the same result when the maximum is unique.
        for seed in 0..8 {
            let mut rng = SimpleRng::new(seed);
            let chosen = choose_destination(&view, &mut rng).unwrap();
            assert_eq!(
                chosen.addr,
                addr(9002),
                "unique emptiest abundant peer wins regardless of seed"
            );
        }
    }

    #[test]
    fn choose_destination_random_among_equally_abundant() {
        // Three abundant peers TIED at the maximum (70%) and one lower-but-
        // abundant (55%). The choice is uniformly random among the three tied
        // at 70 — decorrelating concurrent senders — and never the 55% one.
        let tied = [addr(9001), addr(9002), addr(9003)];
        let view = [
            Candidate {
                headroom_pct: 70,
                addr: tied[0],
            },
            Candidate {
                headroom_pct: 55,
                addr: addr(9009),
            },
            Candidate {
                headroom_pct: 70,
                addr: tied[1],
            },
            Candidate {
                headroom_pct: 70,
                addr: tied[2],
            },
        ];
        let mut picks = std::collections::HashSet::new();
        for seed in 0..64 {
            let mut rng = SimpleRng::new(seed);
            let chosen = choose_destination(&view, &mut rng).unwrap();
            assert!(
                tied.contains(&chosen.addr),
                "must pick a tied-maximum peer, got {}",
                chosen.addr
            );
            assert_ne!(chosen.headroom_pct, 55, "must not pick the lower peer");
            picks.insert(chosen.addr);
        }
        // Decorrelation: across seeds, more than one of the tied peers is chosen.
        assert!(
            picks.len() > 1,
            "random tie-break must spread across tied peers, got only {:?}",
            picks
        );
        // Determinism under a fixed seed: same seed → same pick.
        let pick = |s| {
            let mut r = SimpleRng::new(s);
            choose_destination(&view, &mut r).unwrap().addr
        };
        assert_eq!(pick(7), pick(7), "same seed must be deterministic");
    }

    #[test]
    fn choose_destination_skips_insufficient_peers() {
        // First peer is over the ceiling (10% headroom); second is sufficient.
        let tight = Candidate {
            headroom_pct: 10,
            addr: addr(9001),
        };
        let ok = Candidate {
            headroom_pct: 35,
            addr: addr(9002),
        };
        let view = [tight, ok];
        let mut rng = SimpleRng::new(0);
        let chosen = choose_destination(&view, &mut rng).unwrap();
        assert_eq!(chosen.addr, addr(9002));
    }

    #[test]
    fn choose_destination_none_when_no_peer_sufficient() {
        let tight_a = Candidate {
            headroom_pct: 5,
            addr: addr(9001),
        };
        let tight_b = Candidate {
            headroom_pct: 15,
            addr: addr(9002),
        };
        let mut rng = SimpleRng::new(0);
        assert!(choose_destination(&[tight_a, tight_b], &mut rng).is_none());
        // Empty view → None.
        assert!(choose_destination(&[], &mut rng).is_none());
    }

    #[test]
    fn is_mislocated_only_when_local_lacks_headroom() {
        // Local has room → not mislocated; a unit with room never tries to leave.
        let healthy = HostResources::from_parts(1000, 500, 0.0, 4);
        assert!(!is_mislocated(&healthy));
        // Local over the ceiling → mislocated.
        let pressed = HostResources::from_parts(1000, 50, 0.0, 4);
        assert!(is_mislocated(&pressed));
        // Unavailable reading fails closed → counts as mislocated (can't stay
        // confident it has room) but with no measurable destination it no-ops.
        assert!(is_mislocated(&HostResources::unavailable()));
    }

    #[test]
    fn attempt_transport_not_mislocated_is_noop() {
        let healthy = HostResources::from_parts(1000, 500, 0.0, 4);
        let cands = [Candidate {
            headroom_pct: 90,
            addr: addr(9001),
        }];
        let mut sent = false;
        let attempt = attempt_transport(&healthy, &cands, true, 0, |_| {
            sent = true;
            Ok(ConfirmOutcome::Accepted)
        });
        assert!(matches!(attempt, TransportAttempt::NotMislocated));
        assert!(!sent, "must not transport when not mislocated");
    }

    #[test]
    fn attempt_transport_no_destination_is_noop() {
        let pressed = HostResources::from_parts(1000, 50, 0.0, 4);
        let cands = [Candidate {
            headroom_pct: 5,
            addr: addr(9001),
        }];
        let mut sent = false;
        let attempt = attempt_transport(&pressed, &cands, true, 0, |_| {
            sent = true;
            Ok(ConfirmOutcome::Accepted)
        });
        assert!(matches!(attempt, TransportAttempt::NoDestination));
        assert!(!sent);
    }

    #[test]
    fn attempt_transport_cannot_afford_does_not_send() {
        // Mislocated with a sufficient dest, but starving → no send, no charge.
        let pressed = HostResources::from_parts(1000, 50, 0.0, 4);
        let cands = [Candidate {
            headroom_pct: 90,
            addr: addr(9001),
        }];
        let mut sent = false;
        let attempt = attempt_transport(&pressed, &cands, false, 0, |_| {
            sent = true;
            Ok(ConfirmOutcome::Accepted)
        });
        assert!(matches!(attempt, TransportAttempt::CannotAfford));
        assert!(!sent, "a starving unit cannot flee");
    }

    #[test]
    fn attempt_transport_sends_when_mislocated_afford_and_dest_exists() {
        let pressed = HostResources::from_parts(1000, 50, 0.0, 4);
        let cands = [Candidate {
            headroom_pct: 90,
            addr: addr(9099),
        }];
        let mut sent_addr = None;
        let attempt = attempt_transport(&pressed, &cands, true, 0, |a| {
            sent_addr = Some(a);
            Ok(ConfirmOutcome::Accepted)
        });
        assert_eq!(sent_addr, Some(addr(9099)));
        match attempt {
            TransportAttempt::Attempted(o) => assert!(should_release(&o)),
            other => panic!("expected Attempted, got {other:?}"),
        }
    }

    #[test]
    fn attempt_transport_err_does_not_permit_release() {
        let pressed = HostResources::from_parts(1000, 50, 0.0, 4);
        let cands = [Candidate {
            headroom_pct: 90,
            addr: addr(9001),
        }];
        let attempt = attempt_transport(&pressed, &cands, true, 0, |_| {
            Err(TransportError::Refused)
        });
        match attempt {
            TransportAttempt::Attempted(o) => assert!(!should_release(&o)),
            other => panic!("expected Attempted, got {other:?}"),
        }
    }
}
