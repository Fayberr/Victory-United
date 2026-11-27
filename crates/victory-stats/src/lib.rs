//! Live statistics block: the engine writes it once per frame, the launcher reads it once per second.
//!
//! Contract (see docs/launcher-contract.md): a named shared-memory block called
//! `Local\VictoryUnited.Stats.<pid of the engine process>`, fixed layout, versioned header.
//! Owned by the "ops" worker. This file is a placeholder until that worker fills it in.
