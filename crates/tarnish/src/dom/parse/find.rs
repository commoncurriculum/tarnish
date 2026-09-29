//! Finding the document positions of DOM positions while parsing.

use super::FindPosition;
use super::context::ParseContext;
use crate::Result;
use crate::Text;
use crate::dom::Dom;

impl<'p, D: Dom> ParseContext<'p, D> {
    /// Set each position to find that `hit` picks, of those not found yet when `unfound_only`,
    /// to the current position moved by what `hit` gives.
    fn find_where(
        &mut self,
        unfound_only: bool,
        hit: impl FnMut(&FindPosition<D::Node>) -> Result<Option<isize>>,
    ) -> Result<()> {
        let Some(find) = self.options.find_positions.take() else {
            return Ok(());
        };
        let result = self.set_found(find, unfound_only, hit);
        self.options.find_positions = Some(find);
        result
    }

    fn set_found(
        &mut self,
        find: &mut [FindPosition<D::Node>],
        unfound_only: bool,
        mut hit: impl FnMut(&FindPosition<D::Node>) -> Result<Option<isize>>,
    ) -> Result<()> {
        let mut current = None;
        for target in find {
            if unfound_only && target.pos.is_some() {
                continue;
            }
            let Some(shift) = hit(target)? else { continue };
            // Counting the position closes the nodes above the open one, which JavaScript does
            // only on a hit.
            let pos = match current {
                Some(pos) => pos,
                None => *current.insert(self.current_pos()? as isize),
            };
            target.pos = Some(pos + shift);
        }
        Ok(())
    }

    pub(super) fn find_at_point(&mut self, parent: &D::Node, offset: usize) -> Result<()> {
        let dom = self.dom;
        self.find_where(false, |target| {
            Ok((target.offset == offset && dom.same(&target.node, parent)?).then_some(0))
        })
    }

    /// Find the positions inside an element at the current position. JavaScript checks that the
    /// node is an element, which every caller's is.
    pub(super) fn find_inside(&mut self, element: &D::Node) -> Result<()> {
        let dom = self.dom;
        self.find_where(true, |target| {
            Ok(dom.contains(element, &target.node)?.then_some(0))
        })
    }

    /// Find the positions inside an element that come before, or after, the element holding its
    /// content at the current position.
    pub(super) fn find_around(
        &mut self,
        element: &D::Node,
        content: &D::Node,
        before: bool,
    ) -> Result<()> {
        if self.options.find_positions.is_none() || self.dom.same(element, content)? {
            return Ok(());
        }
        let dom = self.dom;
        let side = if before { 2 } else { 4 };
        self.find_where(true, |target| {
            let hit = dom.contains(element, &target.node)?
                && dom.compare_document_position(content, &target.node)? & side != 0;
            Ok(hit.then_some(0))
        })
    }

    /// Find the positions in a text node just added, counting back from where it ends.
    pub(super) fn find_in_text(&mut self, node: &D::Node, text: &Text) -> Result<()> {
        let dom = self.dom;
        self.find_where(false, |target| {
            let shift = target.offset as isize - text.len() as isize;
            Ok(dom.same(&target.node, node)?.then_some(shift))
        })
    }
}
