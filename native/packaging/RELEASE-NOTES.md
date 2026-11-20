# mcbench-lite 2.0.0

Released 2026-11-20.

This is the first release of mcbench-lite as a native app for macOS, Windows and Linux. AI models build in Minecraft 1.21.10 inside one program written in Rust, which draws its own window. There is no server to start, no browser tab and no port: the app is installed like any other program and opened from the Applications folder, the Start menu or the applications menu.

## Install

The download page is https://adonis-singh.github.io/mcbench-lite/. It shows the commands below for the system it is opened on.

| System | How to install |
|---|---|
| macOS (Apple silicon and Intel) | `curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh \| sh` in Terminal |
| Windows (x64) | `irm https://adonis-singh.github.io/mcbench-lite/install.ps1 \| iex` in PowerShell, or the installer `mcbench-lite-setup-x64.exe` that the command runs. `winget install adonis-singh.mcbench-lite` follows once Microsoft has accepted the manifest |
| Ubuntu and Debian | `wget -qO /tmp/mcbench-lite.deb https://github.com/adonis-singh/mcbench-lite/releases/latest/download/mcbench-lite_$(dpkg --print-architecture).deb && sudo apt install -y /tmp/mcbench-lite.deb` |
| Fedora | `sudo dnf install -y https://github.com/adonis-singh/mcbench-lite/releases/latest/download/mcbench-lite.$(uname -m).rpm` |
| Arch | `yay -S mcbench-lite-bin` or `paru -S mcbench-lite-bin`, or `git clone https://aur.archlinux.org/mcbench-lite-bin.git && cd mcbench-lite-bin && makepkg -si` |
| Other Linux (glibc, x86_64 or aarch64) | `curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh \| sh`, which installs into your home folder without sudo, or the AppImage `mcbench-lite-<arch>.AppImage` |

On Linux the model's bash shell runs under bubblewrap, so the package bubblewrap must be installed. The install script refuses systems that use the musl C library, such as Alpine.

| Download | Size |
|---|---|
| macOS archive (Apple silicon, Intel) | 40.4, 41.9 MB |
| Windows installer | 32.2 MB |
| Linux `.deb`, `.rpm`, archive, AppImage (x86_64) | 37.7, 40.5, 52.8 and 49.5 MB |
| Linux `.deb`, `.rpm`, archive, AppImage (aarch64) | 32.0, 36.5, 48.8 and 45.6 MB |

Measured on the release candidate's builds in virtual machines and containers: the macOS install command takes about a second, the Windows install command about 15 seconds, apt and dnf 5 to 10 seconds; the first window paints about 0.2 seconds after the program starts.

## Warnings on the first install

The app is free to download and the project pays for no signing certificate, so Windows warns about some of its routes.

| Route | What you see |
|---|---|
| macOS, the install command | Nothing. The app opens when the script finishes |
| Windows, the install command in PowerShell | No warning and no administrator prompt: the command downloads the installer, checks its sha256 against the release's `SHA256SUMS` and runs it for your account |
| Windows, the installer downloaded with Edge | Edge holds the download back: "mcbench-lite-setup-x64.exe isn't commonly downloaded. Make sure you trust mcbench-lite-setup-x64.exe before you open it." Choose to keep it in Edge's Downloads panel |
| Windows, opening that installer | "Windows protected your PC", with "Publisher: Unknown publisher": press More info, then Run anyway. The installer then needs no administrator. Where Smart App Control is switched on, it blocks the installer |
| Windows, winget | Needs a current winget; the winget 1.6 in a stock Windows 11 image crashes. Whether SmartScreen asks during a winget install is not known yet |
| Linux, apt, dnf, AUR and the install script | Nothing |
| Linux, AppImage | Make the file executable with `chmod +x` |

The installer needs no administrator. On a computer without WSL, the app's first window shows a setup step with an Install WSL button: WSL is what the model's bash shell and the build tool run in, Windows asks once for an administrator's approval, and on most computers it then needs one restart. On a computer where WSL already works, nothing is asked. The first time the model uses bash or build, the app downloads its Linux base and imports its own WSL distribution, which takes about 20 seconds, and Windows may show its own "Welcome to Windows Subsystem for Linux" window.

## For people who used the localhost app

In this release the native app and the localhost app overlap. The localhost app, which runs as a local server from a checkout of the repository and is used in a browser, still works from a checkout as before. It is removed in the next release, together with its `./start` script, its page, its token link and its port. The table compares the two as they stand in this release.

| | Localhost app | Native app |
|---|---|---|
| How it runs | A Node server opened in a browser at a local address | One program with its own window |
| Data folder | `data` in the checkout, or the folder `MCBENCH_DATA_DIR` named | macOS `~/Library/Application Support/mcbench-lite`, Windows `%LOCALAPPDATA%\mcbench-lite`, Linux `$XDG_DATA_HOME/mcbench-lite` (`~/.local/share/mcbench-lite` when `XDG_DATA_HOME` is not set) |
| Sandbox for the model's shell | Docker | bubblewrap: on the host on Linux, inside the app's own Linux virtual machine on macOS, inside a WSL distribution on Windows |
| Updates | `git pull` in the checkout | The app updates itself, or your package manager does (see Updates) |

The native app opens a localhost app's data folder unchanged. To move your chats and worlds, quit the localhost app, copy the contents of its data folder (`data` in the checkout, or the folder `MCBENCH_DATA_DIR` named) into the native app's data folder before the native app's first start, and then start the native app. If the native app has already been started once, its folder holds an empty database: quit the native app and replace that folder's contents with the copy, without merging the two. The copy leaves the localhost app's folder as it was, as a backup. The sign-ins of Claude Code and Codex are kept by those programs outside the data folder, so on the same computer they carry over with no step.

| In the localhost app's data folder | Copy it |
|---|---|
| `mcbench.sqlite3` with its `-wal` and `-shm` files, `worlds`, `images`, `attachments`, `sandbox`, `harness`, `settings.json`, `prompts.json`, `minecraft-verify` | Yes |
| Everything else | Not needed: the native app ignores it or makes it again |

The native app reads data folders at database schema 14. It refuses an older data folder with the message "Database schema version `<n>` is unsupported (expected 14)". To bring an older data folder to schema 14, open it once with the current localhost app, which migrates it, and then open it with the native app.

Cursor is not in the native app. Chats whose provider is Cursor are kept and can be read, and sending a message in one is refused with the message "Cursor is not in the native app".

Claude Code and Codex CLI are the vendors' own programs, as before. When you choose one, the app downloads it at a version pinned in the app and signs in through the program's own sign-in. Python is no longer needed on your computer for them.

On first start the app downloads the Minecraft textures it draws with from Mojang's servers, from inside the Minecraft client jar. Java and a Minecraft download on your computer are no longer needed.

Sharing a build works as before, and a shared page is the same page as today's. The first time you publish, the app downloads BlueMap's command-line jar (6.6 MB, at a version pinned in the app) from BlueMap's own GitHub release into the data folder and reads the page's viewer from it; Java is not needed. Publishing needs `git` and `gh` on your computer, as before.

## On a machine with no display or no GPU

The app's commands that need no window run on a server. `mcbench-lite headless setup <data folder>` does what the first start does with no window: it unpacks the shipped Minecraft data and fetches and bakes the textures, and prints the two folders as one line, `{"tables": …, "textures": …}`. `mcbench-lite headless turn` then takes those folders as `MCBENCH_TABLES` and `MCBENCH_TEXTURES`; a turn that never calls `see_build` needs only the first.

On Linux the model's `bash` runs in a bubblewrap sandbox whose limits are set by systemd, so the login that runs the app needs a systemd user session: on a server where none starts by itself, `loginctl enable-linger <user>` gives it one. The networked shell also needs `/dev/net/tun`. Setup downloads the shell's root filesystem (about 83 MB) once, checks it against the sha256 pinned in the app, and keeps it in the data folder; on a server, `mcbench-lite headless setup <data folder>` does it, so a machine is ready before its first turn.

On Linux x86_64 the download holds a software renderer (SwiftShader's Vulkan driver, `libvk_swiftshader.so` beside the program, with its licences), which the app opens by itself when the machine has no graphics adapter, so `see_build` draws its pictures on a machine with no GPU and no graphics driver installed. Linux on ARM has no such driver in this release and needs a GPU driver for `see_build`. macOS always has Metal, and Windows uses its own software adapter where there is no GPU.

## Updates

| Installed with | How updates arrive |
|---|---|
| The macOS install command | The app updates itself: it checks in the background, downloads and verifies the new version, and switches to it the next time it starts, without asking |
| The Windows install command or installer | The app updates itself, as on macOS |
| winget | `winget upgrade adonis-singh.mcbench-lite` |
| apt (Ubuntu and Debian) | With your system updates: `sudo apt update && sudo apt upgrade` |
| dnf (Fedora) | With your system updates: `sudo dnf upgrade` |
| AUR (Arch) | With your AUR helper, for example `yay -Syu` |
| The Linux install script | The app updates itself |
| AppImage | The app replaces its own AppImage file |

An update the app installs itself is checked against a signature made with the project's key before it replaces anything. Your window's size, layout and theme carry over an update.

If you quit while a build is running, the app asks first, with Keep building and Quit.

## Uninstall

Every uninstall keeps the data folder, which holds your chats, worlds and settings. Claude Code's transcripts of the app's chats (in the `.claude/projects` folder in your home folder) and the Codex sign-in file `.codex/auth.json` in your home folder belong to those programs and are kept too.

| Installed with | How to uninstall | What it also removes |
|---|---|---|
| The macOS install command | `curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh \| sh -s -- --uninstall` | What macOS keeps for the app (saved window state, caches, preferences), `~/.cache/mcbench` and the app's leftover temporary folders; it prints the folders it keeps |
| The Windows install command or installer | Settings > Apps > Installed apps > mcbench-lite > Uninstall | The app's own WSL distribution `mcbench-lite`, which holds nothing of yours, its cache folder and its leftover temporary folders |
| winget | `winget uninstall adonis-singh.mcbench-lite` | As the Windows installer |
| apt | `sudo apt remove mcbench-lite` | Nothing in your home folder; `~/.cache/mcbench` can be deleted by hand |
| dnf | `sudo dnf remove mcbench-lite` | As apt |
| AUR | `sudo pacman -R mcbench-lite-bin` | As apt |
| The Linux install script | `curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh \| sh -s -- --uninstall` | Its link in `~/.local/bin`, its menu entry and icon, `~/.cache/mcbench` and the app's leftover temporary folders; it prints the folders it keeps |
| AppImage | Delete the AppImage file. If they are there, also delete what the app's updater keeps beside it: every hidden file or folder whose name is a dot, the AppImage's full file name and another dot followed by anything (for example `.mcbench-lite-x86_64.AppImage.update`), and the file with the AppImage's full name followed by `.old` | Nothing else |

## Minecraft data

The native app's download contains data derived from Minecraft 1.21.10 by running Minecraft's own code at release time: block, item, loot and data-fixer tables, block names, render layers, tints, block-model geometry, and geometry captured from Minecraft's block-entity and entity renderers. This data belongs to Mojang Studios and Microsoft and is not under the MIT licence. The download contains no Minecraft textures, sounds, jars, source code or mappings. On first start the app downloads the textures it draws with from Mojang's servers, from inside the Minecraft client jar, to the machine it runs on. mcbench-lite is not an official Minecraft product and is not approved by or associated with Mojang or Microsoft.

## Sources of the GPL software in the download

The Linux system that the model's shell runs in (the base of the macOS virtual machine and of the Windows WSL distribution, and the root filesystem of the shell on every system) is built from Debian packages, some under the GPL. The app downloads the Linux base and the root filesystem from a release of their own, which app releases point at, and the sources of those packages are beside the downloads of those files: https://github.com/adonis-singh/mcbench-lite/releases/tag/sandbox-assets-d76a7188bbcc92ce. The download page links the same address as "Sources of the GPL software in the download".

The source code of mcbench-lite itself is at https://github.com/adonis-singh/mcbench-lite under the MIT licence.
