//! The command panel's highlight (`web/command.mjs`, tests ported from
//! `test/command.test.js`, same cases and names). It belongs to an item, by
//! its key, not to a position in the list: groups that arrive later (the
//! core's matches) leave it on the item it was on.

/// The key highlighted among `keys`, the items listed in order: `active`
/// (the key last highlighted) while it is listed, else the first.
pub fn highlighted<'a>(keys: &'a [String], active: Option<&str>) -> Option<&'a str> {
    keys.iter()
        .find(|k| Some(k.as_str()) == active)
        .or_else(|| keys.first())
        .map(String::as_str)
}

/// The key `step` items on from the highlighted one, wrapping round.
pub fn stepped<'a>(keys: &'a [String], active: Option<&str>, step: isize) -> Option<&'a str> {
    let current = highlighted(keys, active)?;
    let index = keys.iter().position(|k| k == current).expect("listed") as isize;
    let count = keys.len() as isize;
    Some(keys[(index + step).rem_euclid(count) as usize].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn the_highlight_stays_on_its_item_when_groups_arrive_in_front_of_it() {
        let local = keys(&["new-chat", "new-bench", "settings"]);
        // Nothing chosen yet: the first item.
        let first = highlighted(&local, None);
        assert_eq!(first, Some("new-chat"));
        // The core's builds and benches land in front: the same item, not
        // the same position.
        let all = keys(&[
            "build:1",
            "build:2",
            "bench:1",
            "new-chat",
            "new-bench",
            "settings",
        ]);
        assert_eq!(highlighted(&all, first), Some("new-chat"));
        // An item that left the list gives way to the first.
        assert_eq!(highlighted(&all, Some("prompt:gone")), Some("build:1"));
        assert_eq!(highlighted(&[], Some("new-chat")), None);
    }

    #[test]
    fn the_arrows_step_from_the_highlighted_item_and_wrap_round() {
        let k = keys(&["build:1", "new-chat", "settings"]);
        assert_eq!(stepped(&k, Some("new-chat"), 1), Some("settings"));
        assert_eq!(stepped(&k, Some("new-chat"), -1), Some("build:1"));
        assert_eq!(stepped(&k, Some("settings"), 1), Some("build:1"));
        assert_eq!(stepped(&k, Some("build:1"), -1), Some("settings"));
        // From no choice, or one no longer listed, the step is from the first.
        assert_eq!(stepped(&k, None, 1), Some("new-chat"));
        assert_eq!(stepped(&k, Some("gone"), -1), Some("settings"));
    }
}
