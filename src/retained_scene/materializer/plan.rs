//! Retained command installation, plan fragments and painter metadata.
mod commands;
mod compiled;
mod fragments;
mod membership;
mod offscreen;
mod painter;
mod roots;

use super::helpers::*;
use super::*;
use crate::shared::execution::ROOT_COMMAND_LIST_ID;

#[cfg(test)]
#[path = "plan/contracts.rs"]
mod tests;

#[cfg(test)]
mod root_contracts;
