// Copyright 2026 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

// mcbench patch: the AT-SPI Table and TableCell interfaces. Selection of
// rows and columns is not offered (no table in the app selects cells).

use accesskit_atspi_common::{NodeId, PlatformNode};
use zbus::{fdo, interface, names::OwnedUniqueName};

use crate::atspi::{ObjectId, OwnedObjectAddress};

pub(crate) struct TableInterface {
    bus_name: OwnedUniqueName,
    node: PlatformNode,
}

impl TableInterface {
    pub fn new(bus_name: OwnedUniqueName, node: PlatformNode) -> Self {
        Self { bus_name, node }
    }

    fn map_error(&self) -> impl '_ + FnOnce(accesskit_atspi_common::Error) -> fdo::Error {
        |error| crate::util::map_error_from_node(&self.node, error)
    }

    fn address(&self, node: Option<NodeId>) -> (OwnedObjectAddress,) {
        let object = node.map(|node| ObjectId::Node {
            adapter: self.node.adapter_id(),
            node,
        });
        super::optional_object_address(&self.bus_name, object)
    }
}

#[interface(name = "org.a11y.atspi.Table")]
impl TableInterface {
    #[zbus(property)]
    fn n_rows(&self) -> fdo::Result<i32> {
        self.node.table_n_rows().map_err(self.map_error())
    }

    #[zbus(property)]
    fn n_columns(&self) -> fdo::Result<i32> {
        self.node.table_n_columns().map_err(self.map_error())
    }

    #[zbus(property)]
    fn caption(&self) -> OwnedObjectAddress {
        OwnedObjectAddress::null()
    }

    #[zbus(property)]
    fn summary(&self) -> OwnedObjectAddress {
        OwnedObjectAddress::null()
    }

    #[zbus(property)]
    fn n_selected_rows(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn n_selected_columns(&self) -> i32 {
        0
    }

    fn get_accessible_at(&self, row: i32, column: i32) -> fdo::Result<(OwnedObjectAddress,)> {
        let node = self
            .node
            .table_accessible_at(row, column)
            .map_err(self.map_error())?;
        Ok(self.address(node))
    }

    fn get_index_at(&self, row: i32, column: i32) -> fdo::Result<i32> {
        self.node
            .table_index_at(row, column)
            .map_err(self.map_error())
    }

    fn get_row_at_index(&self, index: i32) -> fdo::Result<i32> {
        self.node
            .table_row_at_index(index)
            .map_err(self.map_error())
    }

    fn get_column_at_index(&self, index: i32) -> fdo::Result<i32> {
        self.node
            .table_column_at_index(index)
            .map_err(self.map_error())
    }

    fn get_row_description(&self, _row: i32) -> String {
        String::new()
    }

    fn get_column_description(&self, _column: i32) -> String {
        String::new()
    }

    fn get_row_extent_at(&self, row: i32, column: i32) -> fdo::Result<i32> {
        Ok(self
            .node
            .table_extents_at(row, column)
            .map_err(self.map_error())?
            .0)
    }

    fn get_column_extent_at(&self, row: i32, column: i32) -> fdo::Result<i32> {
        Ok(self
            .node
            .table_extents_at(row, column)
            .map_err(self.map_error())?
            .1)
    }

    fn get_row_header(&self, row: i32) -> fdo::Result<(OwnedObjectAddress,)> {
        let node = self
            .node
            .table_header(false, row)
            .map_err(self.map_error())?;
        Ok(self.address(node))
    }

    fn get_column_header(&self, column: i32) -> fdo::Result<(OwnedObjectAddress,)> {
        let node = self
            .node
            .table_header(true, column)
            .map_err(self.map_error())?;
        Ok(self.address(node))
    }

    fn get_selected_rows(&self) -> Vec<i32> {
        Vec::new()
    }

    fn get_selected_columns(&self) -> Vec<i32> {
        Vec::new()
    }

    fn is_row_selected(&self, _row: i32) -> bool {
        false
    }

    fn is_column_selected(&self, _column: i32) -> bool {
        false
    }

    fn is_selected(&self, _row: i32, _column: i32) -> bool {
        false
    }

    fn add_row_selection(&self, _row: i32) -> bool {
        false
    }

    fn add_column_selection(&self, _column: i32) -> bool {
        false
    }

    fn remove_row_selection(&self, _row: i32) -> bool {
        false
    }

    fn remove_column_selection(&self, _column: i32) -> bool {
        false
    }

    /// (is a cell there, row, column, row span, column span, selected)
    fn get_row_column_extents_at_index(
        &self,
        index: i32,
    ) -> fdo::Result<(bool, i32, i32, i32, i32, bool)> {
        let row = self
            .node
            .table_row_at_index(index)
            .map_err(self.map_error())?;
        let column = self
            .node
            .table_column_at_index(index)
            .map_err(self.map_error())?;
        if row < 0 {
            return Ok((false, -1, -1, 0, 0, false));
        }
        let (rows, columns) = self
            .node
            .table_extents_at(row, column)
            .map_err(self.map_error())?;
        Ok((true, row, column, rows, columns, false))
    }
}

pub(crate) struct TableCellInterface {
    bus_name: OwnedUniqueName,
    node: PlatformNode,
}

impl TableCellInterface {
    pub fn new(bus_name: OwnedUniqueName, node: PlatformNode) -> Self {
        Self { bus_name, node }
    }

    fn map_error(&self) -> impl '_ + FnOnce(accesskit_atspi_common::Error) -> fdo::Error {
        |error| crate::util::map_error_from_node(&self.node, error)
    }

    fn addresses(&self, nodes: Vec<NodeId>) -> Vec<OwnedObjectAddress> {
        nodes
            .into_iter()
            .map(|node| {
                ObjectId::Node {
                    adapter: self.node.adapter_id(),
                    node,
                }
                .to_address(self.bus_name.inner())
            })
            .collect()
    }
}

#[interface(name = "org.a11y.atspi.TableCell")]
impl TableCellInterface {
    #[zbus(property)]
    fn column_span(&self) -> fdo::Result<i32> {
        Ok(self
            .node
            .cell_row_column_span()
            .map_err(self.map_error())?
            .3)
    }

    #[zbus(property)]
    fn row_span(&self) -> fdo::Result<i32> {
        Ok(self
            .node
            .cell_row_column_span()
            .map_err(self.map_error())?
            .2)
    }

    #[zbus(property)]
    fn position(&self) -> fdo::Result<(i32, i32)> {
        let (row, column, _, _) = self.node.cell_row_column_span().map_err(self.map_error())?;
        Ok((row, column))
    }

    #[zbus(property)]
    fn table(&self) -> fdo::Result<OwnedObjectAddress> {
        let table = self.node.cell_table().map_err(self.map_error())?;
        Ok(ObjectId::Node {
            adapter: self.node.adapter_id(),
            node: table,
        }
        .to_address(self.bus_name.inner()))
    }

    fn get_column_header_cells(&self) -> fdo::Result<Vec<OwnedObjectAddress>> {
        let cells = self
            .node
            .cell_header_cells(true)
            .map_err(self.map_error())?;
        Ok(self.addresses(cells))
    }

    fn get_row_header_cells(&self) -> fdo::Result<Vec<OwnedObjectAddress>> {
        let cells = self
            .node
            .cell_header_cells(false)
            .map_err(self.map_error())?;
        Ok(self.addresses(cells))
    }

    /// (row, column, row span, column span)
    fn get_row_column_span(&self) -> fdo::Result<(i32, i32, i32, i32)> {
        self.node.cell_row_column_span().map_err(self.map_error())
    }
}
