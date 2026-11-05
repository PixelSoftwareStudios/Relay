//! Relay core: a sans-IO, deterministic domain model.
//!
//! [`State`] is the single owner of engine state. It consumes [`Input`]s with
//! an explicit [`MonoTime`] and returns [`Outcome`]s carrying [`Effect`]s for
//! adapters to perform. It never reads clocks, sleeps, or touches sockets or
//! audio buffers, so every trace is replayable in tests.

pub mod call;
pub mod channel;
pub mod command;
pub mod effect;
pub mod engine;
pub mod error;
pub mod event;
pub mod gate;
pub mod id;
pub mod limits;
pub mod outcome;
pub mod queue;
pub mod testing;
pub mod time;

mod ops;
mod state;

pub use call::{
	Call, CallControls, CallState, Direction, EndReason, Gain, RouteMode, TransmitRoute,
};
pub use command::{Command, Input, PeerEvent, PttUpdate, Request, SessionEvent, SystemEvent};
pub use effect::{Effect, GateSnapshot, PeerMessage, ReceiveMix};
pub use engine::{Engine, Submitted};
pub use error::{LimitsError, QueueFull, Reject};
pub use event::{
	CallView, CaptureState, Cursor, Event, GlobalView, InputFault, Revisioned, Snapshot, Watch,
};
pub use gate::{DenyReason, GateDecision, GateInputs, evaluate};
pub use id::*;
pub use limits::Limits;
pub use ops::OperationRecord;
pub use outcome::{OpResult, Outcome, Reply};
pub use state::State;
pub use time::{Clock, MonoTime};
