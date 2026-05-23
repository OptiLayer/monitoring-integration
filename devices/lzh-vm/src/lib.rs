//! LZH/Optotech vacuum-machine protocol codec and state machine.
//!
//! Wire format reverse-engineered from real customer pcaps; see
//! `tests/fixtures/protocol-notes.md` for the verified layout.

pub mod frame;
pub mod state;
pub mod transport;

pub use frame::{FRAME_LEN, FrameError, InFrame, OutFrame};
pub use state::{CoatingPlan, LayerSpec, LzhSession, Measurement, State};
pub use transport::{LzhTransport, SharedSession, TransportConfig};
