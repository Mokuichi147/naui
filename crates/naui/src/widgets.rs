use crate::sys;
use crate::Widget;
use naui_core::{Align, EventResponse, KeyEvent, Padding, TextColor, TextStyle};

/// 押しボタン (`GtkButton`)。
#[derive(Clone)]
pub struct Button(pub(crate) sys::Button);

impl_widget!(Button);

impl Button {
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 押されるたびに呼ばれる。呼び直すと以前のものは外れる。
    #[inline]
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        self.0.on_click(f)
    }
}

/// チェックボックス (`GtkCheckButton`)。
#[derive(Clone)]
pub struct Checkbox(pub(crate) sys::Checkbox);

impl_widget!(Checkbox);

impl Checkbox {
    #[inline]
    pub fn is_checked(&self) -> bool {
        self.0.is_checked()
    }

    /// プログラムから状態を変える。`on_toggle` は呼ばれない。
    #[inline]
    pub fn set_checked(&self, checked: bool) {
        self.0.set_checked(checked)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が切り替えるたびに、切り替え後の状態で呼ばれる。
    #[inline]
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle(f)
    }
}

/// 編集できないテキスト (`GtkLabel`)。
#[derive(Clone)]
pub struct Label(pub(crate) sys::Label);

impl_widget!(Label);

impl Label {
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    /// 文字を選んでコピーできるようにするか。既定は 選べない。
    ///
    /// チャットの発言やエラーの詳細のように、読む人が写し取りたい文字に使う。
    /// 入力欄と違い、文字は書き換えられない。
    #[inline]
    pub fn set_selectable(&self, selectable: bool) {
        self.0.set_selectable(selectable)
    }

    #[inline]
    pub fn is_selectable(&self) -> bool {
        self.0.is_selectable()
    }

    /// 長い文字列を折り返すかどうか。既定は折り返さない。
    ///
    /// 折り返さないときは 1 行のまま、入りきらない分を末尾の省略記号 (…) で
    /// 切る (`PangoEllipsizeMode` の `End`)。**省略記号を付けると `GtkLabel` の
    /// 最小幅も小さくなる**ので、狭いコンテナへ入れてもコンテナごと押し広げて
    /// しまうことがなくなる。
    #[inline]
    pub fn set_wrap(&self, wrap: bool) {
        self.0.set_wrap(wrap)
    }

    /// 文字の大きさと太さの段階。既定は [`TextStyle::Body`]。
    ///
    /// libadwaita のスタイルクラス (`.title-1` など) を当てるだけなので、
    /// 級数と太さを決めるのはテーマのほう。デスクトップのフォント設定にも
    /// そのまま追従する。
    #[inline]
    pub fn set_style(&self, style: TextStyle) {
        self.0.set_style(style)
    }

    /// 文字色の役割。既定は [`TextColor::Default`]。
    ///
    /// 色そのものはテーマが持つ (`.accent` はデスクトップのアクセントカラー)。
    /// ライト / ダークの切り替えにもテーマ側で追従する。
    #[inline]
    pub fn set_color(&self, color: TextColor) {
        self.0.set_color(color)
    }
}

/// パスワード入力 (`GtkPasswordEntry`)。
///
/// API の形は [`TextInput`] と同じで、違うのは**打った文字が伏せ字になる**
/// ことだけ。伏せ字を一時的に外すボタン (peek icon) は `GtkPasswordEntry` に
/// あるが、4 環境の共通部分に無いので出さない。
#[derive(Clone)]
pub struct PasswordInput(pub(crate) sys::PasswordInput);

impl_widget!(PasswordInput);

impl PasswordInput {
    /// キーが押されたときの通知。`Handled` を返すと、そのキーは
    /// 入力欄にもウィンドウにも渡らない (改行や確定を止められる)。
    ///
    /// IME で変換している間のキーは届かない。
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
    ///
    /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
    /// 自体がアクティブでなくなっても呼ばれない。
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    /// いま入力されている文字列。
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// Enter で確定したとき (`activate` シグナル) に、そのときの中身で呼ばれる。
    #[inline]
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit(f)
    }
}

/// 進捗バー (`GtkProgressBar`)。
#[derive(Clone)]
pub struct ProgressBar(pub(crate) sys::ProgressBar);

impl_widget!(ProgressBar);

impl ProgressBar {
    /// 0.0..=1.0。
    #[inline]
    pub fn set_value(&self, value: f64) {
        self.0.set_value(value)
    }

    #[inline]
    pub fn value(&self) -> f64 {
        self.0.value()
    }

    /// 進み具合が分からない処理中の表示 (不確定の進捗) にするか。
    ///
    /// `true` の間は値の代わりに動きで「処理中」を示す。戻すと
    /// [`set_value`](Self::set_value) で置いた値の表示に戻る (値は覚えている)。
    ///
    /// GTK4 の進捗バーは `pulse` を呼ぶたびに動くので、その間は一定の間隔で
    /// 呼び続ける。
    #[inline]
    pub fn set_indeterminate(&self, indeterminate: bool) {
        self.0.set_indeterminate(indeterminate)
    }

    #[inline]
    pub fn is_indeterminate(&self) -> bool {
        self.0.is_indeterminate()
    }
}

/// 検索の入力欄 (`GtkSearchEntry`)。
///
/// 虫めがねの印と、打ち始めると出る取り消しボタン (✕) は GTK が出す。
/// `on_change` は打つたび (`changed` シグナル)、`on_search` は Enter で
/// 確定したとき (`activate` シグナル) に呼ばれる。GtkSearchEntry には
/// 打鍵をまとめてから出す `search-changed` もあるが、待ち時間の分だけ
/// 他の環境とずれるので使わない。
#[derive(Clone)]
pub struct SearchInput(pub(crate) sys::SearchInput);

impl_widget!(SearchInput);

impl SearchInput {
    /// キーが押されたときの通知。`Handled` を返すと、そのキーは
    /// 入力欄にもウィンドウにも渡らない (改行や確定を止められる)。
    ///
    /// IME で変換している間のキーは届かない。
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
    ///
    /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
    /// 自体がアクティブでなくなっても呼ばれない。
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    /// いま入力されている文字列。
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// Enter で確定したときに、そのときの中身で呼ばれる。
    #[inline]
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_search(f)
    }
}

/// スライダー (`GtkScale`)。
#[derive(Clone)]
pub struct Slider(pub(crate) sys::Slider);

impl_widget!(Slider);

impl Slider {
    #[inline]
    pub fn value(&self) -> f64 {
        self.0.value()
    }

    /// プログラムから値を変える。`on_change` は呼ばれない。
    #[inline]
    pub fn set_value(&self, value: f64) {
        self.0.set_value(value)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// つまみが動くたびに、その値で呼ばれる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change(f)
    }
}

/// 縦 / 横に子を並べるコンテナ (`GtkBox`)。
#[derive(Clone)]
pub struct Stack(pub(crate) sys::Stack);

impl_widget!(Stack);

impl Stack {
    #[inline]
    pub fn set_spacing(&self, spacing: f64) {
        self.0.set_spacing(spacing)
    }

    #[inline]
    pub fn set_padding(&self, padding: Padding) {
        self.0.set_padding(padding)
    }

    /// 交差軸方向の寄せ方。既定は [`Align::Center`]。
    ///
    /// 交差軸に [`Length::Fill`](naui_core::Length::Fill) を指定した子は、
    /// 自分の指定を優先する。
    #[inline]
    pub fn set_align(&self, align: Align) {
        self.0.set_align(align)
    }

    #[inline]
    pub fn append(&self, child: &dyn Widget) {
        self.0.append(child.as_backend())
    }

    /// 指定した位置へ子を差し込む。`index` が今の数以上なら末尾へ足す。
    #[inline]
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        self.0.insert(index, child.as_backend())
    }

    /// 指定した位置の子を外す。範囲外のときは何もしない。
    #[inline]
    pub fn remove(&self, index: usize) {
        self.0.remove(index)
    }

    /// 子をすべて外す。
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 改行を含む文字列を入力できる欄 (`GtkTextView`)。
///
/// スクロールと同じく中身に合わせた高さを持たないので、
/// [`TextArea::set_sizing`] で指定しておく。
#[derive(Clone)]
pub struct TextArea(pub(crate) sys::TextArea);

impl_widget!(TextArea);

impl TextArea {
    /// キーが押されたときの通知。`Handled` を返すと、そのキーは
    /// 入力欄にもウィンドウにも渡らない (改行や確定を止められる)。
    ///
    /// IME で変換している間のキーは届かない。
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
    ///
    /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
    /// 自体がアクティブでなくなっても呼ばれない。
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// 読み取り専用にするか。既定は書き換えられる。
    ///
    /// 読み取り専用の間も文字は選んでコピーでき、フォーカスも受け取る
    /// (`set_enabled(false)` と違い、薄く表示されない)。ログや生成結果を
    /// 見せる欄に使う。
    #[inline]
    pub fn set_read_only(&self, read_only: bool) {
        self.0.set_read_only(read_only)
    }

    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.0.is_read_only()
    }
}

/// 1 行のテキスト入力 (`GtkEntry`)。
#[derive(Clone)]
pub struct TextInput(pub(crate) sys::TextInput);

impl_widget!(TextInput);

impl TextInput {
    /// キーが押されたときの通知。`Handled` を返すと、そのキーは
    /// 入力欄にもウィンドウにも渡らない (改行や確定を止められる)。
    ///
    /// IME で変換している間のキーは届かない。
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
    ///
    /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
    /// 自体がアクティブでなくなっても呼ばれない。
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// Enter で確定したとき (`activate` シグナル) に、そのときの中身で呼ばれる。
    ///
    /// IME で変換している間の Enter (変換の確定) では呼ばれない。読み取り
    /// 専用の間も呼ばれない。
    #[inline]
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit(f)
    }

    /// 読み取り専用にするか。既定は書き換えられる。
    ///
    /// 読み取り専用の間も文字は選んでコピーでき、フォーカスも受け取る
    /// (`set_enabled(false)` と違い、薄く表示されない)。ログや生成結果を
    /// 見せる欄に使う。
    #[inline]
    pub fn set_read_only(&self, read_only: bool) {
        self.0.set_read_only(read_only)
    }

    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.0.is_read_only()
    }
}
