# 安全性政策 / Security Policy

## 回報安全問題

> 本專案由個人利用課餘時間維護，無法保證固定的回覆或修正時程，但會盡力處理。 / Maintained by one person in their spare time; response times cannot be guaranteed, but reports will be handled as best we can.

如果你發現 Nijikage-Wallpaper 有安全問題（例如權限、檔案存取、音訊擷取、安裝程式或掛載行為相關的弱點），請**不要**直接在公開的 Issues 貼出細節。請使用 GitHub 的「Report a vulnerability」（Security → Advisories）私下回報；若該功能不可用，請開一個只寫「需要私下回報安全問題」的 Issue，不附細節，我們會再提供聯絡方式。

If you find a security problem, please do not post details in a public issue. Use GitHub's "Report a vulnerability" (Security → Advisories); if that is unavailable, open an issue that only says you need a private channel, without details.

## 本專案的安全設計

- 不連網、無遙測，不收集或上傳資料。
- 音訊動態環只在記憶體計算音量與頻帶，不錄音、不儲存、不上傳；關閉時不啟動擷取。
- 只寫入 `%APPDATA%\Nijikage-Wallpaper\`（設定與紀錄）；啟用「開機自啟動」時寫入 `HKCU\...\Run`。
- 只讀取使用者在設定中選擇的圖片。
- MSI 為每位使用者安裝，不需要系統管理員權限。

## 支援的版本

目前只有最新的 Beta 版本會收到修正。
