//! Finding the document positions of DOM positions while parsing.

use super::context::ParseContext;
use crate::dom::{Dom, NodeKind};
use crate::error::Result;
use crate::text::Text;

impl<'p, 'o, D: Dom> ParseContext<'p, 'o, D> {
    /// The indexes of the positions to find that are still unfound.
    pub(super) fn unfound(&self) -> Vec<usize> {
        match &self.options.find_positions {
            Some(find) => (0..find.len())
                .filter(|&index| find[index].pos.is_none())
                .collect(),
            None => Vec::new(),
        }
    }

    pub(super) fn set_found(&mut self, index: usize, pos: usize) {
        if let Some(find) = &mut self.options.find_positions {
            find[index].pos = Some(pos);
        }
    }

    pub(super) fn find_at_point(&mut self, parent: &D::Node, offset: usize) -> Result<()> {
        let count = self
            .options
            .find_positions
            .as_ref()
            .map_or(0, |find| find.len());
        for index in 0..count {
            let target = &self.options.find_positions.as_ref().expect("positions")[index];
            if target.offset == offset && self.dom.same(&target.node, parent)? {
                let pos = self.current_pos()?;
                self.set_found(index, pos);
            }
        }
        Ok(())
    }

    pub(super) fn find_inside(&mut self, parent: &D::Node) -> Result<()> {
        for index in self.unfound() {
            let target = self.options.find_positions.as_ref().expect("positions")[index]
                .node
                .clone();
            if self.dom.kind(parent)? == NodeKind::Element && self.dom.contains(parent, &target)? {
                let pos = self.current_pos()?;
                self.set_found(index, pos);
            }
        }
        Ok(())
    }

    pub(super) fn find_around(
        &mut self,
        parent: &D::Node,
        content: &D::Node,
        before: bool,
    ) -> Result<()> {
        if self.options.find_positions.is_none() || self.dom.same(parent, content)? {
            return Ok(());
        }
        for index in self.unfound() {
            let target = self.options.find_positions.as_ref().expect("positions")[index]
                .node
                .clone();
            if self.dom.kind(parent)? == NodeKind::Element && self.dom.contains(parent, &target)? {
                let position = self.dom.compare_document_position(content, &target)?;
                if position & if before { 2 } else { 4 } != 0 {
                    let pos = self.current_pos()?;
                    self.set_found(index, pos);
                }
            }
        }
        Ok(())
    }

    pub(super) fn find_in_text(&mut self, node: &D::Node, text: &Text) -> Result<()> {
        let count = self
            .options
            .find_positions
            .as_ref()
            .map_or(0, |find| find.len());
        for index in 0..count {
            let target = &self.options.find_positions.as_ref().expect("positions")[index];
            if self.dom.same(&target.node, node)? {
                let offset = target.offset;
                let pos = (self.current_pos()? + offset).saturating_sub(text.len());
                self.set_found(index, pos);
            }
        }
        Ok(())
    }
}
