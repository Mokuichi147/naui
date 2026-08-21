//! ツリーの値型。
//!
//! ツリーは「入れ子になった項目 + 展開状態 + いま選ばれている項目」という
//! 構造を持つ。リスト ([`crate::ListItem`]) と形は似ているが、
//!
//! - 項目が子を持ち、深さが決まっていない
//! - 枝を開いたり閉じたりできる (見えている行が変わる)
//!
//! という違いがあるため、別の型として持つ。バックエンドはこれを
//! `NSOutlineView` の項目や `<li role="treeitem">` へ写す。
//!
//! 項目の識別は**根からの子インデックスの並び (パス)** で行う。
//! `[0, 2]` は「1 番目の根の 3 番目の子」を指す。空のパスはどの項目も
//! 指さないので、「選択なし」を表すのに使う。

/// ツリーの 1 項目。
///
/// ```
/// # use naui_core::TreeItem;
/// let tree = TreeItem::new("src")
///     .expanded(true)
///     .children([TreeItem::new("main.rs").detail("120 行"), TreeItem::new("lib.rs")]);
/// assert_eq!(TreeItem::at(&[tree], &[0, 1]).map(|i| i.label.as_str()), Some("lib.rs"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeItem {
    /// 画面に出る文字列。
    pub label: String,
    /// 補助の文字列。指定すると 2 行目に小さく出る。
    ///
    /// **Web だけは 1 行に収まる。** 行の高さをそろえるため、
    /// `ラベル — 補助` の形で同じ行に続けて出る。
    pub detail: Option<String>,
    /// 選べるかどうか。`false` にすると、**その子孫もまとめて選べなくなる**
    /// ([`TreeItem::selectable`])。
    pub enabled: bool,
    /// 最初に開いた状態で出すかどうか。
    ///
    /// これが効くのは `Tree::set_items` を呼んだ時点だけで、
    /// その後の開閉はウィジェット側が持つ。
    pub expanded: bool,
    /// 子の項目。空なら葉。
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

    /// 2 行目に出す補助の文字列を指定する。
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// 選べるかどうかを指定する (既定は選べる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 最初から開いた状態で出すかどうかを指定する (既定は閉じている)。
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// 子の項目を差し替える。
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

    /// 子の項目を 1 つ足す。
    pub fn child(mut self, child: impl Into<TreeItem>) -> Self {
        self.children.push(child.into());
        self
    }

    /// 子を持たない (開けない) 項目かどうか。
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// 文字列の並びから、子を持たない項目の列を作る。
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

    /// パスの指す項目を返す。無ければ `None`。
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("親").child(TreeItem::new("子"))];
    /// assert_eq!(TreeItem::at(&items, &[0, 0]).unwrap().label, "子");
    /// assert!(TreeItem::at(&items, &[0, 9]).is_none());
    /// // 空のパスはどの項目も指さない。
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

    /// 子孫まで数えた項目の総数。
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("親").children(TreeItem::list(["子", "子"]))];
    /// assert_eq!(TreeItem::count(&items), 3);
    /// ```
    pub fn count(items: &[TreeItem]) -> usize {
        items
            .iter()
            .map(|item| 1 + TreeItem::count(&item.children))
            .sum()
    }

    /// そのパスの項目を選べるかどうか。
    ///
    /// 選べるのは、**その項目と、根までのすべての祖先**が
    /// [`TreeItem::enabled`] のときだけ。枝を無効にすると、その中身も
    /// まとめて選べなくなる (どのバックエンドでも同じ)。
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("親").enabled(false).child(TreeItem::new("子"))];
    /// assert!(!TreeItem::selectable(&items, &[0]));
    /// assert!(!TreeItem::selectable(&items, &[0, 0]), "祖先が無効なら子も選べない");
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

    /// 木を上から順 (深さ優先・親が先) にたどる。
    ///
    /// 親が子より先に来るので、そのまま「祖先から順に開く」処理に使える。
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("親").child(TreeItem::new("子")), TreeItem::new("隣")];
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

    /// 展開状態にしたがって、いま見えている項目のパスを上から順に返す。
    ///
    /// 根の項目は常に見えている。子が見えるのは、親が開いているときだけ。
    ///
    /// ```
    /// # use naui_core::TreeItem;
    /// let items = vec![TreeItem::new("親").child(TreeItem::new("子"))];
    /// // どこも開いていなければ、根だけが見える。
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
