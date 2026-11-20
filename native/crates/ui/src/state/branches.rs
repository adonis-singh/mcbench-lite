//! The branch arrows on a user message (`web/app.js` `branchNavigation`):
//! among the tree's `branchable` messages, siblings with the same parent,
//! sorted by position, give each its place and its neighbours when there
//! are at least two.

use std::collections::HashMap;

use crate::MessageTree;

/// A message's place among its versions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// Its index among its siblings, from 0.
    pub index: usize,
    /// How many versions there are.
    pub count: usize,
    /// The version before it.
    pub previous: Option<String>,
    /// The version after it.
    pub next: Option<String>,
}

/// Each branchable message's place among its versions, by id; a message
/// with no sibling has none.
pub fn branch_navigation(tree: &MessageTree) -> HashMap<String, Branch> {
    type Siblings<'a> = Vec<(i64, &'a str)>;
    let mut by_parent: Vec<(Option<&str>, Siblings)> = Vec::new();
    for message in tree.messages.iter().filter(|m| m.branchable) {
        let parent = message.parent_id.as_deref();
        match by_parent.iter_mut().find(|(p, _)| *p == parent) {
            Some((_, siblings)) => siblings.push((message.position, &message.id)),
            None => by_parent.push((parent, vec![(message.position, &message.id)])),
        }
    }
    let mut navigation = HashMap::new();
    for (_, mut siblings) in by_parent {
        if siblings.len() < 2 {
            continue;
        }
        siblings.sort_by_key(|(position, _)| *position);
        for (index, (_, id)) in siblings.iter().enumerate() {
            navigation.insert(
                id.to_string(),
                Branch {
                    index,
                    count: siblings.len(),
                    previous: index.checked_sub(1).map(|i| siblings[i].1.to_string()),
                    next: siblings.get(index + 1).map(|(_, id)| id.to_string()),
                },
            );
        }
    }
    navigation
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TreeMessage;

    fn message(id: &str, parent: Option<&str>, position: i64, branchable: bool) -> TreeMessage {
        TreeMessage {
            id: id.into(),
            parent_id: parent.map(str::to_string),
            position,
            role: Some(if branchable { "user" } else { "assistant" }.into()),
            branchable,
        }
    }

    #[test]
    fn siblings_of_a_parent_in_position_order_navigate_and_a_lone_message_does_not() {
        let tree = MessageTree {
            head_id: Some("a2".into()),
            messages: vec![
                message("u1", None, 0, true),
                message("a1", Some("u1"), 1, false),
                message("u3", Some("a1"), 6, true),
                message("u2", Some("a1"), 2, true),
                message("u4", Some("a1"), 4, true),
                message("a2", Some("u2"), 3, false),
            ],
        };
        let navigation = branch_navigation(&tree);
        assert_eq!(navigation.len(), 3);
        assert_eq!(
            navigation["u2"],
            Branch {
                index: 0,
                count: 3,
                previous: None,
                next: Some("u4".into())
            }
        );
        assert_eq!(navigation["u4"].previous.as_deref(), Some("u2"));
        assert_eq!(navigation["u3"].index, 2);
        assert!(!navigation.contains_key("u1"));
    }
}
