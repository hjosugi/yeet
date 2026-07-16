use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    English,
    Japanese,
}

static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    let resolved = match language {
        Language::System => system_language(),
        language => language,
    };
    CURRENT_LANGUAGE.store(
        if resolved == Language::Japanese { 1 } else { 0 },
        Ordering::Relaxed,
    );
}

pub fn tr(key: &str) -> &'static str {
    if CURRENT_LANGUAGE.load(Ordering::Relaxed) == 1 {
        japanese(key)
    } else {
        english(key)
    }
}

fn system_language() -> Language {
    let is_japanese = ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.is_empty())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("ja"));
    if is_japanese {
        Language::Japanese
    } else {
        Language::English
    }
}

fn english(key: &str) -> &'static str {
    match key {
        "shelf_title" => "Yeet file shelf",
        "shelf_description" => {
            "Temporary files and snippets. Use the arrow keys to navigate items."
        }
        "hide_shelf" => "Hide shelf",
        "shelf_items" => "Shelf items",
        "shelf_items_help" => {
            "Use Up, Down, Home or End to navigate. Hold Shift to select multiple items."
        }
        "drop_here" => "Drop files or text here",
        "empty_help" => "The shelf is empty. Drop files or text here.",
        "shelf_actions" => "Shelf actions",
        "wayland_mode" => "Wayland layer shell",
        "windows_mode" => "Windows native",
        "fallback_mode" => "Fallback window",
        "clear_unpinned" => "Remove all unpinned items",
        "capture_clipboard" => "Capture clipboard",
        "settings" => "Settings",
        "pin_item" => "Pin item",
        "unpin_item" => "Unpin item",
        "preview_item" => "Preview item",
        "remove_item" => "Remove item",
        "item_actions" => "Item actions",
        "preview" => "Preview",
        "settings_title" => "Yeet Settings",
        "system" => "System",
        "light" => "Light",
        "dark" => "Dark",
        "english" => "English",
        "japanese" => "Japanese",
        "hide_when_empty" => "Hide when empty",
        "restore_shelf" => "Restore shelf at launch",
        "deduplicate_items" => "Ignore duplicate items",
        "stack_multi_drop" => "Stack multi-item drops",
        "start_session" => "Start with the session",
        "edge_width" => "Edge width",
        "screen_edge" => "Screen edge",
        "left" => "Left",
        "right" => "Right",
        "disabled_outputs" => "Disabled outputs",
        "theme" => "Theme",
        "language" => "Language",
        "reduced_motion" => "Reduce motion",
        "global_hotkey" => "Global shortcut",
        "global_hotkey_hint" => "Use Ctrl, Alt, Shift, or Win plus one key",
        "global_hotkey_invalid" => "Invalid shortcut",
        "global_hotkey_conflict" => "This shortcut is already used or reserved.",
        "global_hotkey_restored" => "The previous shortcut is still active.",
        "global_hotkey_rollback_failed" => "The previous shortcut could not be restored.",
        "global_hotkey_unavailable" => "Global shortcut registration is unavailable.",
        "apply" => "Apply",
        "open" => "Open",
        "reveal" => "Reveal in file manager",
        "copy_path" => "Copy path",
        "remove" => "Remove",
        "show_hide" => "Show / Hide",
        "clear" => "Clear",
        "quit" => "Quit",
        "clipboard_image" => "Clipboard image",
        "image_snippet" => "Image snippet",
        _ => "",
    }
}

fn japanese(key: &str) -> &'static str {
    match key {
        "shelf_title" => "Yeet ファイルシェルフ",
        "shelf_description" => {
            "ファイルやスニペットを一時保管します。矢印キーで項目を移動できます。"
        }
        "hide_shelf" => "シェルフを隠す",
        "shelf_items" => "シェルフ項目",
        "shelf_items_help" => "上下・Home・Endキーで移動し、Shiftキーで複数選択できます。",
        "drop_here" => "ファイルまたはテキストをここへドロップ",
        "empty_help" => "シェルフは空です。ファイルまたはテキストをここへドロップしてください。",
        "shelf_actions" => "シェルフ操作",
        "wayland_mode" => "Wayland layer shell",
        "windows_mode" => "Windows ネイティブ",
        "fallback_mode" => "フォールバックウィンドウ",
        "clear_unpinned" => "ピン留めされていない項目をすべて削除",
        "capture_clipboard" => "クリップボードを取り込む",
        "settings" => "設定",
        "pin_item" => "項目をピン留め",
        "unpin_item" => "ピン留めを解除",
        "preview_item" => "項目をプレビュー",
        "remove_item" => "項目を削除",
        "item_actions" => "項目の操作",
        "preview" => "プレビュー",
        "settings_title" => "Yeet 設定",
        "system" => "システム",
        "light" => "ライト",
        "dark" => "ダーク",
        "english" => "英語",
        "japanese" => "日本語",
        "hide_when_empty" => "空になったら隠す",
        "restore_shelf" => "起動時にシェルフを復元",
        "deduplicate_items" => "重複項目を追加しない",
        "stack_multi_drop" => "複数ドロップをまとめて選択",
        "start_session" => "ログイン時に起動",
        "edge_width" => "画面端の幅",
        "screen_edge" => "表示する画面端",
        "left" => "左",
        "right" => "右",
        "disabled_outputs" => "無効にする出力",
        "theme" => "テーマ",
        "language" => "言語",
        "reduced_motion" => "動きを減らす",
        "global_hotkey" => "グローバルショートカット",
        "global_hotkey_hint" => "Ctrl、Alt、Shift、Winのいずれかと1つのキーを指定",
        "global_hotkey_invalid" => "ショートカットが無効です",
        "global_hotkey_conflict" => "このショートカットは使用中または予約済みです。",
        "global_hotkey_restored" => "以前のショートカットは引き続き有効です。",
        "global_hotkey_rollback_failed" => "以前のショートカットを復元できませんでした。",
        "global_hotkey_unavailable" => "グローバルショートカットを登録できません。",
        "apply" => "適用",
        "open" => "開く",
        "reveal" => "ファイルマネージャーで表示",
        "copy_path" => "パスをコピー",
        "remove" => "削除",
        "show_hide" => "表示・非表示",
        "clear" => "クリア",
        "quit" => "終了",
        "clipboard_image" => "クリップボード画像",
        "image_snippet" => "画像スニペット",
        _ => english(key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: &[&str] = &[
        "shelf_title",
        "shelf_description",
        "hide_shelf",
        "shelf_items",
        "shelf_items_help",
        "drop_here",
        "empty_help",
        "shelf_actions",
        "wayland_mode",
        "windows_mode",
        "fallback_mode",
        "clear_unpinned",
        "capture_clipboard",
        "settings",
        "pin_item",
        "unpin_item",
        "preview_item",
        "remove_item",
        "item_actions",
        "preview",
        "settings_title",
        "system",
        "light",
        "dark",
        "english",
        "japanese",
        "hide_when_empty",
        "restore_shelf",
        "deduplicate_items",
        "stack_multi_drop",
        "start_session",
        "edge_width",
        "screen_edge",
        "left",
        "right",
        "disabled_outputs",
        "theme",
        "language",
        "reduced_motion",
        "global_hotkey",
        "global_hotkey_hint",
        "global_hotkey_invalid",
        "global_hotkey_conflict",
        "global_hotkey_restored",
        "global_hotkey_rollback_failed",
        "global_hotkey_unavailable",
        "apply",
        "open",
        "reveal",
        "copy_path",
        "remove",
        "show_hide",
        "clear",
        "quit",
        "clipboard_image",
        "image_snippet",
    ];

    #[test]
    fn every_ui_key_has_english_and_japanese_text() {
        for key in KEYS {
            assert!(!english(key).is_empty(), "missing English key: {key}");
            assert!(!japanese(key).is_empty(), "missing Japanese key: {key}");
        }
    }
}
