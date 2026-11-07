# Installing LabCut

LabCut is built from its source code with **Rust**. The steps are the same on every system:
install Rust, build LabCut once, then start it like any other program. The first build
downloads the libraries LabCut uses and takes a few minutes. Later builds are much faster.

- [Windows](#windows)
- [macOS](#macos)
- [Linux](#linux)
- [Checking that it works](#checking-that-it-works)
- [Updating and uninstalling](#updating-and-uninstalling)
- [Troubleshooting](#troubleshooting)

**What you need on every system**

- Rust **1.95 or newer** (installed with `rustup`, below)
- About 2 GB of free disk space for the build. The finished program is a single file of about 10–20 MB.
- A graphics driver that supports OpenGL 2.0 or newer (almost every computer from the last ten years)
- An internet connection for the first build

---

## Windows

Works on Windows 10 and Windows 11 (64-bit).

### 1. Install the C++ build tools

Rust on Windows needs Microsoft's C++ build tools.

1. Download **Build Tools for Visual Studio** from
   <https://visualstudio.microsoft.com/visual-cpp-build-tools/>.
2. Run the installer, tick **Desktop development with C++**, and click **Install**.

If you already have Visual Studio with C++ installed, skip this step.

### 2. Install Rust

1. Download and run **rustup-init.exe** from <https://rustup.rs>.
2. Press **Enter** to accept the default installation.
3. Close the window and open a **new** PowerShell or Command Prompt window, so it picks up the new settings.
4. Check that it worked:
   ```powershell
   rustc --version
   ```
   It should print `rustc 1.95.0` or newer. If yours is older, run `rustup update`.

### 3. Build LabCut

In PowerShell, go to the LabCut folder (the one containing `Cargo.toml`) and build it:

```powershell
cd C:\path\to\LabCut\app
cargo build --release
```

The program is created at `target\release\labcut.exe`.

### 4. Start it

Double-click `target\release\labcut.exe` in File Explorer, or run:

```powershell
.\target\release\labcut.exe
```

### 5. Add it to the Start menu (optional)

1. Copy `labcut.exe` to a permanent place, for example `C:\Program Files\LabCut\`.
   (You can also make a `LabCut` folder in your user folder.)
2. Right-click `labcut.exe` and choose **Show more options → Create shortcut**.
3. Move the shortcut to
   `%APPDATA%\Microsoft\Windows\Start Menu\Programs\`
   (paste that path into File Explorer's address bar).

LabCut now appears in the Start menu. To open DXF files with LabCut by double-clicking
them, right-click a `.dxf` file and choose **Open with → Choose another app → Choose an
app on your PC**, then pick `labcut.exe`. Do the same for `.svg` and `.gcode` if you want.

### Windows notes

- **SmartScreen warning:** a program you built yourself isn't signed, so Windows may warn
  you the first time. Click **More info → Run anyway**.
- **Command-line messages:** the release build has no console window of its own. When you run
  `labcut.exe --help` or `--export` from PowerShell, its messages appear in that PowerShell window.
  Windows doesn't wait for windowed programs to finish, so the prompt may come back before the
  message appears. In scripts, wait for the conversion to finish like this:
  ```powershell
  Start-Process .\labcut.exe -ArgumentList 'drawing.dxf','--export','drawing.gcode' -Wait -NoNewWindow
  ```

---

## macOS

Works on macOS 11 (Big Sur) and newer, on Apple Silicon (M1/M2/M3/M4) and Intel Macs.

### 1. Install the command-line developer tools

Open **Terminal** (Applications → Utilities) and run:

```bash
xcode-select --install
```

Click **Install** in the window that appears. If it says the tools are already installed, continue.

### 2. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Press **Enter** to accept the defaults. Then close Terminal and open a new one, and check:

```bash
rustc --version
```

### 3. Build LabCut

```bash
cd /path/to/LabCut/app
cargo build --release
```

The program is created at `target/release/labcut`.

### 4. Start it

```bash
./target/release/labcut
```

### 5. Make it a normal Mac app (optional)

These commands wrap the program in `LabCut.app` and put it in your Applications folder,
so you can start it from Launchpad, Spotlight or the Dock:

```bash
mkdir -p LabCut.app/Contents/MacOS
cp target/release/labcut LabCut.app/Contents/MacOS/labcut
cat > LabCut.app/Contents/Info.plist <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>LabCut</string>
  <key>CFBundleDisplayName</key><string>LabCut</string>
  <key>CFBundleIdentifier</key><string>local.labcut</string>
  <key>CFBundleExecutable</key><string>labcut</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
mv LabCut.app /Applications/
```

### macOS notes

- **"LabCut can't be opened because it is from an unidentified developer":** this only
  happens if you copy the app from another Mac. Right-click the app, choose **Open**, then
  **Open** again. Or run `xattr -dr com.apple.quarantine /Applications/LabCut.app`.
- **Ctrl or Cmd:** on a Mac, the keyboard shortcuts use **Cmd** (⌘ O to open, ⌘ S to save).
- **Running from the app bundle** doesn't let you pass command-line options. Use the
  `labcut` program inside `LabCut.app/Contents/MacOS/` for `--export`.

---

## Linux

These steps are written for Ubuntu 22.04 / 24.04 and Debian 12, with the matching
packages for Fedora and Arch below. Other distributions need the same libraries under their own package names.

### 1. Install the system packages

**Ubuntu, Debian, Linux Mint, Pop!_OS**

```bash
sudo apt-get update
sudo apt-get install build-essential pkg-config curl \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev \
    libgl1-mesa-dev xdg-desktop-portal xdg-desktop-portal-gtk
```

**Fedora**

```bash
sudo dnf install gcc pkgconf-pkg-config curl libxkbcommon-devel libxcb-devel \
    mesa-libGL-devel openssl-devel xdg-desktop-portal xdg-desktop-portal-gtk
```

**Arch Linux, Manjaro**

```bash
sudo pacman -S --needed base-devel curl libxkbcommon libxcb mesa openssl \
    xdg-desktop-portal xdg-desktop-portal-gtk
```

The `xdg-desktop-portal` packages provide the **Open** and **Save** file windows. On KDE
Plasma you can use `xdg-desktop-portal-kde` instead of the `-gtk` one. Most desktops
already have one installed.

### 2. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Press **Enter** to accept the defaults, then open a new terminal and check:

```bash
rustc --version
```

Use rustup rather than your distribution's `rustc` package. Distribution packages are often
older than the 1.95 that LabCut needs.

### 3. Build LabCut

```bash
cd /path/to/LabCut/app
cargo build --release
```

The program is created at `target/release/labcut`.

### 4. Start it

```bash
./target/release/labcut
```

### 5. Install it for your user (optional)

This copies LabCut into `~/.local/bin` and adds it to your app menu:

```bash
mkdir -p ~/.local/bin ~/.local/share/applications
cp target/release/labcut ~/.local/bin/
cat > ~/.local/share/applications/labcut.desktop <<EOF
[Desktop Entry]
Type=Application
Name=LabCut
Comment=Turn DXF and SVG drawings into laser G-code
Exec=$HOME/.local/bin/labcut %f
Terminal=false
Categories=Graphics;Engineering;
MimeType=image/vnd.dxf;image/svg+xml;text/x-gcode;
EOF
update-desktop-database ~/.local/share/applications 2>/dev/null || true
```

LabCut now appears in your app menu, and you can choose it under **Open with** for DXF, SVG and G-code files.
If `~/.local/bin` is on your `PATH`, you can also just type `labcut` in a terminal.

To install it for every user instead, copy the program to `/usr/local/bin/` and the
`.desktop` file to `/usr/share/applications/` (with `sudo`), and change the `Exec=` line to match.

### Linux notes

- **Wayland and X11:** LabCut supports both and picks the one your desktop uses.
- **Drag and drop** of files onto the window may not work on Wayland. Use **Open…** or pass
  the file on the command line instead: `labcut drawing.dxf`.

---

## Checking that it works

1. Start LabCut. It opens with a **sample project**: a name tag with a fill-engraved star,
   scored rings and a cut outline on three layers.
2. Click **▶ Simulate** at the top, then **▶ Play**. You should see the laser head
   moving and brown burn marks appearing on the wood-coloured sheet.
3. Click **📄 G-code** to see the generated program.

You can also check from a terminal:

```bash
labcut --help
labcut some-drawing.dxf --export some-drawing.gcode
```

`--export` prints a short summary (number of shapes, size, number of lines, estimated time),
for example:

```
5 shapes, 3 layers, 120.0 x 60.0 mm, 227 lines, about 0:59
```

The `samples/` folder has a DXF (`bracket.dxf`) and an SVG (`badge.svg`) you can try.

## Updating and uninstalling

**Updating:** replace the source folder with the new version, run `cargo build --release`
again, and copy the new program over the old one (Start menu, `/Applications/LabCut.app`
or `~/.local/bin`).

**Uninstalling:** LabCut saves no settings and puts no files anywhere else on your
computer. To remove it:

- delete the program (and the shortcut, the `LabCut.app` bundle or `labcut.desktop` you made);
- delete the source folder. Most of its size is the `target` build folder.
- To remove Rust as well, run `rustup self uninstall`.

## Troubleshooting

| Problem | What to do |
|---|---|
| `error: linker 'link.exe' not found` (Windows) | Install the C++ build tools (Windows step 1), then open a new terminal. |
| `error: rustc 1.xx is not supported` or "package requires rustc 1.95" | Run `rustup update`. |
| `failed to run custom build command` mentioning `xkbcommon`, `xcb` or `pkg-config` (Linux) | Install the system packages from Linux step 1. |
| The window doesn't open; the error mentions OpenGL or `glutin` | Update your graphics driver. In a virtual machine, turn on 3D acceleration or install Mesa. |
| **Open…** or **Save G-code…** does nothing (Linux) | Install `xdg-desktop-portal` and `xdg-desktop-portal-gtk` (or `-kde`), log out and back in. Until then, open files from the command line: `labcut file.dxf`. |
| The first build is very slow | Normal: it compiles all the libraries once. Later builds take seconds. |
| Text in the window is tiny or huge | LabCut follows your system's display scaling. Change it in your system's display settings. |
