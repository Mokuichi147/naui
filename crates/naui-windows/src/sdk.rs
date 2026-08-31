//! Windows App SDK ランタイムの取り付け。
//!
//! WinUI 3 のコントロールは Windows SDK ではなく **Windows App SDK** の
//! フレームワークパッケージに入っている。未パッケージのアプリは、それを
//! 「動的依存」として自分のプロセスへ取り付けてから使う。
//!
//! 相手のパッケージは版ごとに別物 (`Microsoft.WindowsAppRuntime.<版>_8wekyb3d8bbwe`)
//! なので、naui は 1 つに決め打ちせず、**新しいほうから順に試して最初に
//! 取り付けられたもの**を使う。利用者の環境に 2.x が入っていなくても、
//! 1.x が入っていればそのまま動く。
//!
//! 下限を 1.3 にしているのは、ウィンドウの背景に使う `MicaBackdrop` と
//! `Window.SystemBackdrop` が 1.3 で入ったため。それより古い版では
//! `Window` の組み立てから成り立たない。

use naui_core::{Error, Result};
use naui_winui3::bootstrap::{PackageDependency, Version};

/// 取り付けを試す版。新しいものが先。
///
/// `Cbs`(OS 同梱) 系は、対応する版の通常パッケージが入っていない環境向けの
/// 別系統なので、同じ版の直後に置く。
pub(crate) const CANDIDATES: &[Version] = &[
    Version::V2,
    Version::Cbs2,
    Version::V1_8,
    Version::Cbs1_8,
    Version::V1_7,
    Version::V1_6,
    Version::Cbs1_6,
    Version::V1_5,
    Version::V1_4,
    Version::V1_3,
];

/// 使える Windows App SDK ランタイムを 1 つ取り付ける。
///
/// 返り値を落とすと取り付けも外れるので、アプリが終わるまで持ち続ける。
pub(crate) fn initialize() -> Result<PackageDependency> {
    let mut last = None;
    for version in CANDIDATES {
        match PackageDependency::initialize(*version) {
            Ok(dependency) => return Ok(dependency),
            Err(e) => last = Some(e),
        }
    }
    let detail = match last {
        Some(e) => e.message(),
        // CANDIDATES が空にならない限りここへは来ない。
        None => String::new(),
    };
    Err(Error::new(
        "Windows App SDK ランタイムの初期化",
        format!("{detail} (試した版: {})", version_list()),
    ))
}

/// エラーに載せる、試した版の一覧。
fn version_list() -> String {
    CANDIDATES
        .iter()
        .map(|version| format!("{version:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 新しい版から順に試す。古いほうが先に当たると、2.x が入っている環境で
    /// わざわざ 1.x を掴んでしまう。
    #[test]
    fn candidates_are_newest_first() {
        let order: Vec<String> = CANDIDATES.iter().map(|v| format!("{v:?}")).collect();
        let first = order.first().map(String::as_str);
        assert_eq!(first, Some("V2"));
        assert_eq!(order.last().map(String::as_str), Some("V1_3"));
    }

    /// 同じ版を 2 回試しても意味が無い。
    #[test]
    fn candidates_have_no_duplicates() {
        let mut seen: Vec<String> = CANDIDATES.iter().map(|v| format!("{v:?}")).collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), before, "重複した版がある");
    }

    /// エラーの文面に、試した版が全部並ぶ。
    #[test]
    fn version_list_names_every_candidate() {
        let list = version_list();
        for version in CANDIDATES {
            assert!(list.contains(&format!("{version:?}")), "{version:?} が無い");
        }
    }
}
