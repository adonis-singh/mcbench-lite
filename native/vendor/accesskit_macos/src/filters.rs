// Copyright 2023 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::Role;
use accesskit_consumer::{FilterResult, Node, common_filter};

/// mcbench patch: the common filter, except that a text run is kept when its
/// parent also holds something other than text runs (a paragraph with a link
/// in it), or when its parent is a table cell or a code block. VoiceOver then reads the
/// paragraph's own text around the link, and a cell's text, as static text;
/// otherwise it hears only the link, or "cell column 1, row 1" with no text.
pub(crate) fn filter(node: &Node) -> FilterResult {
    if node.role() == Role::TextRun
        && !node.is_hidden()
        && node.parent().is_some_and(|parent| {
            matches!(
                parent.role(),
                Role::Cell | Role::GridCell | Role::ColumnHeader | Role::RowHeader | Role::Code
            ) || parent.children().any(|child| child.role() != Role::TextRun)
        })
    {
        return FilterResult::Include;
    }
    common_filter(node)
}
