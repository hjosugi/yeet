# Yeet

[![CI](https://github.com/hjosugi/wayland-yeet/actions/workflows/ci.yml/badge.svg)](https://github.com/hjosugi/wayland-yeet/actions/workflows/ci.yml)

Wayland と Windows 向けの、Yoink ライクな軽量ドラッグ＆ドロップ shelf です。

ファイルを画面端の細い strip にドラッグすると shelf が現れます。いったん shelf
へ置いて移動先を開き、そこからもう一度ドラッグできます。受け入れられた drop
だけを削除し、Esc や無効な場所への drop で item を失いません。

![2つのファイルを保持したYeetシェルフ](docs/screenshots/yeet-linux-dark.png)

0.3 は Rust ネイティブ版として、core shelf、file/text/image drop、
複数選択 drag-out、pin、preview、永続化、settings、clipboard capture、
single-instance CLI forwarding、multi-monitor strip、`gtk4-layer-shell` と
GNOME fallback、GlobalShortcuts portal、tray、キーボード操作、日英UIを
実装しています。

## Linux へインストール

現在のリリースアーカイブをダウンロードし、`/usr/local` へインストールします。

```sh
version=0.3.0
base="https://github.com/hjosugi/wayland-yeet/releases/download/v${version}"
curl -fLO "$base/yeet-${version}-linux-x86_64.tar.gz"
curl -fLO "$base/SHA256SUMS-linux.txt"
grep "yeet-${version}-linux-x86_64.tar.gz" SHA256SUMS-linux.txt | sha256sum -c -
tar -xzf "yeet-${version}-linux-x86_64.tar.gz"
root="yeet-${version}-linux-x86_64"
sudo cp -a "$root/bin/." /usr/local/bin/
sudo cp -a "$root/share/." /usr/local/share/
yeet --hidden
```

先にGTKのruntimeをインストールしてください。

```sh
# Arch Linux
sudo pacman -S gtk4 gtk4-layer-shell

# Fedora
sudo dnf install gtk4 gtk4-layer-shell

# Ubuntu 25.10以降
sudo apt install libgtk-4-1 libgtk4-layer-shell0
```

Ubuntu 24.04には`gtk4-layer-shell`パッケージがありません。Yeetを起動する前に、
CIと同じ固定バージョンをupstreamからインストールします。

```sh
sudo apt update
sudo apt install libgtk-4-dev libwayland-dev wayland-protocols meson ninja-build
git clone --depth 1 --branch v1.3.0 https://github.com/wmww/gtk4-layer-shell.git /tmp/gtk4-layer-shell
meson setup /tmp/gtk4-layer-shell/build /tmp/gtk4-layer-shell \
  --prefix=/usr/local -Dexamples=false -Ddocs=false -Dtests=false \
  -Dintrospection=false -Dvapi=false
ninja -C /tmp/gtk4-layer-shell/build
sudo ninja -C /tmp/gtk4-layer-shell/build install
sudo ldconfig
```

リリースアーカイブは現在x86-64向けです。Archでは
`packaging/arch/PKGBUILD`から、Nixでは`nix run github:hjosugi/wayland-yeet`でも
インストール・起動できます。

## ソースからビルド

Rust 1.92 以上、GTK 4.10 以上、Wayland では `gtk4-layer-shell` が必要です。
Ubuntu 24.04 には GTK4 版の package がないため、CI と同じく upstream
v1.3.0 を source build してください。

PDF preview は Poppler の `pdftoppm` が利用可能なら先頭ページを表示し、
未インストールの場合は既定の PDF application で開きます。

```sh
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
