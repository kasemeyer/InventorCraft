# Compatibility with the SolveCraft name

SolidCraft was first called SolveCraft, and builds up to 0.4 kept that name in their
identifiers after the displayed name changed. 0.5.0 renamed the binaries, release assets and
packages; the release after it renames everything else: crates `solidcraft-*`, binaries `solidcraft` and `solidcraft-cli`, `.solidcraft`
designs, the `"format": "solidcraft/N"` tag, `SOLIDCRAFT_*` environment variables, settings
and recovery folders, the MCP server name, docs, CI and packaging.

The old name survives **only** in the shims below (plus their tests, this file, and changelog /
release notes). `git grep -i solvecraft` must list nothing else. When you add or remove a
shim, update this file in the same PR.

## 1. `.solvecraft` design files

New designs are saved as `.solidcraft`. Files with the old extension still open everywhere:

| Path in | Where |
|---|---|
| Extension → design format | `crates/io/src/lib.rs` (`LEGACY_DESIGN_EXT`, format detection) |
| Open dialog filters (desktop) | `apps/solidcraft/src/main.rs` (`services`) |
| Drag-drop, recent files, `doc.open`, MCP `open`, CLI `eval`/`run` | go through the same format detection in `solidcraft_io` |
| Browser file picker | `apps/solidcraft-web/src/files.rs` (`accept`) |
| Windows | `assets/platforms/windows/solidcraft.wxs`: the `.solvecraft` extension on the design ProgId and in `SupportedTypes` |
| macOS | `assets/platforms/macos/Info.plist.in`: `solvecraft` in the design document type's extensions |
| Linux / FreeBSD / Flatpak | `assets/platforms/linux/com.tcicad.TciCad.mime.xml`: `*.solvecraft` glob and the `application/x-solvecraft` alias |

Recent-files entries that point at a `.solvecraft` file keep working: they are plain paths and
open through the same detection. The browser app also lists designs it stored under the old
extension, and keyboard-shortcut files exported as `solvecraft-shortcuts/1` still import
(`crates/ui-egui/src/toolbar_custom.rs`). Tests: `crates/io` (`Format::from_name`,
`is_design_path`), `legacy_extension_and_format_tag_still_open` (CLI, `run_results.rs`) and
`legacy_design_files_and_resource_uris` (MCP `open`).

## 2. The `solvecraft/N` format tag

Designs record their format as `"format": "solidcraft/N"`. Files saved by builds up to 0.5 say
`"format": "solvecraft/N"`; `crates/doc/src/format.rs` (`version_of`) reads both prefixes and
upgrades them as usual (the file is re-tagged `solidcraft/N` when saved).

Not backward-compatible: builds up to 0.5 don't know the new tag and refuse files saved by
later builds. Test: `version_of` accepts both prefixes; legacy-tagged designs load and are
re-tagged on save.

## 3. `SOLVECRAFT_*` environment variables

Every runtime variable is read through one helper, `solidcraft_geom::env` (`var`, `var_os`):
it reads `SOLIDCRAFT_<NAME>` and falls back to `SOLVECRAFT_<NAME>` when the new one is unset.
The shell entry points do the same: `packaging/linux/AppRun`
(`SOLIDCRAFT_NO_DESKTOP_INTEGRATION`) and `packaging/env.sh` (`SOLIDCRAFT_VERSION`,
`SOLIDCRAFT_BUILD_DATE`). Tests: the helper's unit tests (new wins, old is the fallback,
neither → unset).

## 4. Settings, preferences, autosave and recovery folders

TCI-CAD keeps its own folders and preferences key, so it never reads or changes those of a
SolidCraft or SolveCraft installed on the same machine:

| | Linux | macOS | Windows |
|---|---|---|---|
| Preferences, graphics settings | `~/.local/share/tci-cad` | `~/Library/Application Support/TCI-CAD` | `%APPDATA%\TCI-CAD\data` |
| Recovery (autosave) | `~/.local/share/tci-cad/recovery` | `…/TCI-CAD/recovery` | `%LOCALAPPDATA%\TCI-CAD\recovery` |
| Control channel tokens and port | `~/.local/share/tci-cad/control` | `…/TCI-CAD/control` | `%LOCALAPPDATA%\TCI-CAD\control` |

The preferences key in eframe's storage is `tci-cad.prefs` (`solidcraft_ui_egui::prefs::STORAGE_KEY`);
`solidcraft.prefs` and `solvecraft.prefs` are never read. Nothing is copied from SolidCraft's or
SolveCraft's settings or recovery folders: TCI-CAD's first launch is a fresh install, and the
legacy-settings migration (`adopt_legacy_settings`) was removed. Autosaves named `<id>.solvecraft`
inside TCI-CAD's own recovery folder are still listed and recovered. The browser app also reads
files and the autosave it kept under the old names in local storage and OPFS
(`apps/solidcraft-web/src/files.rs`). A 3DxWare profile the driver made for the old executable
name is read until it makes a new one (`apps/solidcraft/src/space_mouse.rs`).
In the Flatpak sandbox these folders live under `~/.var/app/com.tcicad.TciCad/data`.
The thumbnail cache is not migrated; it is rebuilt. Tests: `recovery_and_data_folders_are_tci_cads`,
`the_control_folder_is_under_tci_cads_data_folder`, `tci_cad_has_its_own_app_id_and_settings_folder`,
`tci_cad_reads_only_its_own_prefs_key`, `autosave_from_a_legacy_folder_is_listed_and_loads`.

## 5. `solvecraft-cli` alias

A tiny `solvecraft-cli` binary (`apps/solidcraft-cli/src/alias.rs`) ships in every package for
at least one release (since 0.5.0). It prints a one-line deprecation note to stderr and runs the
`solidcraft-cli` next to it with the same arguments, stdin/stdout and exit code, so scripts
and MCP client configs that call `solvecraft-cli` keep working. Test: `apps/solidcraft-cli/tests/legacy_alias.rs` (forwards `--version` and the exit status;
the note goes to stderr only, so stdout stays clean for MCP).

There is no alias for the desktop binary: launchers, file associations and Start-menu entries
installed by the packages point at `solidcraft`.

## 6. MCP

`serverInfo.name` is now `solidcraft`. The name a client shows is chosen by the user when the
server is added, so an existing setup such as

```sh
claude mcp add solvecraft -- solvecraft-cli mcp
```

keeps working (through the alias above). New setups use:

```sh
claude mcp add solidcraft -- solidcraft-cli mcp
```

Switch before the alias is removed. Resources are listed as `solidcraft://design` and
`solidcraft://commands`; the old `solvecraft://` URIs still read (`crates/mcp/src/server.rs`,
test `legacy_design_files_and_resource_uris`).

## Package IDs

| Package | Decision | Why |
|---|---|---|
| Flatpak app ID / Linux desktop ID | `com.tcicad.TciCad` | TCI-CAD's own id, so it never shares a desktop entry, Flatpak data folder or window identity with SolidCraft. The `.desktop`, metainfo and MIME file names, the icon name, the Wayland `app_id` / X11 class, the macOS bundle id, the updater (`update::APP_ID`) and `solidcraft_io::APP_ID` all use it; test `packaging_files_use_the_one_app_id`. |
| Flatpak remote file | `solidcraft.flatpakrepo`, plus a legacy `solvecraft.flatpakrepo` copy | Old links keep working; existing remotes point at the repo URL, which is unchanged. |
| macOS bundle ID | `com.tcicad.TciCad` | No updater depends on it; the app is drag-installed. |
| MSI `UpgradeCode` | still SolidCraft's | Not yet changed: see the Plan B final-wave report (an owner decision, since a new code also needs new component GUIDs). |
| deb / rpm package name | renamed to `solidcraft`; deb `Replaces`/`Conflicts`/`Provides: solvecraft`, rpm `Obsoletes`/`Provides: solvecraft` | Installing the new package replaces the old one instead of installing side by side. |
| AppImage | `solidcraft-<version>-linux-<arch>.AppImage`; the release also carries a copy of its `.zsync` under the old `solvecraft-…` name | AppImages already installed look for the old-named `.zsync`, so AppImageUpdate keeps finding updates. |

## Linux app ID

TCI-CAD uses `com.tcicad.TciCad` everywhere (Flatpak, deb, rpm, AppImage, Snap, FreeBSD, Nix).
It does not claim to replace SolidCraft (`ai.storyteller.solidcraft`) or SolveCraft
(`ai.storyteller.solvecraft`): its metainfo has no `<replaces>` or legacy `<provides>`, its desktop
entry no `X-Flatpak-RenamedFrom`, and its Flatpak manifest no grant to the old app's data folder.
Upstream's remaining rename shims are still in the tree and name only SolveCraft's old id:

| Shim | Where | What it does |
|---|---|---|
| End-of-life rebase in the Flatpak repository | `packaging/flatpak-site.sh` | Marks the old `ai.storyteller.solvecraft` refs end-of-life with a rebase to `$app_id`. |
| Old AppImage menu entries | `packaging/linux/AppRun` | An AppImage's first run removes the `ai.storyteller.solvecraft.desktop` entry and icons that an AppImage up to 0.7 installed in `~/.local/share`. |
| `flatpak_twin` / `legacy_flatpak_path` | `crates/io/src/legacy_dir.rs` | No longer called: recovery adopts nothing. Test: `flatpak_twin_maps_our_sandbox_data_to_the_old_app_id`. |

## macOS

Releases before 0.4 were installed as `SolveCraft.app`. To update, quit the app, drag
`SolidCraft.app` from the DMG onto Applications, move `SolveCraft.app` to the Trash, and
replace any Dock shortcut with the old name (open `SolidCraft.app`, then
**Options > Keep in Dock**). Designs and settings are kept (see 4).

## Flatpak remotes added from the former website address

The feed has moved twice: from the SolveCraft address, then from `bherbruck.github.io/solidcraft`
when the project moved to `docs.solidcraft.io` (#1920). A remote added from either address no
longer finds updates. Point it at the current one:
`flatpak remote-modify --user --url=https://docs.solidcraft.io/flatpak/repo/ solidcraft`
(use your remote's name: `solvecraft` for one added before 0.4), then `flatpak update --user`.

## Draft design containers

The #303 draft used `solvecraft-container/1`. `crates/io/src/design.rs` accepts that
marker alongside `solidcraft-container/1`, and writes only the new marker. Optional
old previews are discarded; required document/blob validation is unchanged. The
`draft_container_marker_remains_readable` regression covers this shim.
