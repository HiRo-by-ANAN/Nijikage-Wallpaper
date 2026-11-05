//! 不建立 GPU 的 WGSL 語法、型別及管線介面檢查。
#[test]
fn validate_wgsl() {
    for source in [
        include_str!("../src/scene.wgsl"),
        include_str!("../src/present.wgsl"),
    ] {
        let module = naga::front::wgsl::parse_str(source).expect("WGSL 語法錯誤");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL 驗證失敗");
        let (_, ty) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Uniforms"))
            .expect("缺少統一參數");
        match ty.inner {
            naga::TypeInner::Struct { span, .. } => assert_eq!(span, 224),
            _ => panic!("統一參數必須為結構"),
        }
    }
}
