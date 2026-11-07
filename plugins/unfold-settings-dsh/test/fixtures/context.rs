// A stand-in for the agent's context.rs: only the fold bounds the settings panel reads.

/// Turns kept in the model context.
pub const MAX_TURNS: usize = 128;
/// Bytes of the whole fold.
pub const MAX_CONTEXT_BYTES: usize = 32 * 1024;
/// Bytes of one turn.
pub const MAX_TURN_BYTES: usize = 4 * 1024;
