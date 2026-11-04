//! The split tree of one tab: which panes it shows, where, and the separator lines between them.

use ratatui::layout::Rect;

use crate::protocol::{Direction, PaneId, SplitDir};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Leaf(PaneId),
    /// `Right` puts `b` to the right of `a`; `Down` puts `b` below `a`. `ratio` is `a`'s share.
    Split {
        dir: SplitDir,
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

/// A separator line between the two sides of a split.
#[derive(Debug, Clone, PartialEq)]
pub struct Separator {
    /// One cell wide (vertical line) or one cell tall (horizontal line).
    pub rect: Rect,
    pub vertical: bool,
    /// Path from the root to the split (false = a, true = b), for dragging.
    pub path: Vec<bool>,
    /// The area the split divides, for turning a drag position into a ratio.
    pub split_area: Rect,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub panes: Vec<(PaneId, Rect)>,
    pub separators: Vec<Separator>,
}

impl Layout {
    pub fn rect_of(&self, id: PaneId) -> Option<Rect> {
        self.panes.iter().find(|(p, _)| *p == id).map(|(_, r)| *r)
    }

    pub fn pane_at(&self, x: u16, y: u16) -> Option<(PaneId, Rect)> {
        self.panes.iter().copied().find(|(_, r)| contains(*r, x, y))
    }

    pub fn separator_at(&self, x: u16, y: u16) -> Option<&Separator> {
        self.separators.iter().find(|s| contains(s.rect, x, y))
    }
}

pub fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// The space a split leaves between its sides, and the smallest side, with and without pane frames.
fn spacing(frames: bool) -> (u16, u16) {
    if frames { (0, 3) } else { (1, 1) }
}

/// Splits `area` for a split node: (a, handle, b). Without frames the handle is the 1-cell separator
/// line; with frames the sides touch and the handle is the two frame lines where they meet. Each side
/// keeps its minimum size when it can.
pub fn split_area(area: Rect, dir: SplitDir, ratio: f32, frames: bool) -> (Rect, Option<Rect>, Rect) {
    let (gap, min) = spacing(frames);
    let total = match dir {
        SplitDir::Right => area.width,
        SplitDir::Down => area.height,
    };
    let avail = total.saturating_sub(gap);
    if avail < 2 * min {
        // Too small to split: `a` gets everything and `b` is empty.
        let empty = match dir {
            SplitDir::Right => Rect::new(area.x + area.width, area.y, 0, area.height),
            SplitDir::Down => Rect::new(area.x, area.y + area.height, area.width, 0),
        };
        return (area, None, empty);
    }
    let first = ((f32::from(avail) * ratio).round() as u16).clamp(min, avail - min);
    // With frames the handle starts on a's last line (its right or bottom frame) and covers b's first.
    let (handle_at, handle_len) = if frames { (first - 1, 2) } else { (first, 1) };
    match dir {
        SplitDir::Right => (
            Rect::new(area.x, area.y, first, area.height),
            Some(Rect::new(area.x + handle_at, area.y, handle_len, area.height)),
            Rect::new(area.x + first + gap, area.y, avail - first, area.height),
        ),
        SplitDir::Down => (
            Rect::new(area.x, area.y, area.width, first),
            Some(Rect::new(area.x, area.y + handle_at, area.width, handle_len)),
            Rect::new(area.x, area.y + first + gap, area.width, avail - first),
        ),
    }
}

/// Whether `rect` has room to be split in `dir`.
pub fn can_split(rect: Rect, dir: SplitDir, frames: bool) -> bool {
    let (gap, min) = spacing(frames);
    let total = match dir {
        SplitDir::Right => rect.width,
        SplitDir::Down => rect.height,
    };
    total >= 2 * min + gap
}

/// The split ratio that puts a dragged border at `pos`.
pub fn drag_ratio(pos: u16, area: Rect, vertical: bool, frames: bool) -> f32 {
    let (start, len) = if vertical { (area.x, area.width) } else { (area.y, area.height) };
    let offset = f32::from(pos.saturating_sub(start));
    if frames { (offset + 1.0) / f32::from(len.max(1)) } else { offset / f32::from(len.saturating_sub(1).max(1)) }
}

/// The inside of a pane frame.
pub fn inner(r: Rect) -> Rect {
    Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), r.height.saturating_sub(2))
}

impl Node {
    pub fn layout(&self, area: Rect, frames: bool) -> Layout {
        let mut out = Layout::default();
        self.layout_into(area, frames, &mut Vec::new(), &mut out);
        out
    }

    fn layout_into(&self, area: Rect, frames: bool, path: &mut Vec<bool>, out: &mut Layout) {
        match self {
            Node::Leaf(id) => out.panes.push((*id, area)),
            Node::Split { dir, ratio, a, b } => {
                let (ra, sep, rb) = split_area(area, *dir, *ratio, frames);
                if let Some(rect) = sep {
                    out.separators.push(Separator {
                        rect,
                        vertical: *dir == SplitDir::Right,
                        path: path.clone(),
                        split_area: area,
                    });
                }
                path.push(false);
                a.layout_into(ra, frames, path, out);
                path.pop();
                path.push(true);
                b.layout_into(rb, frames, path, out);
                path.pop();
            }
        }
    }

    /// Pane ids in reading order (left to right, top to bottom).
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn contains(&self, id: PaneId) -> bool {
        match self {
            Node::Leaf(p) => *p == id,
            Node::Split { a, b, .. } => a.contains(id) || b.contains(id),
        }
    }

    /// Replaces the leaf `target` with a split holding `target` and `new`. Returns false if not found.
    pub fn split(&mut self, target: PaneId, new: PaneId, dir: SplitDir) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                *self = Node::Split { dir, ratio: 0.5, a: Box::new(Node::Leaf(target)), b: Box::new(Node::Leaf(new)) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split(target, new, dir) || b.split(target, new, dir),
        }
    }

    /// Removes the leaf `target`; its sibling takes the split's place. A lone root leaf can't be removed
    /// (the caller closes the tab instead).
    pub fn remove(&mut self, target: PaneId) -> bool {
        let Node::Split { a, b, .. } = self else { return false };
        let a_is = matches!(**a, Node::Leaf(id) if id == target);
        let b_is = matches!(**b, Node::Leaf(id) if id == target);
        if a_is || b_is {
            let keep = if a_is {
                std::mem::replace(&mut **b, Node::Leaf(0))
            } else {
                std::mem::replace(&mut **a, Node::Leaf(0))
            };
            *self = keep;
            return true;
        }
        a.remove(target) || b.remove(target)
    }

    fn at_path_mut(&mut self, path: &[bool]) -> Option<&mut Node> {
        match (path.split_first(), self) {
            (None, node) => Some(node),
            (Some((side, rest)), Node::Split { a, b, .. }) => {
                if *side {
                    b.at_path_mut(rest)
                } else {
                    a.at_path_mut(rest)
                }
            }
            _ => None,
        }
    }

    /// Sets the ratio of the split at `path` (from a border drag).
    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) {
        if let Some(Node::Split { ratio: r, .. }) = self.at_path_mut(path) {
            *r = ratio.clamp(0.02, 0.98);
        }
    }

    /// Moves the nearest border on `dir`'s axis around `target` by `cells`, in `dir`'s direction.
    pub fn move_border(&mut self, target: PaneId, dir: Direction, cells: u16, area: Rect, frames: bool) -> bool {
        let axis = match dir {
            Direction::Left | Direction::Right => SplitDir::Right,
            Direction::Up | Direction::Down => SplitDir::Down,
        };
        let sign = if matches!(dir, Direction::Right | Direction::Down) { 1.0 } else { -1.0 };
        // Walk down to the target, remembering the deepest split on the right axis.
        let mut best: Option<(Vec<bool>, Rect)> = None;
        let mut path = Vec::new();
        let mut node: &Node = self;
        let mut node_area = area;
        loop {
            match node {
                Node::Leaf(_) => break,
                Node::Split { dir: d, ratio, a, b } => {
                    if *d == axis {
                        best = Some((path.clone(), node_area));
                    }
                    let (ra, _, rb) = split_area(node_area, *d, *ratio, frames);
                    if a.contains(target) {
                        path.push(false);
                        node = a;
                        node_area = ra;
                    } else if b.contains(target) {
                        path.push(true);
                        node = b;
                        node_area = rb;
                    } else {
                        return false;
                    }
                }
            }
        }
        let Some((split_path, split_area)) = best else { return false };
        let total = match axis {
            SplitDir::Right => split_area.width,
            SplitDir::Down => split_area.height,
        };
        let avail = f32::from(total.saturating_sub(spacing(frames).0).max(1));
        if let Some(Node::Split { ratio, .. }) = self.at_path_mut(&split_path) {
            *ratio = (*ratio + sign * f32::from(cells) / avail).clamp(0.02, 0.98);
            return true;
        }
        false
    }

    /// Moves one side of `target`: `dir` names the side, and `grow` moves it outwards (the pane gets
    /// bigger) or inwards. Returns false if that side is the edge of the tab.
    pub fn resize_side(
        &mut self,
        target: PaneId,
        dir: Direction,
        grow: bool,
        cells: u16,
        area: Rect,
        frames: bool,
    ) -> bool {
        let axis = match dir {
            Direction::Left | Direction::Right => SplitDir::Right,
            Direction::Up | Direction::Down => SplitDir::Down,
        };
        // The left (top) side is a border where the target is in the split's second half.
        let in_b_wanted = matches!(dir, Direction::Left | Direction::Up);
        let mut best: Option<(Vec<bool>, Rect)> = None;
        let mut path = Vec::new();
        let mut node: &Node = self;
        let mut node_area = area;
        while let Node::Split { dir: d, ratio, a, b } = node {
            let in_b = b.contains(target);
            if !in_b && !a.contains(target) {
                return false;
            }
            if *d == axis && in_b == in_b_wanted {
                best = Some((path.clone(), node_area));
            }
            let (ra, _, rb) = split_area(node_area, *d, *ratio, frames);
            path.push(in_b);
            (node, node_area) = if in_b { (&**b, rb) } else { (&**a, ra) };
        }
        if !node.contains(target) {
            return false;
        }
        let Some((split_path, split_area)) = best else { return false };
        let total = match axis {
            SplitDir::Right => split_area.width,
            SplitDir::Down => split_area.height,
        };
        let avail = f32::from(total.saturating_sub(spacing(frames).0).max(1));
        // Growing the left side moves the border left (a smaller ratio); growing the right side moves it right.
        let sign = if in_b_wanted == grow { -1.0 } else { 1.0 };
        if let Some(Node::Split { ratio, .. }) = self.at_path_mut(&split_path) {
            *ratio = (*ratio + sign * f32::from(cells) / avail).clamp(0.02, 0.98);
            return true;
        }
        false
    }

    /// Swaps the places of panes `x` and `y`.
    pub fn swap(&mut self, x: PaneId, y: PaneId) {
        match self {
            Node::Leaf(id) if *id == x => *id = y,
            Node::Leaf(id) if *id == y => *id = x,
            Node::Leaf(_) => {}
            Node::Split { a, b, .. } => {
                a.swap(x, y);
                b.swap(x, y);
            }
        }
    }

    /// Puts pane `new` where `old` is. Returns false if `old` isn't here.
    pub fn replace(&mut self, old: PaneId, new: PaneId) -> bool {
        match self {
            Node::Leaf(id) if *id == old => {
                *id = new;
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.replace(old, new) || b.replace(old, new),
        }
    }
}

/// The pane next to `from` in direction `dir`: the closest one that overlaps it on the other axis.
pub fn neighbor(panes: &[(PaneId, Rect)], from: PaneId, dir: Direction) -> Option<PaneId> {
    let cur = panes.iter().find(|p| p.0 == from)?.1;
    let overlap =
        |a0: u16, alen: u16, b0: u16, blen: u16| i32::from((a0 + alen).min(b0 + blen)) - i32::from(a0.max(b0));
    panes
        .iter()
        .filter(|(id, r)| *id != from && r.width > 0 && r.height > 0)
        .filter_map(|(id, r)| {
            let (dist, over) = match dir {
                Direction::Left => {
                    (i32::from(cur.x) - i32::from(r.x + r.width), overlap(cur.y, cur.height, r.y, r.height))
                }
                Direction::Right => {
                    (i32::from(r.x) - i32::from(cur.x + cur.width), overlap(cur.y, cur.height, r.y, r.height))
                }
                Direction::Up => {
                    (i32::from(cur.y) - i32::from(r.y + r.height), overlap(cur.x, cur.width, r.x, r.width))
                }
                Direction::Down => {
                    (i32::from(r.y) - i32::from(cur.y + cur.height), overlap(cur.x, cur.width, r.x, r.width))
                }
            };
            (dist >= 0 && over > 0).then_some((dist, -over, *id))
        })
        .min()
        .map(|(_, _, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 1, 81, 20)
    }

    #[test]
    fn split_right_then_down() {
        let mut root = Node::Leaf(1);
        assert!(root.split(1, 2, SplitDir::Right));
        assert!(root.split(2, 3, SplitDir::Down));
        assert_eq!(root.panes(), vec![1, 2, 3]);
        let l = root.layout(area(), false);
        assert_eq!(l.rect_of(1), Some(Rect::new(0, 1, 40, 20)));
        assert_eq!(l.rect_of(2), Some(Rect::new(41, 1, 40, 10)));
        assert_eq!(l.rect_of(3), Some(Rect::new(41, 12, 40, 9)));
        assert_eq!(l.separators.len(), 2);
        assert!(l.separators[0].vertical);
        assert_eq!(l.separators[0].rect, Rect::new(40, 1, 1, 20));
    }

    #[test]
    fn remove_promotes_the_sibling() {
        let mut root = Node::Leaf(1);
        root.split(1, 2, SplitDir::Right);
        root.split(2, 3, SplitDir::Down);
        assert!(root.remove(2));
        assert_eq!(root.panes(), vec![1, 3]);
        assert!(root.remove(1));
        assert_eq!(root, Node::Leaf(3));
        assert!(!root.remove(3), "a lone root leaf stays");
    }

    #[test]
    fn neighbors_by_direction() {
        let mut root = Node::Leaf(1);
        root.split(1, 2, SplitDir::Right);
        root.split(2, 3, SplitDir::Down);
        let l = root.layout(area(), false);
        assert_eq!(neighbor(&l.panes, 1, Direction::Right), Some(2));
        assert_eq!(neighbor(&l.panes, 3, Direction::Left), Some(1));
        assert_eq!(neighbor(&l.panes, 2, Direction::Down), Some(3));
        assert_eq!(neighbor(&l.panes, 3, Direction::Up), Some(2));
        assert_eq!(neighbor(&l.panes, 1, Direction::Left), None);
    }

    #[test]
    fn move_border_and_drag() {
        let mut root = Node::Leaf(1);
        root.split(1, 2, SplitDir::Right);
        assert!(root.move_border(2, Direction::Right, 8, area(), false));
        assert_eq!(root.layout(area(), false).rect_of(1).unwrap().width, 48);
        assert!(!root.move_border(2, Direction::Up, 1, area(), false), "no horizontal border");
        root.set_ratio(&[], 0.25);
        assert_eq!(root.layout(area(), false).rect_of(1).unwrap().width, 20);
    }

    #[test]
    fn tiny_areas_do_not_panic() {
        let mut root = Node::Leaf(1);
        root.split(1, 2, SplitDir::Right);
        let l = root.layout(Rect::new(0, 0, 2, 1), false);
        assert_eq!(l.rect_of(1).unwrap().width, 2);
        assert_eq!(l.rect_of(2).unwrap().width, 0);
        let l = root.layout(Rect::new(0, 0, 5, 5), true);
        assert_eq!(l.rect_of(2).unwrap().width, 0, "frames need 3 cells a side");
        assert!(!can_split(Rect::new(0, 0, 5, 5), SplitDir::Right, true));
        assert!(can_split(Rect::new(0, 0, 6, 5), SplitDir::Right, true));
    }

    #[test]
    fn frames_touch_and_have_a_two_cell_handle() {
        let mut root = Node::Leaf(1);
        root.split(1, 2, SplitDir::Right);
        let l = root.layout(Rect::new(0, 1, 80, 20), true);
        assert_eq!(l.rect_of(1), Some(Rect::new(0, 1, 40, 20)));
        assert_eq!(l.rect_of(2), Some(Rect::new(40, 1, 40, 20)));
        assert_eq!(l.separators[0].rect, Rect::new(39, 1, 2, 20));
        assert_eq!(inner(Rect::new(40, 1, 40, 20)), Rect::new(41, 2, 38, 18));
        // Dragging the handle to column 59 makes the left pane 60 wide.
        root.set_ratio(&[], drag_ratio(59, Rect::new(0, 1, 80, 20), true, true));
        assert_eq!(root.layout(Rect::new(0, 1, 80, 20), true).rect_of(1).unwrap().width, 60);
    }

    #[test]
    fn resize_one_side_swap_and_replace() {
        let a = Rect::new(0, 0, 100, 40);
        let mut root = Node::Leaf(1);
        root.split(1, 2, SplitDir::Right);
        root.split(2, 3, SplitDir::Down);
        // Pane 2's left side grows: the vertical border moves left.
        assert!(root.resize_side(2, Direction::Left, true, 10, a, true));
        assert_eq!(root.layout(a, true).rect_of(1).unwrap().width, 40);
        // Shrinking it moves the border back.
        assert!(root.resize_side(2, Direction::Left, false, 10, a, true));
        assert_eq!(root.layout(a, true).rect_of(1).unwrap().width, 50);
        // Pane 2's bottom side grows: pane 3 gets shorter.
        assert!(root.resize_side(2, Direction::Down, true, 4, a, true));
        assert_eq!(root.layout(a, true).rect_of(2).unwrap().height, 24);
        // Pane 1 has no left side to move.
        assert!(!root.resize_side(1, Direction::Left, true, 4, a, true));
        root.swap(1, 3);
        assert_eq!(root.panes(), vec![3, 2, 1]);
        assert!(root.replace(2, 9));
        assert_eq!(root.panes(), vec![3, 9, 1]);
    }
}
