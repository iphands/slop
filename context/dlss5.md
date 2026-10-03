# DLSS 5 (3D-Guided Neural Rendering) — Linux state as of 2026-09-22

## What it is
- NGX feature **18** "DLSSNR". Model snippet `nvngx_dlssnr.dll` (~166 MB, ~3x DLSS 4 SR).
- **Not an upscaler.** A generative relight/material pass: 1 frame in, 1 frame out. Inputs are color + motion vectors
  (depth optional; the community routes mostly skip it). Ray-traced/path-traced input lighting gives better results.
- Launched 2026-09-01 in **NBA 2K27** (the only official title). Windows driver 616.64 WHQL. Official support: GeForce RTX 50 only.
- Cost (Digital Foundry, NBA 2K27): 5090 at 4K +~8 ms (128→63 fps); 5080 ~95→40; 5060 at 1080p 70→40. ~730 MB VRAM.
  Scales with output resolution: ~5 ms at ~1000x750 emulator resolution.
- Stylized art drifts: DF saw FF7 Rebirth faces "photoreal-ified" (pink lips, extra skin detail). 2D/sprite content gets nothing out of it.

## Linux blocker
- Driver dispatch of feature 18 returns `0xBAD0000C FAIL_OutOfDate`, "unavailable until driver 616.56+". The newest Linux driver is 615.71.09,
  and the NGX OTA updater isn't reachable under Proton. The 610.57.04 NGX (`/usr/lib64/nvidia/wine/{nvngx,_nvngx}.dll`,
  `libnvidia-ngx.so`) contains no dlssnr strings.
- Every working community route **loads the snippet directly** and bypasses driver dispatch.
- Ada: the stock model returns `FAIL_FeatureNotSupported`. Only a community-patched "RTX40" model build works (untested-model warnings).
- No 32-bit NGX runtime exists. 32-bit games need a 64-bit helper process.

## Routes
| Route | Covers | Motion vectors | Notes |
|---|---|---|---|
| `NapXDD/addon-dlssnr-linux` (ReShade add-on, GPL-3) | D3D12 under Proton, and the game must be running DLSS SR | real game MVs | Best quality. Hooks CreateFeature/EvaluateFeature and runs NR after each SR eval. Needs the exact model SHA-256 `e16bcf15e16e13f5…fc8e` (other builds crash silently after minutes). Uses the forwarder `nvngx.dll_nrfwd.dll` to pass the snippet's caller-name gate. GE/cachyos: `PROTON_FORCE_NVAPI=1 WINEDLLOVERRIDES="dxgi=n,b" %command%`. F10 toggles. Tested on 610.57.04. |
| `bmitch87/DLSS5VKLayer` (AGPL-3) | **any Vulkan swapchain**: native Linux, DXVK/VKD3D under Proton, Vulkan emulator backends | synthetic, `VK_NV_optical_flow`, no depth | Implicit layer that sends frames over shm/dma-buf IPC to a Windows NGX helper under Wine/Proton-GE/CachyOS. Valve Proton isn't supported (no dxvk-nvapi). `VKLayer_DLSS5=1`; `dlssnr-helper init/doctor/start/status/stop`; `dlssnr-gui`; config at `$XDG_CONFIG_HOME/dlssnr/config.ini`; model at `$XDG_DATA_HOME/dlssnr/binaries`. With Smooth Motion set `VK_INSTANCE_LAYERS=VK_LAYER_NV_dlssnr:VK_LAYER_NV_present`. Needs permissive ptrace. No 2nd-GPU offload. Published 2026-09-08, still immature (open issues: "can't make it work" x2, DEVICE_LOST/Xid 13 on a 5060 Ti). |
| `NIGos/dlss5-bridge` (ReShade, MIT) | D3D11 / Vulkan mirrored into a private D3D12 session | the game's DLSS inputs, or OF synth (`synth=1`) | Needs the closed RenoDX `renodx-dlss5.addon64` (Discord). |
| DLSS5-Feeder (+ Mesa Zink for GL) | games without DLSS, Windows emulator builds | ReShade depth + MV shaders | Per `kamalzakaria/dlss5-on-linux`: DuckStation D3D11 hits 60 fps; D3D12 goes black. Direct GL is **blocked** because Wine advertises `GL_EXT_semaphore_win32` but NV only implements the fd variants. Workaround: Zink `opengl32.dll` proxy. |

## OpenGL
There's no direct route. Switch the app to a Vulkan backend, or run GL through Zink so a Vulkan-layer route can see it.

## Model provenance and risk
- The only clean source is an official DLSS 5 game install (NBA 2K27). Circulating copies are the leaked NBA 2K27 early-access build.
- "DLSS 5 download" sites, SourceForge projects and random GitHub orgs are malware bait.
- The one-click "DLSS5 Swapper" tools are Windows-first. The Linux fork ships no binaries and has vague component sourcing.
- DLL injection into anti-cheat games (VAC/EAC/BattlEye) risks bans.
