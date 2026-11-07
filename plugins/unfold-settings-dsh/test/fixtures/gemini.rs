// A stand-in for the agent's gemini.rs: only the shapes the settings panel reads (a policy
// constant tagged `[<name>-voice-policy <id>]`). Synthetic text; the real one is the agent's.

/// The default voice policy.
pub const POLICY_PTBR_ADDRESSEE_V1: &str = "[unfold-voice-policy ptbr-addressee-v1]
You are Unfold, a voice assistant. Answer in Brazilian Portuguese, briefly, and only when \
someone is talking to you: a name said in passing is not \"being addressed\".";

/// Not a policy: no tag.
pub const GREETING: &str = "hello";
