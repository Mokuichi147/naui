//! Tree value types.
//!
//! A tree has the structure "nested items + expansion state + the items currently
//! selected". It resembles a list ([`crate::ListItem`]) in shape, but differs in
//! that
//!
//! - items have children, with no fixed depth
//! - branches can be expanded and collapsed (which changes the visible rows)
//!
//! so it is a separate type. Backends map it onto `NSOutlineView` items or
//! `<li role="treeitem">`.
//!
//! Items are identified by **the sequence of child indices from the root (a
//! path)**. `[0, 2]` means "the third child of the first root". An empty path
//! points at no item, so it is used to mean "no selection".

/// One tree item.
///
/// ```
/// # use naui_core::TreeItem;
/// let tree = TreeItem::new("src")
///     .expanded(true)
///     .children([TreeItem::new("main.rs").detail("120 lines"), TreeItem::new("lib.rs")]);
/// assert_eq!(TreeItem::at(&[tree], &[0, 1]).map(|i| i.label.as_str()), Some("lib.rs"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeItem {
    /// The text shown on screen.
    pub label: String,
    /// Secondary text. When set, it appears small on a second line.
    ///
    /// **Only on the Web does it fit on one line.** To keep row heights uniform, it
    /// follows on the same row in the form `label — detail`.
    pub detail: Option<String>,
    /// Whether it can be selected. Setting `false` **also makes all its descendants
    /// unselectable** ([`TreeItem::selectable`]).
    pub enabled: bool,
    /// Whether it is initially shown expanded.
    ///
    /// This takes effect only when `Tree::set_items` is called; after that, the widget
    /// owns the expansion state.
    pub expanded: bool,
    /// The child items. A leaf if empty.
    pub children: Vec<TreeItem>,
}

impl TreeItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: None,
            enabled: true,
            expanded: false,
            children: Vec::new(),
        }
    }

    /// Sets the secondary text shown on the second line.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets whether it can be selected (selectable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets whether it is shown expanded from the start (collapsed by default).
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Replaces the child items.
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let item = TreeItem::new("docs").children(TreeItem::list(["README.md", "LICENSE"]));
    /// assert_eq!(item.children.len(), 2);
    /// ```
    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<TreeItem>,
    {
        self.children = children.into_iter().map(Into::into).collect();
        self
    }

    /// Adds one child item.
    pub fn child(mut self, child: impl Into<TreeItem>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Whether the item has no children (cannot be expanded).
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Creates a list of childless items from a sequence of strings.
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let leaves = TreeItem::list(["a.rs", "b.rs"]);
    /// assert!(leaves.iter().all(|item| item.is_leaf()));
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<TreeItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(TreeItem::new).collect()
    }

    /// Returns the item the path points at. `None` if there is none.
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("parent").child(TreeItem::new("child"))];
    /// assert_eq!(TreeItem::at(&items, &[0, 0]).unwrap().label, "child");
    /// assert!(TreeItem::at(&items, &[0, 9]).is_none());
    /// // An empty path points at no item.
    /// assert!(TreeItem::at(&items, &[]).is_none());
    /// ```
    pub fn at<'a>(items: &'a [TreeItem], path: &[usize]) -> Option<&'a TreeItem> {
        let (&first, rest) = path.split_first()?;
        let item = items.get(first)?;
        match rest.is_empty() {
            true => Some(item),
            false => TreeItem::at(&item.children, rest),
        }
    }

    /// The total number of items, descendants included.
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("parent").children(TreeItem::list(["child", "child"]))];
    /// assert_eq!(TreeItem::count(&items), 3);
    /// ```
    pub fn count(items: &[TreeItem]) -> usize {
        items
            .iter()
            .map(|item| 1 + TreeItem::count(&item.children))
            .sum()
    }

    /// Whether the item at the path can be selected.
    ///
    /// It can be selected only when **the item and all its ancestors up to the root**
    /// are [`TreeItem::enabled`]. Disabling a branch makes its contents unselectable
    /// too (the same on every backend).
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("parent").enabled(false).child(TreeItem::new("child"))];
    /// assert!(!TreeItem::selectable(&items, &[0]));
    /// assert!(!TreeItem::selectable(&items, &[0, 0]), "a disabled ancestor makes the child unselectable");
    /// ```
    pub fn selectable(items: &[TreeItem], path: &[usize]) -> bool {
        let Some((&first, rest)) = path.split_first() else {
            return false;
        };
        match items.get(first) {
            Some(item) if item.enabled => {
                rest.is_empty() || TreeItem::selectable(&item.children, rest)
            }
            _ => false,
        }
    }

    /// Walks the tree from the top (depth-first, parents first).
    ///
    /// Parents come before their children, so it can be used directly for "expand
    /// starting from the ancestors".
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("parent").child(TreeItem::new("child")), TreeItem::new("sibling")];
    /// let mut seen = Vec::new();
    /// TreeItem::walk(&items, |path, item| seen.push((path.to_vec(), item.label.clone())));
    /// assert_eq!(seen[0].0, vec![0]);
    /// assert_eq!(seen[1].0, vec![0, 0]);
    /// assert_eq!(seen[2].0, vec![1]);
    /// ```
    pub fn walk(items: &[TreeItem], mut f: impl FnMut(&[usize], &TreeItem)) {
        let mut path = Vec::new();
        walk_inner(items, &mut path, &mut f);
    }

    /// Returns the paths of the currently visible items from the top, according to the
    /// expansion state.
    ///
    /// Root items are always visible. Children are visible only when their parent is
    /// expanded.
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("parent").child(TreeItem::new("child"))];
    /// // With nothing expanded, only the roots are visible.
    /// assert_eq!(TreeItem::visible(&items, |_| false), vec![vec![0]]);
    /// assert_eq!(TreeItem::visible(&items, |_| true), vec![vec![0], vec![0, 0]]);
    /// ```
    pub fn visible(
        items: &[TreeItem],
        mut is_expanded: impl FnMut(&[usize]) -> bool,
    ) -> Vec<Vec<usize>> {
        let mut paths = Vec::new();
        let mut path = Vec::new();
        visible_inner(items, &mut path, &mut is_expanded, &mut paths);
        paths
    }
}

fn walk_inner(items: &[TreeItem], path: &mut Vec<usize>, f: &mut impl FnMut(&[usize], &TreeItem)) {
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        f(path, item);
        walk_inner(&item.children, path, f);
        path.pop();
    }
}

fn visible_inner(
    items: &[TreeItem],
    path: &mut Vec<usize>,
    is_expanded: &mut impl FnMut(&[usize]) -> bool,
    out: &mut Vec<Vec<usize>>,
) {
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        out.push(path.clone());
        if !item.is_leaf() && is_expanded(path) {
            visible_inner(&item.children, path, is_expanded, out);
        }
        path.pop();
    }
}

impl From<&str> for TreeItem {
    fn from(label: &str) -> Self {
        TreeItem::new(label)
    }
}

impl From<String> for TreeItem {
    fn from(label: String) -> Self {
        TreeItem::new(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<TreeItem> {
        vec![
            TreeItem::new("src")
                .expanded(true)
                .children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
            TreeItem::new("docs").child(TreeItem::new("README.md")),
            TreeItem::new("target")
                .enabled(false)
                .child(TreeItem::new("debug")),
        ]
    }

    #[test]
    fn tree_item_defaults() {
        let item = TreeItem::new("src");
        assert_eq!(item.label, "src");
        assert_eq!(item.detail, None);
        assert!(item.enabled);
        assert!(!item.expanded, "既定では閉じていること");
        assert!(item.is_leaf());
        assert_eq!(TreeItem::from("src"), item);
        assert_eq!(TreeItem::from(String::from("src")), item);
    }

    #[test]
    fn builders_keep_the_other_fields() {
        let item = TreeItem::new("src")
            .detail("3 ファイル")
            .enabled(false)
            .expanded(true)
            .child("main.rs");
        assert_eq!(item.detail.as_deref(), Some("3 ファイル"));
        assert!(!item.enabled);
        assert!(item.expanded);
        assert_eq!(item.children.len(), 1);
        assert!(!item.is_leaf());
    }

    #[test]
    fn at_walks_down_the_path() {
        let items = sample();
        assert_eq!(TreeItem::at(&items, &[0]).unwrap().label, "src");
        assert_eq!(TreeItem::at(&items, &[0, 1]).unwrap().label, "lib.rs");
        assert_eq!(TreeItem::at(&items, &[1, 0]).unwrap().label, "README.md");
        assert!(TreeItem::at(&items, &[]).is_none());
        assert!(TreeItem::at(&items, &[3]).is_none());
        // 葉の下は無い。
        assert!(TreeItem::at(&items, &[0, 0, 0]).is_none());
    }

    #[test]
    fn count_includes_descendants() {
        assert_eq!(TreeItem::count(&sample()), 7);
        assert_eq!(TreeItem::count(&[]), 0);
    }

    #[test]
    fn selectable_requires_every_ancestor() {
        let items = sample();
        assert!(TreeItem::selectable(&items, &[0]));
        assert!(TreeItem::selectable(&items, &[0, 0]));
        assert!(!TreeItem::selectable(&items, &[2]), "無効な枝は選べない");
        assert!(
            !TreeItem::selectable(&items, &[2, 0]),
            "無効な枝の中身も選べない"
        );
        assert!(!TreeItem::selectable(&items, &[]), "空のパスは何も指さない");
        assert!(!TreeItem::selectable(&items, &[9]));
    }

    #[test]
    fn walk_visits_parents_before_children() {
        let mut seen: Vec<Vec<usize>> = Vec::new();
        TreeItem::walk(&sample(), |path, _| seen.push(path.to_vec()));
        assert_eq!(
            seen,
            vec![
                vec![0],
                vec![0, 0],
                vec![0, 1],
                vec![1],
                vec![1, 0],
                vec![2],
                vec![2, 0],
            ]
        );
    }

    #[test]
    fn visible_follows_the_expansion() {
        let items = sample();
        // 何も開いていなければ、根の 3 つだけ。
        assert_eq!(
            TreeItem::visible(&items, |_| false),
            vec![vec![0], vec![1], vec![2]]
        );
        // 最初の枝だけを開く。
        assert_eq!(
            TreeItem::visible(&items, |path| path == [0]),
            vec![vec![0], vec![0, 0], vec![0, 1], vec![1], vec![2]]
        );
        // 閉じた親の中は、子が開いていても見えない。
        assert_eq!(
            TreeItem::visible(&items, |path| path == [1, 0]),
            vec![vec![0], vec![1], vec![2]]
        );
    }
}
