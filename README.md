# VoidLauncher

Windows desktop launcher prototype written in Rust + egui.

## Download the Windows build

GitHub Actions builds a Windows executable on every push to `main` and on manual dispatch.

1. Open the **Actions** tab.
2. Select **Build VoidLauncher for Windows**.
3. Open the latest successful run.
4. Download and extract the `VoidLauncher-Windows` artifact.
5. Run `voidlauncher.exe`.

## Included in this early build

- Dark purple desktop interface.
- Fabric installer helper for the selected Minecraft version.
- Import/open folders for Fabric mods, resource packs, and shader packs.
- Saves basic launcher settings locally.

## Not implemented yet

This is not yet a complete standalone game launcher:
- The app opens the official Minecraft Launcher to start the game; it does not yet resolve libraries or launch Minecraft itself.
- Ely.by OAuth/session authentication and skin display are not implemented. The app links to the official Ely.by website; never enter an Ely.by password into this prototype.
- RAM setting is stored but is not yet passed to a custom game launch command.
- Mod and pack activation is still handled in Minecraft or via the file manager.
- Iris is not installed automatically.

Minecraft, Fabric, and Ely.by are third-party projects and are not affiliated with VoidLauncher.
