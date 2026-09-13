//! Shared filter preparation preserves child-first filter order, backdrop-first
//! order, and content-before-mask order. Adapters upload the resulting payloads.

pub(crate) mod cursors;
pub(crate) mod paths;
pub(crate) mod tables;

#[cfg(test)]
mod tests;
