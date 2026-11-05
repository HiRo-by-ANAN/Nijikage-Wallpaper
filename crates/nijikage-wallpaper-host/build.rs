// 把 app.manifest（含 Windows 8 以上 supportedOS 相容性宣告）嵌入執行檔。
// 分層子視窗（WS_EX_LAYERED child）在新版桌面上需要此宣告才會被合成顯示。
fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("app.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
