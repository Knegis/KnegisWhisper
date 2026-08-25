//! Choosing a loopback port for a sidecar.
//!
//! Two rules, both load-bearing:
//!
//! 1. **Loopback only.** Probing binds `127.0.0.1`, never `0.0.0.0`. A sidecar
//!    is a local model server holding audio and transcripts; a bind to all
//!    interfaces would expose it to the network with no authentication at all.
//!    The brief forbids it and so does this module — there is no parameter to
//!    pass a different address.
//! 2. **A range, not a port.** A wedged predecessor still holding the socket,
//!    or a second copy of the app, must not make the feature unavailable.
//!
//! Probing a port and then handing it to a child is inherently racy: something
//! else can take it in between. That race is not worth closing here (the socket
//! cannot be passed to these child processes), so it is handled where it
//! surfaces — the child fails to bind, and the caller allocates again. What
//! this module guarantees is only that the port was free a moment ago, which is
//! enough to make repeated collisions vanishingly unlikely rather than
//! impossible.

use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};

use crate::manifest::PortRange;

/// The only address a sidecar is ever probed or bound on.
pub const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

/// Why no port could be allocated.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PortError {
    #[error(
        "every port from {first} to {last} is in use. Another copy of this application, or a \
         sidecar from a previous run that has not exited, is holding them."
    )]
    RangeExhausted { first: u16, last: u16 },

    #[error("port range {first}-{last} is empty or reversed")]
    InvalidRange { first: u16, last: u16 },
}

/// Anything that can say whether a loopback port is free.
///
/// A trait so the allocator can be tested without depending on which ports
/// happen to be free on the machine running the tests.
pub trait PortProbe {
    /// True when a listener could be bound on `127.0.0.1:port` right now.
    fn is_free(&self, port: u16) -> bool;
}

/// Probes by actually binding a listener and dropping it.
#[derive(Debug, Clone, Copy, Default)]
pub struct BindProbe;

impl PortProbe for BindProbe {
    fn is_free(&self, port: u16) -> bool {
        // Bound and immediately dropped. SO_REUSEADDR is deliberately not set:
        // the question is whether a child could bind this port normally, and
        // enabling reuse would answer a different, more permissive question.
        TcpListener::bind(SocketAddrV4::new(LOOPBACK, port)).is_ok()
    }
}

/// The lowest free port in the range.
///
/// Lowest rather than random so that a running sidecar is predictable across
/// restarts, which makes a stale process easier to recognise.
pub fn allocate<P: PortProbe>(range: PortRange, probe: &P) -> Result<u16, PortError> {
    if !range.is_valid() {
        return Err(PortError::InvalidRange {
            first: range.first,
            last: range.last,
        });
    }

    range
        .ports()
        .find(|port| probe.is_free(*port))
        .ok_or(PortError::RangeExhausted {
            first: range.first,
            last: range.last,
        })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    struct Taken(BTreeSet<u16>);

    impl PortProbe for Taken {
        fn is_free(&self, port: u16) -> bool {
            !self.0.contains(&port)
        }
    }

    fn range(first: u16, last: u16) -> PortRange {
        PortRange { first, last }
    }

    #[test]
    fn the_lowest_free_port_is_chosen() {
        let probe = Taken(BTreeSet::new());
        assert_eq!(allocate(range(8178, 8199), &probe).unwrap(), 8178);
    }

    #[test]
    fn taken_ports_are_skipped_in_order() {
        let probe = Taken([8178, 8179, 8180].into_iter().collect());
        assert_eq!(allocate(range(8178, 8199), &probe).unwrap(), 8181);
    }

    #[test]
    fn the_last_port_in_the_range_is_usable() {
        // An off-by-one here would waste a port and, on a one-port range, make
        // the feature permanently unavailable.
        let probe = Taken((8178..8199).collect());
        assert_eq!(allocate(range(8178, 8199), &probe).unwrap(), 8199);
    }

    #[test]
    fn an_exhausted_range_names_the_range_and_the_likely_cause() {
        let probe = Taken((8178..=8199).collect());
        let err = allocate(range(8178, 8199), &probe).unwrap_err();
        assert_eq!(
            err,
            PortError::RangeExhausted {
                first: 8178,
                last: 8199
            }
        );
        assert!(err.to_string().contains("has not exited"));
    }

    #[test]
    fn a_reversed_range_is_an_error_rather_than_an_empty_search() {
        // Iterating 8199..=8178 yields nothing, which would otherwise be
        // reported as "every port is in use" -- true, but useless.
        let probe = Taken(BTreeSet::new());
        assert_eq!(
            allocate(range(8199, 8178), &probe).unwrap_err(),
            PortError::InvalidRange {
                first: 8199,
                last: 8178
            }
        );
    }

    #[test]
    fn port_zero_is_refused() {
        // Binding port 0 means "any port", so a range starting at 0 would
        // always succeed while telling the child to pick its own port.
        let probe = Taken(BTreeSet::new());
        assert!(allocate(range(0, 0), &probe).is_err());
    }

    #[test]
    fn a_single_port_range_works() {
        let probe = Taken(BTreeSet::new());
        assert_eq!(allocate(range(8178, 8178), &probe).unwrap(), 8178);
    }

    #[test]
    fn the_real_probe_reports_a_bound_port_as_taken() {
        // Binds an ephemeral port so the test does not depend on which fixed
        // ports are free on this machine.
        let listener = TcpListener::bind(SocketAddrV4::new(LOOPBACK, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        assert!(!BindProbe.is_free(port), "a bound port must read as taken");
        drop(listener);
        assert!(
            BindProbe.is_free(port),
            "and free again once the listener is gone"
        );
    }

    #[test]
    fn the_real_probe_only_ever_looks_at_loopback() {
        // Not a behavioural test -- a guard on the constant, because a sidecar
        // bound to 0.0.0.0 would be an unauthenticated model server on the
        // network.
        assert_eq!(LOOPBACK, Ipv4Addr::new(127, 0, 0, 1));
        assert!(LOOPBACK.is_loopback());
    }
}
