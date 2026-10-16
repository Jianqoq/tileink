use super::*;

#[test]
fn holes_split_and_coalesce_without_moving_live_list_ids() {
    let mut lists = vec![CommandList::default()];
    let mut storage = CommandStorage::default();
    let a = storage.allocate(&mut lists, 2);
    let b = storage.allocate(&mut lists, 3);
    let live = storage.allocate(&mut lists, 1);
    lists[live.start].commands.push(Command::Draw(42));
    storage.release(&mut lists, b);
    storage.release(&mut lists, a);
    assert_eq!(storage.free.len(), 1);
    assert_eq!(storage.free[0], 1..6);
    let reused = storage.allocate(&mut lists, 4);
    assert_eq!(reused, 1..5);
    assert_eq!(storage.free.len(), 1);
    assert_eq!(storage.free[0], 5..6);
    assert!(matches!(
        lists[live.start].commands.as_slice(),
        [Command::Draw(42)]
    ));
    storage.release(&mut lists, reused);
    storage.release(&mut lists, live);
    assert_eq!(lists.len(), 1);
    assert!(storage.free.is_empty());
}

#[test]
fn releasing_an_interior_range_drops_commands_before_reuse() {
    let mut lists = vec![CommandList::default()];
    let mut storage = CommandStorage::default();
    let released = storage.allocate(&mut lists, 1);
    storage.allocate(&mut lists, 1);
    lists[released.start].commands.push(Command::Draw(1));
    storage.release(&mut lists, released.clone());
    assert!(lists[released.start].commands.is_empty());
    assert_eq!(storage.allocate(&mut lists, 1), released);
    assert!(storage.free.is_empty());
}

#[test]
fn releasing_the_middle_of_two_holes_merges_both_sides() {
    let mut lists = vec![CommandList::default()];
    let mut storage = CommandStorage::default();
    let a = storage.allocate(&mut lists, 1);
    let b = storage.allocate(&mut lists, 1);
    let c = storage.allocate(&mut lists, 1);
    storage.allocate(&mut lists, 1);
    storage.release(&mut lists, a);
    storage.release(&mut lists, c);
    storage.release(&mut lists, b);
    assert_eq!(storage.free.len(), 1);
    assert_eq!(storage.free[0], 1..4);
    assert_eq!(storage.allocate(&mut lists, 3), 1..4);
}
