//! Stable command-list ranges with explicit subtree ownership.
//!
//! Incremental recompilation must release its previous tree. Appending replacements left
//! unreachable lists alive forever; periodic full rebuilding would only hide that leak.
use super::*;
use std::ops::Range;

/// Free ranges stay sorted and coalesced. Live list IDs never move, so unrelated retained
/// command locations and immutable Canvas snapshots keep their meaning.
#[derive(Default)]
pub(super) struct CommandStorage {
    free: Vec<Range<usize>>,
}

impl CommandStorage {
    pub(super) fn reset(&mut self) {
        self.free.clear();
    }

    fn allocate(&mut self, lists: &mut Vec<CommandList>, count: usize) -> Range<usize> {
        debug_assert_ne!(count, 0);
        if let Some(index) = self.free.iter().position(|range| range.len() >= count) {
            let start = self.free[index].start;
            self.free[index].start += count;
            if self.free[index].is_empty() {
                self.free.remove(index);
            }
            return start..start + count;
        }
        let start = lists.len();
        lists.resize_with(start + count, CommandList::default);
        start..start + count
    }

    fn release(&mut self, lists: &mut Vec<CommandList>, mut range: Range<usize>) {
        for list in &mut lists[range.clone()] {
            list.commands.clear();
        }
        let mut index = self.free.partition_point(|free| free.start < range.start);
        if index > 0 && self.free[index - 1].end == range.start {
            index -= 1;
            range.start = self.free.remove(index).start;
        }
        if index < self.free.len() && self.free[index].start == range.end {
            range.end = self.free.remove(index).end;
        }
        if range.end == lists.len() {
            lists.truncate(range.start);
        } else {
            self.free.insert(index, range);
        }
    }
}

impl PersistentSceneMaterializer {
    pub(super) fn allocate_command_lists(&mut self, count: usize) -> Range<usize> {
        self.command_storage
            .allocate(&mut Rc::make_mut(&mut self.canvas).command_lists, count)
    }

    pub(super) fn release_command_lists(&mut self, range: Range<usize>) {
        self.command_storage
            .release(&mut Rc::make_mut(&mut self.canvas).command_lists, range);
    }

    /// Follow the published command's ownership, not current chunks or node locations: a
    /// transaction may already have removed those nodes or moved them to another fragment.
    pub(super) fn release_command(&mut self, command: Command) {
        match command {
            Command::MaterializedRetainedScene { fragment, .. } => {
                self.release_command_lists(fragment);
            }
            Command::Layer { children, .. } => self.release_command_branch(children),
            Command::MaskLayer { content, mask, .. } => {
                self.release_command_branch(content);
                self.release_command_branch(mask);
            }
            Command::Draw(_) => unreachable!("retained containers own scene or layer commands"),
        }
    }

    pub(super) fn release_command_branch(&mut self, list: usize) {
        let mut commands =
            std::mem::take(&mut Rc::make_mut(&mut self.canvas).command_lists[list].commands);
        while let Some(command) = commands.pop() {
            self.release_command(command);
        }
        Rc::make_mut(&mut self.canvas).command_lists[list].commands = commands;
        self.release_command_lists(list..list + 1);
    }
}

#[cfg(test)]
mod tests;
