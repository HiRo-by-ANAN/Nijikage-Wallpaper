# 參與貢獻 / Contributing

感謝你對 Nijikage-Wallpaper 有興趣！歡迎回報問題、提出想法、改善翻譯或送出程式碼。 / Thanks for your interest! Bug reports, ideas, translation fixes and pull requests are all welcome.

> 本專案由個人利用課餘時間維護，無法保證固定的回覆或修正時程。 / This project is maintained by one person in their spare time; no fixed response or fix schedule can be guaranteed.

## 回報問題

請在 Issues 中附上：

- Windows 版本（`winver` 顯示的版本與組建編號）、顯示卡型號與解析度；
- 重現步驟與預期行為；
- `%APPDATA%\Nijikage-Wallpaper\log.txt` 的相關內容（貼出前請檢查是否含有你不想公開的路徑或名稱）；
- 若是「桌布不顯示」：請先確認不是用 GDI 截圖（`CopyFromScreen`）造成的誤判，見 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 開發環境

- Rust 1.85 以上（edition 2024），目前使用 `x86_64-pc-windows-gnu`（MinGW）工具鏈。
- 離屏 GPU 測試需要可用的 GPU。

```bash
cargo build --release
cargo test --release
cargo fmt --all
cargo clippy --workspace --all-targets
```

## 送出 Pull Request

1. 在新的分支上修改，保持每個 PR 聚焦在單一主題。
2. 新增或修改使用者看得到的文字時，請同時更新 `crates/nijikage-wallpaper-core/src/i18n/en.rs` 與 `ja.rs`。
3. 動到設定格式（`nijikage-wallpaper-core` 的 `Config`）時：欄位只增不改名，新欄位加上 `#[serde(default)]`，並維持 `sanitize()` 與測試。
4. 動到渲染時，請附上前後對照圖，並確認 `cargo test --release` 通過。
5. 請勿加入個人資料、私人圖片或有版權疑慮的素材；測試與示意圖請使用程式產生的內容。

## 授權

送出貢獻即表示你同意以 **GPL-3.0-or-later** 授權你的貢獻。
