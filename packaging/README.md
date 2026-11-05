# 打包 / Packaging

## 建置 MSI 與免安裝 ZIP

需要：

- Rust 工具鏈（見根目錄 README）；
- [WiX Toolset 3.x](https://github.com/wixtoolset/wix3/releases)（例如 `wix314-binaries.zip`，解壓縮即可，不需要安裝）。

```powershell
powershell -ExecutionPolicy Bypass -File packaging\build-msi.ps1 `
    -WixBin C:\tools\wix314 -Version 0.1.0 -Label 0.1.0-beta.1
```

產物在 `dist\`：

| 檔案 | 說明 |
|---|---|
| `Nijikage-Wallpaper-<版本>-x64.msi` | 安裝檔（每位使用者安裝到 `%LOCALAPPDATA%\Programs\Nijikage-Wallpaper`，不需要系統管理員權限） |
| `Nijikage-Wallpaper-<版本>-x64.zip` | 免安裝版 |
| `SHA256SUMS.txt` | 檔案雜湊，供使用者核對 |

## MSI 的行為

- **安裝位置**：`%LOCALAPPDATA%\Programs\Nijikage-Wallpaper\`；開始功能表建立 **Nijikage-Wallpaper** 與 **Nijikage-Wallpaper Settings** 兩個捷徑。
- **升級**：相同 UpgradeCode 的新版本會直接取代舊版；安裝前會結束執行中的 `nijikage-wallpaper.exe`／`nijikage-wallpaper-settings.exe`。
- **解除安裝**：移除檔案與捷徑，並呼叫 `nijikage-wallpaper.exe --cleanup` 移除「開機自啟動」的登錄值（`HKCU\...\Run\Nijikage-Wallpaper`）。**不會刪除** `%APPDATA%\Nijikage-Wallpaper\` 內的設定檔與紀錄檔，想完全清除請手動刪除該資料夾。
- **需求**：64 位元 Windows 10 以上（建議 Windows 11）。
- 安裝程式介面為英文；程式本身的介面語言預設依系統顯示語言，可在設定視窗中切換。

## 注意

- 目前的 MSI 與執行檔**沒有數位簽章**，Windows SmartScreen 可能顯示警告。
- `packaging/wix/license.rtf` 由 `build-msi.ps1` 依 `LICENSE` 自動產生，不需要手動維護。
