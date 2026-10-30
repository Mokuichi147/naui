//! Following changes of the user's preferred languages.
//!
//! Changing the language or region settings broadcasts `WM_SETTINGCHANGE` with
//! `"intl"` to every top-level window, which is when
//! `GlobalizationPreferences.Languages` is read again. naui's windows are
//! subclassed to receive it, and the reading is deferred through the dispatcher so
//! the app's closure does not run inside the window procedure (a panic there would
//! abort).
//!
//! naui's own strings are chosen from [`display_languages`] instead, which follows
//! the Windows display language like WinUI's own strings do.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use naui_core::Sender;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WM_NCDESTROY, WM_SETTINGCHANGE};

use crate::UiInner;

/// Distinguishes this subclass from the others on the same window.
const SUBCLASS_ID: usize = 0x6c61_6e67; // "lang"

thread_local! {
    /// Reads the languages again on the UI thread, once it is free.
    static REFRESH: RefCell<Option<Sender<()>>> = const { RefCell::new(None) };
    /// What [`preferred_languages`] returns instead of the OS's, for tests.
    static OVERRIDE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Starts telling `ui` about changes. Called on the UI thread.
pub(crate) fn watch(ui: &Rc<UiInner>) {
    let target: Weak<UiInner> = Rc::downgrade(ui);
    let refresh = ui.tasks.channel(move |()| {
        if let Some(inner) = target.upgrade() {
            inner.languages.update(preferred_languages());
        }
    });
    REFRESH.with(|slot| *slot.borrow_mut() = Some(refresh));
}

/// Has `hwnd` pass on `WM_SETTINGCHANGE` with `"intl"`. Returns whether it worked.
pub(crate) fn watch_window(hwnd: HWND) -> bool {
    // SAFETY: HWND は作ったばかりのウィンドウのもので、UI スレッドから呼んでいる。
    unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0).as_bool() }
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if message == WM_SETTINGCHANGE && is_intl(lparam) {
        // ウィンドウが複数あると同じ放送が何度も届くが、読み直した結果が
        // 同じなら知らせないので重ならない。
        REFRESH.with(|slot| {
            if let Some(refresh) = slot.borrow().as_ref() {
                let _ = refresh.send(());
            }
        });
    }
    if message == WM_NCDESTROY {
        let result = DefSubclassProc(hwnd, message, wparam, lparam);
        let _ = RemoveWindowSubclass(hwnd, Some(subclass), subclass_id);
        return result;
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

/// Whether `lparam` of `WM_SETTINGCHANGE` names the `"intl"` area.
fn is_intl(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    // SAFETY: WM_SETTINGCHANGE の lParam は NUL 終端の UTF-16 文字列か 0。
    let area = unsafe { windows_core::PCWSTR(lparam.0 as *const u16).to_string() };
    area.is_ok_and(|area| area.eq_ignore_ascii_case("intl"))
}

/// The user's preferred languages, most preferred first (`"ja"`, `"en-US"` and so
/// on), in the order of the list in Settings.
///
/// Read with `GlobalizationPreferences.Languages`, which works for unpackaged apps
/// too and sees a reordering while the app runs. `GetUserPreferredUILanguages`
/// does not: it starts with the display language, which changes only after signing
/// in again. Falls back to [`display_languages`] if it fails.
pub(crate) fn preferred_languages() -> Vec<String> {
    use windows::System::UserProfile::GlobalizationPreferences;

    if let Some(languages) = OVERRIDE.with(|o| o.borrow().clone()) {
        return languages;
    }
    let languages = GlobalizationPreferences::Languages().map(|tags| {
        tags.into_iter()
            .map(|tag| tag.to_string())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>()
    });
    match languages {
        Ok(languages) if !languages.is_empty() => languages,
        _ => display_languages(),
    }
}

/// The user's preferred UI languages, starting with the display language
/// (`"ja-JP"` and so on). What naui's own strings are chosen from.
///
/// Read with `GetUserPreferredUILanguages`, which works for unpackaged apps too.
/// Empty if it fails, which leaves naui's strings in English.
pub(crate) fn display_languages() -> Vec<String> {
    use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

    let mut count = 0u32;
    let mut len = 0u32;
    // 1 回目で必要な長さを聞き、2 回目で受け取る。中身は NUL 区切りで、
    // 最後に NUL が 2 つ続く。
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len) }
        .is_err()
    {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    if unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(windows_core::PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    buffer
        .split(|&c| c == 0)
        .filter(|tag| !tag.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// Makes the languages read from now on `languages` instead of the OS's (`None`
/// goes back to the OS). **For automated tests only.**
///
/// Nothing is notified by this alone. Send `WM_SETTINGCHANGE` with `"intl"` to a
/// window to have them read again.
#[doc(hidden)]
pub fn override_preferred_languages_for_test(languages: Option<Vec<String>>) {
    OVERRIDE.with(|o| *o.borrow_mut() = languages);
}
