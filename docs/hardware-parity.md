# Hardware parity

> **Last reviewed:** 2026-11-26 · **Last updated:** 2026-11-26 · **Change:** minor (**Windows renders on DirectX 12 again**: AMD's OpenGL driver (`atio6axx.dll`, `0xc0000005`) crashed a session on a 112.8 MB workbook and the *screen flashing on Windows 11* (#76) this page tracked is that driver failing; the adapter and driver are logged at every start, File ▸ Options ▸ 图形 › 渲染器 picks 自动 / DirectX 12 / OpenGL / Vulkan, and a run that dies leaves a marker so the next start takes the other backend — [rendering.md](rendering.md)) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

The hardware Excel uses, per platform, against Cella. Part of
[target-app-parity.md](target-app-parity.md). **Hardware: ~40% ready, 25–40 h** (estimated).

| Hardware feature | Excel | Cella (macOS · Windows · Linux · BSD · web) | Gap and estimate |
|---|---|---|---|
| Multi-core recalculation | Multi-threaded recalculation on all cores (Windows and Mac), configurable thread count | **Parallel for independent formulas** since the 10-27/28 rounds (up to 8 shards a wave, sequential when a whole-column precedent, a dynamic or array formula in the dirty set, or iterative calculation makes it unsafe; a 100k-dependent edit is 20–25 ms). No thread-count setting; a fully sequential chain stays sequential (by nature). WASM is single-threaded | A thread-count option and better load balance on mixed dirty sets: 4–6 h of the original 12–18 |
| GPU rendering of the grid and charts | DirectX / Metal accelerated drawing | wgpu: Metal · **DirectX 12 is the Windows default since 11-26** — OpenGL was, from 11-18, for 183 MB less memory, until the AMD OpenGL driver (`atio6axx.dll`) crashed on a 112.8 MB archival workbook and the "screen flashing on Windows 11" of #76 found its likely cause; OpenGL stays as the retry path and the only backend for a machine whose DirectX 12 has no usable GPU, and `WGPU_BACKEND=gl` still asks for the lighter one (`choose_backends` carries the measurements; Vulkan only on request, so the Intel UHD `igvk64.dll` crash (#120) can't happen; DirectX 12 shaders compiled with FXC, so no DXC runtime is needed, #339) · Vulkan/GL · Vulkan/GL · WebGPU or WebGL2 | At parity in kind. #76 is addressed by the backend change; a crash in another vendor's driver is still worth a report, and the adapter is now logged (`renderer: <backend> adapter … driver …`) so one is diagnosable. File › Options › 图形 › 渲染器 picks 自动 / DirectX 12 / OpenGL / Vulkan, and a run that dies in its driver leaves a marker that makes the next start take the other road |
| Pen and touch (Draw tab) | Pen pressure and tilt, eraser end, touch mode, Ink to Shape, ink replay | Pen, highlighter, eraser, Ink to Shape with mouse/pen input; **no pressure or tilt**, no touch mode | Pressure-sensitive strokes where winit reports force; touch-friendly ribbon spacing: 4–6 h |
| Trackpad and mouse | Smooth scrolling, pinch zoom, horizontal scroll, Magic Mouse | Smooth scrolling and zoom via egui | Verify pinch zoom on each platform: 1–2 h |
| Multiple windows and monitors | One window per workbook, New Window for the same workbook, per-monitor DPI | One window; multiple workbooks switch inside it (#162; PR #171) | Native window per workbook, New Window, Arrange All across monitors: 6–8 h |
| High-DPI and display scaling | yes | yes (display scale matched to desktop density; View › Interface size 75–200 % on top, #230) | — |
| Accessibility hardware (screen readers, braille displays, switch control) | Narrator, JAWS, NVDA, VoiceOver | AccessKit enabled in eframe; grid cells and ribbon not yet exposed as an accessible tree | Counted under UI ([ui-parity.md](ui-parity.md)) |
| Printers | System print with driver options | Desktop submits through craft-print with printer choice, copies, duplex and grayscale; web browser printing | Counted under print |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-26 | minor | The renderer is inspectable and selectable: the adapter, vendor and driver are logged at start, File › Options › 图形 › 渲染器 picks the backend (restart to apply), and `session.json` — written at start, removed on a clean quit — makes the start after a driver crash take the other backend and say so. [rendering.md](rendering.md) is the page for it |
| 2026-11-26 | minor | Windows: **DirectX 12 is the default renderer again** - the AMD OpenGL driver (`atio6axx.dll` 32.0.31044.16) crashed a session with a 112.8 MB workbook open (`0xc0000005`), and the window flashing the owner reported before it (#76) is the same driver failing; costs ~213 MB with that workbook open (1,270 against 1,057 MB), and `WGPU_BACKEND=gl` still asks for OpenGL |
| 2026-11-18 | trivial | Windows: **OpenGL is the default renderer** - the same 1.4 ms a frame at 144 Hz as DirectX 12 with 183 MB less working set (performance.md); DirectX 12 is the retry path and stays in the list on WARP-only machines |
| 2026-10-12 | minor | Desktop File > Print now discovers printers and submits PDF jobs through pinned craft-print with printer choice, copies, duplex and grayscale; browser printing and PDF export remain available. Remaining print parity includes page ranges and driver-specific tray/options UI; physical printer fidelity needs hardware validation. |
| 2026-10-11 | trivial | Windows: DirectX 12 shaders compiled with FXC, no DXC runtime needed (#339, #354) |
| 2026-10-11 | trivial | Windows: OpenGL fallback when DirectX 12 has no GPU or fails to start; Vulkan never by default (#120) |
| 2026-10-10 | major | Created: single-threaded calc, wgpu rendering, no pen pressure, single window |
