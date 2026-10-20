# layoutsmith

A desktop tool, written in Rust, for opening Android layout XML files and changing their design visually: move views, resize them, align them, restyle and recolor them. Every change is written back as a small edit to the XML, so the file stays clean in version control.

![layoutsmith editing a profile screen](docs/screenshot.png)

## Features

- **Live preview** of the layout on a phone, tablet or desktop-sized screen, portrait or landscape.
  - It uses a measure and layout engine modeled on Android's.
  - Supported containers: `LinearLayout` (weights, gravity), `FrameLayout`, `RelativeLayout`, `ConstraintLayout` (constraints, bias, `0dp` match-constraints) and `ScrollView`.
- **Move** views by dragging them. layoutsmith writes whatever the parent understands:
  - margins in a `FrameLayout` or `RelativeLayout`
  - margins, bias or a new constraint in a `ConstraintLayout`
  - a new position among the children in a `LinearLayout`

  Drop a view onto another container to move it inside. Arrow keys nudge by 1dp (Shift: 8dp).
- **Resize** with the handles. The view keeps its anchoring: a view pinned to the end stays pinned to the end.
- **Align** start, center, end or fill on each axis, with one click.
- **Style** from the inspector:
  - size: `match_parent`, `wrap_content`, fixed, match-constraints, weight
  - margins and padding
  - text, size, color, bold, italic, all caps and gravity
  - background, tint, alpha, visibility, elevation, card color and corner radius
  - image source, scale type and tint
- **Colors** with a color picker, or chosen from the project's own `@color/…` values.
- **Your project's resources**: `@color`, `@dimen`, `@string`, styles, `textAppearance` and theme attributes such as `?attr/colorPrimary` are read from `res/values/`, so the preview matches your app.
- **Lossless editing**: comments, attribute order, quoting and indentation are kept exactly as they were. Untouched files save byte for byte, and edits only change the attributes involved. New attributes follow the file's formatting.
- **Safe structure changes**:
  - Deleting a view, or moving it out of a `ConstraintLayout` or `RelativeLayout`, re-attaches the siblings that were positioned relative to it.
  - Duplicating a view gives the copy fresh ids.
- **View tree** with move up and down, duplicate and delete, plus a palette of common views.
- **XML panel** to read or edit the source directly.
- Undo and redo for everything, a list of the other layouts in the same folder, and drag-and-drop to open files.

## Getting started

### Windows: download

1. Open the [latest release](https://github.com/iiD4x/layoutsmith/releases/latest).
2. Download `layoutsmith-<version>-windows-x86_64.exe`. The `.zip` holds the same program plus the sample project.
3. Double-click it. There is nothing to install, and you don't need Rust or the Visual C++ runtime.

The exe isn't code-signed yet. The first time, Windows SmartScreen may say "Windows protected your PC": click **More info**, then **Run anyway**.

To open a layout directly, drag the `.xml` file onto the window, or use **Open with** and choose `layoutsmith.exe`.

### Build from source

You need a recent stable Rust toolchain (1.95 or newer).

- **Windows**: install Rust with [rustup-init.exe](https://rustup.rs). It offers to install the Visual Studio C++ Build Tools Rust needs; accept.
- **macOS / Linux**: install Rust with [rustup](https://rustup.rs).

Then:

```sh
git clone https://github.com/iiD4x/layoutsmith
cd layoutsmith
cargo run --release -- path/to/app/src/main/res/layout/activity_main.xml
```

On Windows the path looks like `C:\Users\you\AndroidStudioProjects\MyApp\app\src\main\res\layout\activity_main.xml`. `cargo build --release` puts the program in `target\release\layoutsmith.exe`.

Run it without a file to get a welcome screen with **Open**, **New layout** and **Try the sample**. The `samples/` folder contains a small Android `res` tree to play with.

Open layouts from inside your Android project (`…/res/layout/…`) so layoutsmith can find `res/values/` next to them.

Files with Windows line endings (CRLF) stay CRLF when you edit them.

### Windows graphics troubleshooting

On Windows, layoutsmith draws with Direct3D 12. On virtual machines and Remote Desktop without a GPU, it falls back to Windows' built-in software renderer, and then to OpenGL. If the window still doesn't appear, an error box says why. Updating the graphics driver usually fixes it. You can also choose the renderer yourself before starting the app:

```bat
set LAYOUTSMITH_RENDERER=glow
layoutsmith.exe
```

(`glow` is OpenGL, `wgpu` is Direct3D.)

### Linux

At runtime you need an X11 or Wayland session with OpenGL, plus `libxkbcommon-x11` (Debian and Ubuntu: `sudo apt install libxkbcommon-x11-0`). The open and save dialogs use the XDG desktop portal. If no portal is available, pass the file on the command line instead.

## Using it

| Action | How |
| --- | --- |
| Select | Click a view on the canvas or in the tree. Esc selects the parent. |
| Move | Drag the view. In a LinearLayout, dragging reorders. |
| Move into a container | Drag and drop onto the container (it is outlined in green). |
| Resize | Drag one of the eight handles. Turn on **Snap 8dp** for grid sizes. |
| Nudge | Arrow keys (1dp), Shift + arrow keys (8dp). |
| Align | **Position** section of the inspector. |
| Add a view | **Add view** above the tree, or right-click a container in the tree. |
| Duplicate / delete | Ctrl+D / Del |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y) |
| Save / save as | Ctrl+S / Ctrl+Shift+S |
| Zoom | Ctrl + mouse wheel, or the buttons in the status bar. |

## How it works

The workspace has two crates:

- **`layoutsmith-core`**: no GUI dependencies.
  - `xml`: a lossless XML model. Every byte of whitespace, quoting and comments survives a round trip.
  - `layout`: a simplified version of Android's measure and layout passes (`MeasureSpec`s, weights, gravity, RelativeLayout rules, constraints with bias), producing a rectangle for every view.
  - `edit`: design operations (move, resize, align, reparent, duplicate, insert) expressed as minimal attribute changes for each kind of parent.
  - `resources`: `res/values` lookup.
  - `history`: undo and redo.
- **`layoutsmith`**: the [egui](https://github.com/emilk/egui) desktop app (canvas, view tree, inspector).

```sh
cargo test     # core tests, including end-to-end checks against samples/
cargo clippy --all-targets
```

## Limitations

The preview is an approximation, not a pixel-exact Android renderer.

- Drawables, vector images and custom views are shown as placeholders.
- Text uses egui's bundled font rather than Roboto.
- ConstraintLayout chains, guidelines, barriers and dimension ratios are not modeled yet.
- `<include>` is shown as a placeholder.
- Only the default `values/` folder is read (no `values-night`, no qualifiers).

## License

MIT, see [LICENSE](LICENSE).
