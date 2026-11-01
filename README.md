# VoidLauncher

A Windows-first Minecraft launcher written in Rust + egui.

## Current development build

VoidLauncher now uses `mc-launcher-core` to install Minecraft/Fabric profiles and build a direct Java launch command. It does not open the official Minecraft Launcher.

Implemented in the current branch:
- Separate instances with their own game folders.
- Fabric installation on first launch.
- Offline username launch.
- Ely.by password authentication and authlib-injector launch integration (experimental; must be tested against a real Ely.by account).
- Automatic Java runtime detection; if the required runtime is missing, the launcher downloads Eclipse Temurin JRE into the user's local app data.
- Java major-version selection for legacy releases and the 26.x release line.
- Configurable memory limit.
- Retry attempts for temporary Minecraft download network errors.
- Per-instance mods, resourcepacks, shaderpacks, and config folders.

## Build on Windows

GitHub Actions builds the Windows executable on every push to `main`.

1. Open the repository's **Actions** tab.
2. Open the latest successful **Build VoidLauncher for Windows** run.
3. Download the `VoidLauncher-Windows` artifact.
4. Extract the ZIP and run `voidlauncher.exe`.

## First launch

The first launch can take a while. The launcher may need to download a Java runtime, the Minecraft client, libraries, assets, and Fabric files. Keep the launcher open and make sure the connection to Mojang/Minecraft download hosts is stable.

## Important

This is still a development build. A successful Windows compilation proves that the program builds, not that every Minecraft version, Ely.by account, graphics driver, or network environment has been runtime-tested. Do not treat the Ely.by integration as verified until it has been tested with an actual account and a real multiplayer login.

Minecraft, Fabric, Eclipse Temurin, and Ely.by are third-party projects and are not affiliated with VoidLauncher.
