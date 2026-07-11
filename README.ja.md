# Yeet

Wayland と Windows 向けの、Yoink ライクな軽量ドラッグ＆ドロップ shelf です。

ファイルを画面端の細い strip にドラッグすると shelf が現れます。いったん shelf
へ置いて移動先を開き、そこからもう一度ドラッグできます。受け入れられた drop
だけを削除し、Esc や無効な場所への drop で item を失いません。

0.2 で Rust ネイティブ版へ移行しました。core shelf、file/text/image drop、
複数選択 drag-out、pin、preview、永続化、settings、clipboard capture、
single-instance CLI forwarding、multi-monitor strip、`gtk4-layer-shell` と
GNOME fallback を実装しています。

## ビルド

Rust 1.92 以上、GTK 4.8 以上、Wayland では `gtk4-layer-shell` が必要です。

```sh
sudo apt install libgtk-4-dev libgtk4-layer-shell-dev
cargo build --release
cargo test
./target/release/yeet --hidden
```

Linux では layer-shell を実行時に検出します。利用できない場合や GNOME では通常 window
fallback になり、edge strip は表示されません。compositor の keybind に
`yeet --toggle` を登録してください。

Windows では shelf と edge strip の両方へ `HWND_TOPMOST` を適用し、shelf を
再表示するたびに topmost を再設定します。global toggle は Ctrl+Alt+Y です。

CLI:

```text
yeet FILE...   ファイルを追加して表示
yeet --toggle  表示・非表示を切り替え
yeet --clear   pin されていない item を削除
yeet --hidden  非表示で起動
```

詳細な確認項目は [Wayland compositor matrix](docs/compositors.md) と
[Windows の制約](docs/windows.md) を参照してください。
