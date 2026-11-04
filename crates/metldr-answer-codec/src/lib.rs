#[cfg(any(feature = "guest", test))]
mod dom;
mod event;
mod program;
mod protocol;

#[cfg(feature = "guest")]
mod guest;

#[cfg(any(feature = "guest", test))]
pub use dom::{
    DomBatch, DomEventKind, DomMutation, DomMutationKind, DomSession, TaskEvent, TaskEventKind,
};
pub use event::{AnswerEvent, AnswerEventKind};

pub use program::encode_program;
pub use protocol::{
    AnswerFailureKind, Binding, ChildMessage, HostMessage, decode_child, decode_host,
    decode_initialize, encode_child, encode_host, encode_initialize,
};

#[cfg(any(feature = "guest", test))]
pub use program::GuestProgram;

#[cfg(feature = "guest")]
pub use guest::{GuestCommand, GuestCommandKind, GuestSession};
