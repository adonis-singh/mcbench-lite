"""Text: every paragraph, heading, code block and cell on screen read through
AT-SPI's Text interface by character, word and line, with each one's
character and range extents checked against its own extents; then the
composer's text, caret and selection as real keys change them, and its
caret and selection set through the Text interface."""

import time

import common

Atspi = common.Atspi
report = common.Report("text")
common.app()
time.sleep(2.0)
common.focus_window()
# Bring the first message and the long answer in: Shift+Tab to the current
# message, Home, then Down twice.
common.key("shift+Tab")
common.key("Home", settle=0.5)
common.key("Down", settle=0.3)
common.key("Down", settle=0.6)
app = common.app()

TEXT_ROLES = {"paragraph", "heading", "static", "table cell", "column header"}
nodes = common.find(app, pred=lambda o: o.get_role_name() in TEXT_ROLES and "Text" in o.get_interfaces())
report.check("text nodes found", len(nodes) >= 15, f"{len(nodes)} nodes with the Text interface")
by_role = {}
for node in nodes:
    by_role[node.get_role_name()] = by_role.get(node.get_role_name(), 0) + 1
report.check("every kind of text node", TEXT_ROLES <= set(by_role), str(by_role))


def inside(inner, outer, slack=2):
    return (
        inner.x >= outer.x - slack
        and inner.y >= outer.y - slack
        and inner.x + inner.width <= outer.x + outer.width + slack
        and inner.y + inner.height <= outer.y + outer.height + slack
    )


for node in nodes:
    count = Atspi.Text.get_character_count(node)
    text = Atspi.Text.get_text(node, 0, count)
    name = f"{node.get_role_name()} {text[:30]!r}"
    if count == 0:
        report.check(f"{name} has text", False)
        continue
    # By character: every character read back joins into the whole text.
    chars = "".join(
        Atspi.Text.get_string_at_offset(node, i, Atspi.TextGranularity.CHAR).content for i in range(count)
    )
    report.check(f"{name}: by character", chars == text, f"{len(chars)} of {count}")
    word = Atspi.Text.get_string_at_offset(node, 0, Atspi.TextGranularity.WORD)
    first_word = text.split()[0] if text.split() else ""
    report.check(
        f"{name}: first word",
        word.content.strip().startswith(first_word.strip(".,:;")[:3]),
        f"{word.content!r} [{word.start_offset}, {word.end_offset})",
    )
    line = Atspi.Text.get_string_at_offset(node, 0, Atspi.TextGranularity.LINE)
    report.check(
        f"{name}: first line",
        len(line.content) > 0 and text.startswith(line.content.rstrip("\n")),
        f"{line.content[:40]!r}",
    )
    own = node.get_extents(Atspi.CoordType.WINDOW)
    first = Atspi.Text.get_character_extents(node, 0, Atspi.CoordType.WINDOW)
    whole = Atspi.Text.get_range_extents(node, 0, count, Atspi.CoordType.WINDOW)
    report.check(
        f"{name}: character extents",
        first.width > 0 and first.height > 0 and inside(first, own),
        f"char 0 {first.x},{first.y} {first.width}x{first.height} in {own.x},{own.y} {own.width}x{own.height}",
    )
    report.check(
        f"{name}: range extents",
        whole.width > 0 and whole.height > 0 and inside(whole, own),
        f"{whole.x},{whole.y} {whole.width}x{whole.height}",
    )
    at = Atspi.Text.get_offset_at_point(node, first.x + 1, first.y + first.height // 2, Atspi.CoordType.WINDOW)
    report.check(f"{name}: offset at the first character's point", at == 0, f"{at}")

# The table: its Table interface and its cells' TableCell interface.
table = common.one(app, "table")
report.check("the table offers Table", "Table" in table.get_interfaces(), str(table.get_interfaces()))
rows, columns = Atspi.Table.get_n_rows(table), Atspi.Table.get_n_columns(table)
report.check("rows and columns", rows >= 2 and columns >= 2, f"{rows} x {columns}")
cell_text = lambda c: Atspi.Text.get_text(c, 0, Atspi.Text.get_character_count(c))
corner = Atspi.Table.get_accessible_at(table, 0, 0)
report.check("the cell at row 0, column 0 is a column header", corner.get_role_name() == "column header", corner.get_role_name())
cell = Atspi.Table.get_accessible_at(table, 1, 1)
report.check("the cell at row 1, column 1 offers TableCell", "TableCell" in cell.get_interfaces(), str(cell.get_interfaces()))
position = Atspi.TableCell.get_position(cell)
report.check("its position is row 1, column 1", tuple(position)[-2:] == (1, 1), str(position))
span = Atspi.TableCell.get_row_column_span(cell)
report.check("its span", tuple(span)[-4:] == (1, 1, 1, 1), str(span))
headers = Atspi.TableCell.get_column_header_cells(cell)
header = Atspi.Table.get_column_header(table, 1)
report.check(
    "its column header is the header over column 1",
    len(headers) == 1 and cell_text(headers[0]) == cell_text(header),
    f"{[cell_text(h) for h in headers]} / {cell_text(header)}",
)
report.check("the cell's table is the table", Atspi.TableCell.get_table(cell).get_role_name() == "table")
index = Atspi.Table.get_index_at(table, 1, 1)
report.check(
    "index and back",
    (Atspi.Table.get_row_at_index(table, index), Atspi.Table.get_column_at_index(table, index)) == (1, 1),
    str(index),
)
report.check("cell text", cell_text(cell) != "", repr(cell_text(cell)))

# The composer: real typing, then the caret and a selection.
common.key("End", settle=0.3)
common.key("Tab", settle=0.4)
composer = common.one(common.app(), "entry", "Message")
report.check("the composer has focus", composer.get_state_set().contains(Atspi.StateType.FOCUSED))
common.type_text("hello world")
count = Atspi.Text.get_character_count(composer)
report.check("typed text", Atspi.Text.get_text(composer, 0, count) == "hello world", Atspi.Text.get_text(composer, 0, count))
report.check("caret after typing", Atspi.Text.get_caret_offset(composer) == 11, str(Atspi.Text.get_caret_offset(composer)))
common.key("Left")
report.check("caret after Left", Atspi.Text.get_caret_offset(composer) == 10, str(Atspi.Text.get_caret_offset(composer)))
Atspi.Text.set_caret_offset(composer, 5)
time.sleep(0.4)
report.check("caret set through AT-SPI", Atspi.Text.get_caret_offset(composer) == 5, str(Atspi.Text.get_caret_offset(composer)))
Atspi.Text.set_selection(composer, 0, 0, 5) if Atspi.Text.get_n_selections(composer) else Atspi.Text.add_selection(composer, 0, 5)
time.sleep(0.4)
selections = [
    (Atspi.Text.get_selection(composer, i).start_offset, Atspi.Text.get_selection(composer, i).end_offset)
    for i in range(Atspi.Text.get_n_selections(composer))
]
report.check("selection set through AT-SPI", selections == [(0, 5)], str(selections))
extent = Atspi.Text.get_character_extents(composer, 0, Atspi.CoordType.WINDOW)
report.check("composer character extents", extent.width > 0 and extent.height > 0, f"{extent.x},{extent.y} {extent.width}x{extent.height}")
report.check("composer offers EditableText", "EditableText" in composer.get_interfaces(), str(composer.get_interfaces()))
Atspi.EditableText.set_text_contents(composer, "set through AT-SPI")
time.sleep(0.5)
count = Atspi.Text.get_character_count(composer)
report.check("text set through EditableText", Atspi.Text.get_text(composer, 0, count) == "set through AT-SPI", Atspi.Text.get_text(composer, 0, count))
common.screenshot("text.png")
common.quit_key(settle=1.5)
report.finish()
