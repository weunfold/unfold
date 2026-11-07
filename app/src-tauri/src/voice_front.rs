
#[path = "front_child.rs"]
pub mod child;
#[path = "front_context.rs"]
pub mod context;
#[path = "front_http.rs"]
pub mod http;
#[path = "front_marker.rs"]
pub mod marker;
#[path = "front_policy.rs"]
pub mod policy;
#[path = "front_link.rs"]
pub mod link;
#[path = "front_runtime.rs"]
pub mod runtime;
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Admission,
    Assets,
    Child,
    Closed,
    Busy,
    Cancelled,
    Deadline,
    Http,
    Stream,
    Context,
    Prefill,
    Persistence,
}
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(feature = "voice_pane")]
#[path = "front_render.rs"]
pub mod render;

#[cfg(feature = "voice_pane")]
#[path = "front_capture.rs"]
pub mod capture;

#[cfg(feature = "voice_pane")]
#[path = "front_worker.rs"]
pub mod worker;
