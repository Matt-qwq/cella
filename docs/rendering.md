# Rendering

> **Last reviewed:** 2026-11-26 · **Last updated:** 2026-11-26 · **Change:** new page — the renderer
> became selectable, inspectable and crash-aware on 11-26 (DirectX 12 back as the Windows default
> after AMD's OpenGL driver crashed a session); this page is the one place that says which backend
> Cella uses, why, how to change it, and what to do when a driver misbehaves.
> **Target:** Microsoft Excel parity, and a window that keeps drawing when a driver does not.

Everything Cella draws goes through **wgpu** (the Rust graphics abstraction), and wgpu talks to the
system through a *backend* — one of the platform's graphics APIs. The backend is not a preference in
the abstract: it is which driver code runs under the window, so it decides both how much memory the
window holds and whether a driver bug can take the process down.

## The backends

| Backend | Where it is used | Notes |
|---|---|---|
| **DirectX 12** | **Windows default** | Microsoft's own API, validated with every vendor. The most-tested road on Windows, and the heaviest in memory |
| **OpenGL** | Windows fallback; Linux default | The oldest of the three. Works on chips whose driver has no DirectX 12 (and in virtual machines), saves memory, and has the weakest driver quality control — AMD's Windows OpenGL driver is the one that crashed (§ "Why DirectX 12") |
| **Vulkan** | Linux, Android; selectable on Windows | The cross-platform modern API. Never chosen automatically on Windows: Intel's Vulkan driver has crashed here (`igvk64.dll`, #120) and a screenshot readback hung in an older build |
| **Metal** | macOS (the only one) | Not a choice; eframe uses it |
| **WebGPU / WebGL2** | The browser build | The browser decides; there is no setting |

`WARP` — DirectX 12's software rasterizer — is kept in the candidate list for machines whose DirectX
12 offers nothing but a software adapter, so the window still appears without an OpenGL driver
either. It is not a user-facing option (see "Not offered").

## What Cella picks on its own (自动)

`apps/cella/src/graphics.rs`:

1. A DirectX 12 probe runs first (a short-lived instance of its own, dropped before the real one).
2. **Usable hardware DirectX 12 GPU → DirectX 12.**
3. No DirectX 12 driver at all → **OpenGL** (the only road to a GPU).
4. DirectX 12 has a GPU whose device cannot be created → **OpenGL**.
5. DirectX 12 offers only WARP → **both** backends are offered, so a hardware OpenGL adapter wins and
   WARP is still there when there is no OpenGL driver either.
6. Automatic never picks Vulkan, and never picks Metal or WebGPU on a platform that is not theirs.

## Changing it

**File ▸ Options ▸ 图形 › 渲染器** (Options ▸ Graphics › Renderer) offers **自动 / DirectX 12 / OpenGL /
Vulkan**, and shows what this run is actually drawing with — `DirectX 12 · AMD Radeon RX 9070 GRE` —
under the row.

- The choice is saved to **`renderer.json`** in the settings folder (`%APPDATA%\Cella` on Windows,
  `~/Library/Application Support/Cella` on macOS, `~/.config/cella` elsewhere).
- It is read **before the window exists** (the backend has to be chosen before eframe creates the
  device), so a change applies on the **next start** — the page says so.
- The file is meant to be human-editable; a byte-order mark is skipped, and an unknown value reads as
  自动 rather than breaking the start.
- `WGPU_BACKEND=dx12|gl|vulkan` (and `WGPU_ADAPTER_NAME`) still override everything for a single run:
  an environment override is a debugging decision and nothing moves it, not even the crash rule below.
- The choice only exists in the desktop build; the browser's renderer is the browser's.

## When a driver crashes

A GPU driver can die under a process without any Rust code running — no error, no unwinding, no
chance to save. Cella cannot catch that, so it plans for it:

- A run writes **`session.json`** with the backend it actually got (`{"backend":"dx12"}`) as soon as
  the adapter is known, and **removes it on a clean exit**.
- A start that finds the file knows the previous run **died instead of quitting**.
- With 自动, it then takes the **other** road (DirectX 12 ↔ OpenGL) and says so in the info bar, with
  the backend that died and the one it is on now. A machine whose only road is OpenGL keeps it.
- An **explicit** choice (DirectX 12, OpenGL or Vulkan picked in Options) is kept even after a crash —
  the person asked for that backend — and the info bar says nothing changed and where to change it.
- Deleting `session.json` by hand is the reset: the next start is a normal one.
- A start that fails *before* the window exists (no adapter, no device, no surface) is a different
  path: eframe is run a second time with the other backend, which is why the first start of an
  unsupported configuration still opens a window.

## What the log says

`%APPDATA%\Cella\logs\cella.log` (path printed by the first line) is where a rendering problem becomes
measurement instead of a description:

```
renderer: Dx12 adapter "AMD Radeon RX 9070 GRE" (vendor 0x1002, device 0x7550, driver "32.0.31044.16" "")
graphics: choice Auto, 7 DirectX 12 adapter(s) [...]; backends Backends(DX12)
renderer: the last run did not close (it was on dx12); this one is on gl
graphics: choice Auto, 7 DirectX 12 adapter(s); backends Backends(GL) after a run that died on Dx12
opening D:\…\workbook.xlsx
opened D:\…\workbook.xlsx in 2011 ms
```

The first line comes from the adapter wgpu **actually selected** — the probe above only says what the
machine offers. There are also two lines for the "the window keeps redrawing" shape of trouble: at
sixty passes in a row that asked for another repaint with no input arrives
(`repainting every pass with no input for … s`), and again when the run ends
(`repaint storm ended after … passes in … s`). `WGPU_BACKEND` overrides are logged too.

## The numbers

Measured on 2026-11-26 on one Windows machine (an AMD Radeon RX 9070 GRE, 31 GB of RAM), the same
112.8 MB archival workbook open (23.6 M cells), `CELLA_FRAME_STATS=1` repainting continuously:

| Backend | Starts | Working set | Paint per frame | Frame interval | Screenshot readback |
|---|---|---:|---:|---:|---|
| DirectX 12 (default) | yes | 1,346 MB | 1.35–1.42 ms | 6.9 ms (144 Hz) | works |
| OpenGL | yes | **1,150 MB** | **1.07–1.12 ms** | 6.9–7.2 ms | works |
| Vulkan | yes | 1,207 MB | 1.14–1.21 ms | 6.9–7.0 ms | works |
| WARP (software) | — | — | — | — | **not verified**: Cella cannot currently report which adapter it picked, so a `WGPU_ADAPTER_NAME` run cannot be proven to have used WARP |

The idle window was audited earlier (2026-11-18, 70 MB workbook, on a hidden desktop so nothing was
put in front of anyone): 317 MB on DirectX 12, 240 on Vulkan and 173 on OpenGL, and DirectX 12 and
OpenGL paint at the same cost (1.41 against 1.38 ms a frame) — the numbers behind OpenGL having been
the default for a week (see [performance.md](performance.md) § "The idle window").

## Why DirectX 12, when OpenGL is faster and lighter here

Because speed was never the problem — the driver was. On 2026-11-26 AMD's OpenGL driver
(`atio6axx.dll`, 32.0.31044.16) took a session down with `0xc0000005` about ten seconds after that
112.8 MB workbook opened, and the window had been flashing before it — the *"screen flashing on
Windows 11"* (#76) the hardware page had tracked for months. The cost of avoiding it is the 196–213 MB
the table above shows. A crash outranks that; `WGPU_BACKEND=gl` still gives the lighter backend back
to anyone who wants it.

## Not offered, and why

- **A WARP (software rendering) option** — the last resort when every GPU driver is broken. It needs
  the adapter choice to be reported and selectable first (see the table's last row); about two hours
  of work, and it would be slow enough that the honest label is "for diagnosis".
- **Vulkan as a Windows default** — it runs here today, but the history above is Windows-specific and
  the automatic rule has no way to know a vendor's driver version is buggy.
- **Rendering in a separate process** (what browsers do, so a driver crash kills only the painter) —
  the only structural answer to driver crashes. Weeks of work, not hours; the marker above is the
  cheap version of the same promise.
