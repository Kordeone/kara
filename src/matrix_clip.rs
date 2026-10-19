// SPDX-License-Identifier: MPL-2.0
//! Clip native drop hit boxes to a scroll viewport. The locked scrollable
//! translates child destinations but does not clip them. All other widget
//! behavior, including accessibility and keyboard operations, is delegated.
use cosmic::{
    Element, Renderer, Theme,
    iced::{
        self, Length, Rectangle, Size, Vector,
        advanced::{
            self, Widget, layout, mouse, overlay, renderer,
            widget::{Operation, Tree, tree},
        },
    },
};
use iced::advanced::clipboard::DndDestinationRectangles;

pub fn clip<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Clip(content.into()))
}
struct Clip<'a, M>(Element<'a, M>);
fn intersection(
    rect: &iced::clipboard::dnd::Rectangle,
    bounds: Rectangle,
) -> Option<iced::clipboard::dnd::Rectangle> {
    let x = rect.x.max(f64::from(bounds.x));
    let y = rect.y.max(f64::from(bounds.y));
    let right = (rect.x + rect.width).min(f64::from(bounds.x + bounds.width));
    let bottom = (rect.y + rect.height).min(f64::from(bounds.y + bounds.height));
    (right > x && bottom > y).then_some(iced::clipboard::dnd::Rectangle {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}
impl<M> Widget<M, Theme, Renderer> for Clip<'_, M> {
    fn size(&self) -> Size<Length> {
        self.0.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.0.as_widget().size_hint()
    }
    fn tag(&self) -> tree::Tag {
        self.0.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.0.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.0.as_widget().children()
    }
    fn diff(&mut self, tree: &mut Tree) {
        self.0.as_widget_mut().diff(tree);
    }
    fn id(&self) -> Option<iced::widget::Id> {
        self.0.as_widget().id()
    }
    fn set_id(&mut self, id: iced::widget::Id) {
        self.0.as_widget_mut().set_id(id);
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.0.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: layout::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.0
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: layout::Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn advanced::Clipboard,
        shell: &mut advanced::Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.0.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: layout::Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.0
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: layout::Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.0
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: layout::Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, M, Theme, Renderer>> {
        self.0
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
    fn a11y_nodes(
        &self,
        layout: layout::Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.0.as_widget().a11y_nodes(layout, tree, cursor)
    }
    fn drag_destinations(
        &self,
        tree: &Tree,
        layout: layout::Layout<'_>,
        renderer: &Renderer,
        rectangles: &mut DndDestinationRectangles,
    ) {
        let mut children = DndDestinationRectangles::new();
        self.0
            .as_widget()
            .drag_destinations(tree, layout, renderer, &mut children);
        // Preserve registration order; into_rectangles reverses it for the backend.
        for child in children.as_ref() {
            if let Some(rectangle) = intersection(&child.rectangle, layout.bounds()) {
                let mut child = child.clone();
                child.rectangle = rectangle;
                rectangles.push(child);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scrolled_out_targets_are_rejected_and_partial_targets_are_clipped() {
        let viewport = Rectangle {
            x: 100.,
            y: 100.,
            width: 200.,
            height: 300.,
        };
        let rect = |y, height| iced::clipboard::dnd::Rectangle {
            x: 110.,
            y,
            width: 100.,
            height,
        };
        assert!(intersection(&rect(10., 80.), viewport).is_none());
        assert!(intersection(&rect(400., 50.), viewport).is_none());
        let clipped = intersection(&rect(80., 40.), viewport).unwrap();
        assert_eq!((clipped.y, clipped.height), (100., 20.));
        let clipped = intersection(&rect(380., 40.), viewport).unwrap();
        assert_eq!((clipped.y, clipped.height), (380., 20.));
    }
}
