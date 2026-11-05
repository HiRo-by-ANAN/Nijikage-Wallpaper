# Nijikage-Wallpaper 架構說明

本文件說明 Nijikage-Wallpaper 的程式結構、Windows 11 新版桌面的掛載原理，以及診斷與測試方式。

## 專案結構

```
crates/
  nijikage-wallpaper-core/       共用：設定格式（Config）、路徑、多語系（i18n）、圖片前處理
  nijikage-wallpaper-render/     GPU 繪製（wgpu）：背景、光碟、光學效果、粒子、音訊環；不碰視窗與設定檔
  nijikage-wallpaper-host/  桌布主程式：桌面掛載、游標輪詢、托盤、音訊擷取、設定熱重載、開機自啟動
  nijikage-wallpaper-settings/   設定視窗（egui／eframe）
assets/               圖示
docs/                 文件與示意圖
packaging/            MSI 打包（WiX）
```

兩個執行檔之間**只透過設定檔 `%APPDATA%\Nijikage-Wallpaper\config.json` 溝通**：設定視窗寫入、桌布程式每 500 ms 檢查修改時間並重新載入。這讓兩個程式可以各自獨立開發、重啟與當掉而不互相影響。

### 設定檔契約（`nijikage-wallpaper-core`）

- 欄位只增不改名；新增欄位一律 `#[serde(default)]`，舊設定檔才讀得進來；未知欄位會被忽略。
- `Config::try_load()` 區分「檔案不存在」（回傳預設值）與「讀取或解析失敗」（回傳錯誤）。呼叫端遇到錯誤應保留舊設定、稍後重試，**不可用預設值覆蓋使用者的檔案**。
- 存檔採「先寫暫存檔（含行程 ID）再改名」，避免另一個程式讀到寫到一半的檔案。
- `Config::sanitize()` 把手改或損壞的數值限制在合理範圍，NaN／無限大退回預設值。

### 繪製器介面（`nijikage-wallpaper-render`）

```rust
Renderer::new(device, queue, format, size, &config)
Renderer::resize(device, queue, size)
Renderer::apply_config(device, queue, &config)   // 圖片只在來源（路徑＋修改時間）改變時才重讀
Renderer::set_audio(&AudioInput)                 // 音量與 16 個頻帶
Renderer::render(device, queue, &view, &FrameInput { time, dt, cursor, size })
```

繪製器不持有視窗或設定檔，宿主程式負責建立 wgpu 裝置與表面。離屏測試（`tests/`）直接把結果讀回成 PNG 檢查。

### 光學模型

- **光碟模式**：盤面座標以光碟直徑為 1、圓心為原點。圖片取樣時先減位移、反向旋轉、再除以縮放；`art_zoom = 1` 時圖片**短邊等於光碟直徑**。圖片範圍外與光碟圓外不顯示，圖片不足處顯示盤面本色。中心沒有洞，同心細紋在中心平滑收斂。
- **全桌面模式**：不畫光碟，背景當作盤面，疊加同心細紋高光、彩虹繞射色帶、慢速掃過的光帶與游標光源（含局部折射）。強度為 0 時與原背景逐像素相同。
- **音訊環**：`audio_ring.enabled` 且有聲音時，在光碟最外緣畫彩虹色動態環；頻帶沿圓周對稱排列，聲音停止約 0.5 秒淡出，無聲時完全不繪製。

## 在 Windows 11 新版桌面掛載桌布視窗

較新的 Windows 11（24H2 之後，實測 26H2 build 26300）改變了桌面視窗結構（Microsoft 稱為 "raised desktop with layered ShellView"）：

```
Progman            style 0x96000000, ex 0x200080（含 WS_EX_NOREDIRECTIONBITMAP）
  ├─ SHELLDLL_DefView   ex 0x80000（WS_EX_LAYERED）  ← 桌面圖示，大部分透明
  ├─ （我們的視窗）      WS_CHILD, WS_EX_LAYERED      ← 放在 DefView 之下、WorkerW 之上
  └─ WorkerW            系統桌布層
```

Microsoft 對這個結構的說明（Lively 原始碼中引用）：第三方桌布需自行建立 `WS_EX_LAYERED` 子視窗，放在 DefView 之下、WorkerW 之上，並設定 `SetLayeredWindowAttributes(bAlpha=0xFF)`。

**實測發現**：只照上面的結構掛載並不夠。我們的視窗在「`SetParent` 之前」設定分層，掛載後狀態會被系統重置，結果視窗存在、可見、持續 present，但畫面不會被合成到桌面。可顯示的順序（`NIJIKAGE_WALLPAPER_ATTACH_MODE=1`，預設）是：

1. 視窗先保持隱藏，設成乾淨的 `WS_CHILD` 樣式；
2. `SetParent(hwnd, Progman)`；
3. **掛載之後**重新設定 `WS_EX_LAYERED` 與 `SetLayeredWindowAttributes(255)`；
4. `SetWindowPos(..., SWP_FRAMECHANGED)` 並放在 DefView 之後；確保 WorkerW 在 Progman 子視窗的最底；
5. 等第一幀繪製完成後才顯示視窗。

另外，執行檔需內嵌 Windows 8 以上的相容性 manifest（`supportedOS`）。建置時 MinGW 連結器會出現 `.rsrc merge failure: multiple non-default manifests` 的警告：Rust 標準函式庫預設也會嵌入一份 manifest（含 `longPathAware`），與我們的並存；實測 `GetVersionExW` 回報 10.0，表示我們的相容性宣告有生效，此警告可忽略。

> **測試桌面畫面的注意事項**：GDI 的 `BitBlt`／.NET `Graphics.CopyFromScreen` **抓不到**這種桌面層的合成內容，會讓你誤以為程式沒有顯示。請使用 DXGI Desktop Duplication（例如 `ffmpeg -f lavfi -i "ddagrab=0,hwdownload,format=bgra" -frames:v 1 out.png`）或螢幕錄影。

### 診斷用環境變數

| 變數 | 作用 |
|---|---|
| `NIJIKAGE_WALLPAPER_ATTACH_MODE=0..4` | 0＝舊掛載順序（對照組）；1＝預設；2＝先在螢幕外頂層視窗 present 再掛載；3＝繞過 HWND swapchain，GPU 離屏繪製後以 `UpdateLayeredWindow` 呈現；4＝方案 1 加上首幀後短暫 hide/show DefView |
| `NIJIKAGE_WALLPAPER_GDI_TEST=1` | 用純 GDI 在掛載後的視窗畫紅色，用來區分「視窗沒被合成」與「GPU 路徑有問題」 |
| `NIJIKAGE_WALLPAPER_ZTOP=1` | 把視窗提到 Progman 子視窗最上層（除錯用） |
| `WGPU_BACKEND=dx12\|vulkan\|gl` | 指定 wgpu 後端 |
| `--windowed`（參數） | 以一般視窗執行，不掛載到桌面 |

啟動時的實際選項與掛載過程會寫入 `%APPDATA%\Nijikage-Wallpaper\log.txt`。

## 音訊擷取

`nijikage-wallpaper-host/src/audio.rs` 以 WASAPI loopback 擷取預設輸出裝置的混音，轉成單聲道後做 1024 點 FFT（Hann 窗），取整體音量與 60 Hz–12 kHz 的 16 個對數頻帶（皆為 0–1，快速上升、緩慢下降）。取樣只存在於一個小環形緩衝，不寫檔、不上傳。`audio_ring.enabled = false` 時不會建立擷取執行緒。預設輸出裝置被切換或失效時會自動重試。

## 多語系

`nijikage_wallpaper_core::i18n` 以**繁體中文原文當鍵**：程式碼寫 `tr("背景")`，英文與日文對照表在 `crates/nijikage-wallpaper-core/src/i18n/en.rs`、`ja.rs`。目前語言是繁中或表中查不到時直接回傳原文。含變數的句子用 `tr_fmt("已載入設定檔：{}", &[path])`（各語言版本的 `{}` 數量必須相同）。語言由設定 `language`（`Auto`／`ZhTw`／`En`／`Ja`）決定，`Auto` 依 Windows 顯示語言。新增字串時，請同時補上兩張表；`nijikage-wallpaper-core` 的測試會檢查表中沒有重複鍵、沒有空翻譯、`{}` 數量一致。

## 測試

- `cargo test --release`：單元測試（設定、i18n、音訊平滑）與離屏 GPU 測試（需要可用的 GPU；以 Vulkan／DX12 離屏繪製並讀回像素）。
- 渲染測試會輸出 PNG 到 `crates/nijikage-wallpaper-render/target/`，並以量化指標驗收，例如全桌面模式在強度 1.0／0.7／0.3 時與原圖的差異像素比例與 PSNR、強度 0 時逐像素相同、游標在畫面外不產生透鏡。
- 測試用圖由程式產生（`tests/support/mod.rs`），不依賴外部檔案。
- `cargo test --release -p nijikage-wallpaper-render --test desktop_performance -- --ignored` 量測 30 fps 預算（需獨佔 GPU）。
