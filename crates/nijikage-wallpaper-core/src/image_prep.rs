//! 圖片前處理模組：負責讀取完整原圖（保留長寬比）或置中裁切正方形，以及產生光碟盤面色調的 CPU 預覽圖。

use crate::{
    DiscStyle,
    i18n::{tr, tr_fmt},
};
use image::{DynamicImage, Rgba, RgbaImage, imageops::FilterType};
use std::path::Path;

/// 預設的最大邊長上限（像素）。
const DEFAULT_MAX_SIDE: u32 = 2048;

/// 讀取完整圖片並保留原始長寬比，最長邊不超過 `max_side`（若傳入 0 則採用預設值 2048）。
/// 讀取失敗、格式不支援或圖片尺寸無效時回傳繁體中文錯誤訊息，絕不 panic。
pub fn load_full(path: &Path, max_side: u32) -> Result<RgbaImage, String> {
    let decoded = decode_image_with_limits(path)?;
    resize_full(&decoded, max_side)
}

/// 將已解碼的圖片保留原始長寬比縮放，使最長邊不超過 `max_side`（0 表示預設值 2048）。
pub fn resize_full(img: &DynamicImage, max_side: u32) -> Result<RgbaImage, String> {
    let width = img.width();
    let height = img.height();
    if width == 0 || height == 0 {
        return Err(tr("圖片尺寸無效：寬度或高度為 0").to_string());
    }

    let limit = if max_side == 0 {
        DEFAULT_MAX_SIDE
    } else {
        max_side
    };

    let max_dim = width.max(height);
    let processed = if max_dim > limit {
        let (new_w, new_h) = if width >= height {
            let h = ((height as u64 * limit as u64 + (width as u64 / 2)) / width as u64).max(1) as u32;
            (limit, h)
        } else {
            let w = ((width as u64 * limit as u64 + (height as u64 / 2)) / height as u64).max(1) as u32;
            (w, limit)
        };
        img.resize_exact(new_w, new_h, FilterType::Triangle)
    } else {
        img.clone()
    };

    Ok(processed.to_rgba8())
}

/// 讀圖並置中裁成正方形，邊長不超過 `max_side`（若傳入 0 則採用預設值 2048）。
/// 讀取失敗、格式不支援或圖片尺寸無效時回傳繁體中文錯誤訊息，絕不 panic。
pub fn load_square(path: &Path, max_side: u32) -> Result<RgbaImage, String> {
    let decoded = decode_image_with_limits(path)?;
    crop_and_resize_square(&decoded, max_side)
}

/// 開啟並解碼圖片檔案，套用記憶體與尺寸安全上限。
fn decode_image_with_limits(path: &Path) -> Result<DynamicImage, String> {
    let path_str = path.display().to_string();
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| tr_fmt("無法開啟圖片檔案「{}」：{}", &[&path_str, &e.to_string()]))?
        .with_guessed_format()
        .map_err(|e| tr_fmt("無法判別圖片格式「{}」：{}", &[&path_str, &e.to_string()]))?;

    // 設定合理的記憶體與尺寸上限，避免損毀或超大圖片造成記憶體耗盡
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);

    reader
        .decode()
        .map_err(|e| tr_fmt("無法解碼圖片「{}」：{}", &[&path_str, &e.to_string()]))
}

/// 將已解碼的圖片置中裁切為正方形，並將邊長限制在 `max_side` 以內。
pub fn crop_and_resize_square(img: &DynamicImage, max_side: u32) -> Result<RgbaImage, String> {
    let width = img.width();
    let height = img.height();
    if width == 0 || height == 0 {
        return Err(tr("圖片尺寸無效：寬度或高度為 0").to_string());
    }

    let side = width.min(height);
    let offset_x = (width - side) / 2;
    let offset_y = (height - side) / 2;
    let cropped = img.crop_imm(offset_x, offset_y, side, side);

    let limit = if max_side == 0 {
        DEFAULT_MAX_SIDE
    } else {
        max_side
    };

    let processed = if side > limit {
        cropped.resize_exact(limit, limit, FilterType::Triangle)
    } else {
        cropped
    };

    Ok(processed.to_rgba8())
}

/// CPU 版「光碟盤色調」預覽，供設定視窗顯示；最終畫面以 GPU 著色器為準。
///
/// 依 [`DiscStyle`] 將圖片轉換為對應的光碟金屬色調：
/// - [`DiscStyle::Cd`]：銀白基調，帶微幅冷銀光澤與彩虹繞射偏振感
/// - [`DiscStyle::Dvd`]：金紫雙色調（暗部深紫、亮部暖金）
/// - [`DiscStyle::Bd`]：藍紫雙色調（暗部靛紫、亮部寶藍）
///
/// 轉換過程同時保留原圖的明暗層次與色彩細節，不會退化為單一平塗色。
pub fn disc_tone_preview(img: &RgbaImage, style: DiscStyle) -> RgbaImage {
    let width = img.width();
    let height = img.height();
    let mut out = RgbaImage::new(width, height);

    if width == 0 || height == 0 {
        return out;
    }

    let cx = (width as f32 - 1.0) * 0.5;
    let cy = (height as f32 - 1.0) * 0.5;
    let inv_radius = 1.0 / cx.hypot(cy).max(1.0);

    for (x, y, pixel) in img.enumerate_pixels() {
        let [r_u8, g_u8, b_u8, a_u8] = pixel.0;
        let r = r_u8 as f32 / 255.0;
        let g = g_u8 as f32 / 255.0;
        let b = b_u8 as f32 / 255.0;

        // 計算相對亮度（Rec. 709）並保留對比曲線
        let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
        let curved = luma.powf(0.88);

        // 計算相對圓心的極座標，用於模擬微細光碟同心紋與角度光澤（中心平滑收斂以避免奇異點）
        let dx = (x as f32 - cx) * inv_radius;
        let dy = (y as f32 - cy) * inv_radius;
        let radius = dx.hypot(dy).clamp(0.0, 1.42);
        let center_smooth = (radius * 4.0).clamp(0.0, 1.0);
        let angle = dy.atan2(dx);
        let sheen = 0.025 * center_smooth * (angle * 2.0 + radius * 6.0).cos();

        let (toned_r, toned_g, toned_b) = match style {
            DiscStyle::Cd => {
                // CD：銀白金屬基調（暗部冷銀灰、亮部銀白）＋微幅彩虹色散＋原色細節保留
                let shadow = [0.17_f32, 0.20, 0.25];
                let mid = [0.54_f32, 0.58, 0.63];
                let highlight = [0.93_f32, 0.96, 1.00];

                let base = sample_three_stops(shadow, mid, highlight, curved);
                let irid = [
                    0.022 * center_smooth * (angle * 2.0 + radius * 8.0).cos(),
                    0.022 * center_smooth * (angle * 2.0 + radius * 8.0 + 2.094).cos(),
                    0.022 * center_smooth * (angle * 2.0 + radius * 8.0 + 4.188).cos(),
                ];
                let detail = [r * 0.92, g * 0.96, b * 1.02];

                (
                    base[0] * 0.74 + detail[0] * 0.26 + irid[0] + sheen * 0.4,
                    base[1] * 0.74 + detail[1] * 0.26 + irid[1] + sheen * 0.4,
                    base[2] * 0.74 + detail[2] * 0.26 + irid[2] + sheen * 0.5,
                )
            }
            DiscStyle::Dvd => {
                // DVD：金紫雙色調（暗部深紫、中調紫金、亮部暖金）＋原色細節保留
                let shadow = [0.22_f32, 0.08, 0.30];
                let mid = [0.62_f32, 0.38, 0.46];
                let highlight = [0.97_f32, 0.80, 0.40];

                let base = sample_three_stops(shadow, mid, highlight, curved);
                let detail = [r * 1.06, g * 0.84, b * 0.76];

                (
                    base[0] * 0.76 + detail[0] * 0.24 + sheen * 0.6,
                    base[1] * 0.76 + detail[1] * 0.24 + sheen * 0.4,
                    base[2] * 0.76 + detail[2] * 0.24 - sheen * 0.3,
                )
            }
            DiscStyle::Bd => {
                // 藍光：藍紫雙色調（暗部深靛紫、中調藍紫、亮部亮藍紫）＋原色細節保留
                let shadow = [0.13_f32, 0.10, 0.32];
                let mid = [0.35_f32, 0.42, 0.78];
                let highlight = [0.68_f32, 0.80, 1.00];

                let base = sample_three_stops(shadow, mid, highlight, curved);
                let detail = [r * 0.76, g * 0.84, b * 1.12];

                (
                    base[0] * 0.75 + detail[0] * 0.25 + sheen * 0.3,
                    base[1] * 0.75 + detail[1] * 0.25 + sheen * 0.4,
                    base[2] * 0.75 + detail[2] * 0.25 + sheen * 0.6,
                )
            }
        };

        out.put_pixel(
            x,
            y,
            Rgba([
                to_u8(toned_r),
                to_u8(toned_g),
                to_u8(toned_b),
                a_u8,
            ]),
        );
    }

    out
}

/// 三段式色階插值（0.0 為暗部、0.5 為中間調、1.0 為亮部）。
fn sample_three_stops(shadow: [f32; 3], mid: [f32; 3], highlight: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        let k = t * 2.0;
        [
            shadow[0] + (mid[0] - shadow[0]) * k,
            shadow[1] + (mid[1] - shadow[1]) * k,
            shadow[2] + (mid[2] - shadow[2]) * k,
        ]
    } else {
        let k = (t - 0.5) * 2.0;
        [
            mid[0] + (highlight[0] - mid[0]) * k,
            mid[1] + (highlight[1] - mid[1]) * k,
            mid[2] + (highlight[2] - mid[2]) * k,
        ]
    }
}

/// 將 `0.0..=1.0` 的浮點色彩分量安全轉換為 `u8`。
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 以程式產生一張具有明暗梯度與多色細節的小圖（不依賴外部檔案）。
    fn make_synthetic_image(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width.max(1)) as u8;
                let g = ((y * 255) / height.max(1)) as u8;
                let b = (((x + y) * 127) / (width + height).max(1)) as u8;
                let a = if (x + y) % 7 == 0 { 180 } else { 255 };
                img.put_pixel(x, y, Rgba([r, g, b, a]));
            }
        }
        img
    }

    #[test]
    fn test_load_full_missing_file_and_aspect_ratio() {
        let missing = Path::new("__non_existent_nijikage_test_full_12345__.png");
        assert!(load_full(missing, 512).is_err(), "讀取不存在的檔案應回傳 Err");

        // 產生 100x60 的長方形圖，驗證 load_full 保留原始長寬比且不裁切
        let rect_img = make_synthetic_image(100, 60);
        let temp_path = std::env::temp_dir().join(format!(
            "nijikage_test_load_full_{}.png",
            std::process::id()
        ));
        rect_img
            .save(&temp_path)
            .expect("測試用暫存圖片寫入失敗");

        let full = load_full(&temp_path, 256).expect("應成功讀取完整圖片");
        assert_eq!(full.dimensions(), (100, 60));

        let scaled = load_full(&temp_path, 50).expect("應成功等比縮放完整圖片");
        assert_eq!(scaled.dimensions(), (50, 30));

        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_load_square_missing_file_returns_err() {
        let missing = Path::new("__non_existent_nijikage_test_image_12345__.png");
        let res = load_square(missing, 512);
        assert!(res.is_err(), "讀取不存在的檔案應回傳 Err");
    }

    #[test]
    fn test_load_square_center_crop_and_max_side() {
        // 產生 100x60 的長方形小圖，中心點設定特殊顏色以便驗證置中裁切
        let mut rect_img = make_synthetic_image(100, 60);
        rect_img.put_pixel(50, 30, Rgba([250, 10, 200, 255]));

        let temp_path = std::env::temp_dir().join(format!(
            "nijikage_test_load_square_{}.png",
            std::process::id()
        ));
        rect_img
            .save(&temp_path)
            .expect("測試用暫存圖片寫入失敗");

        // 1. max_side 大於短邊（60）時，應置中裁成 60x60 正方形
        let sq = load_square(&temp_path, 256).expect("應成功讀取並裁切暫存圖");
        assert_eq!(sq.width(), 60);
        assert_eq!(sq.height(), 60);
        // 原圖 (50, 30) 在置中裁切 (offset_x = 20, offset_y = 0) 後位於 (30, 30)
        assert_eq!(sq.get_pixel(30, 30), &Rgba([250, 10, 200, 255]));

        // 2. max_side 小於短邊（例如 32）時，應縮放至 32x32
        let sq_small = load_square(&temp_path, 32).expect("應成功讀取並縮放至 max_side");
        assert_eq!(sq_small.width(), 32);
        assert_eq!(sq_small.height(), 32);

        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_disc_tone_preview_styles_and_detail_preservation() {
        let src = make_synthetic_image(24, 24);

        let cd = disc_tone_preview(&src, DiscStyle::Cd);
        let dvd = disc_tone_preview(&src, DiscStyle::Dvd);
        let bd = disc_tone_preview(&src, DiscStyle::Bd);

        // 尺寸與透明度必須保留
        assert_eq!(cd.dimensions(), (24, 24));
        assert_eq!(dvd.dimensions(), (24, 24));
        assert_eq!(bd.dimensions(), (24, 24));
        assert_eq!(cd.get_pixel(0, 0)[3], src.get_pixel(0, 0)[3]);

        // 三種光碟風格應產生不同的色調結果
        assert_ne!(cd.as_raw(), dvd.as_raw(), "CD 與 DVD 色調應不同");
        assert_ne!(dvd.as_raw(), bd.as_raw(), "DVD 與 BD 色調應不同");
        assert_ne!(cd.as_raw(), bd.as_raw(), "CD 與 BD 色調應不同");

        // 驗證各風格色調特徵（取中間偏亮像素）
        let p_cd = cd.get_pixel(16, 16).0;
        let p_dvd = dvd.get_pixel(18, 18).0;
        let p_bd = bd.get_pixel(16, 16).0;

        // CD 為銀白色調，R/G/B 接近且偏亮
        let cd_diff = (p_cd[0] as i32 - p_cd[1] as i32).abs().max((p_cd[1] as i32 - p_cd[2] as i32).abs());
        assert!(cd_diff < 35, "CD 銀白色調各通道差距應小，實際：{p_cd:?}");

        // DVD 亮部偏金（紅、綠高於藍），暗部偏紫（藍、紅高於綠）
        let p_dvd_dark = dvd.get_pixel(3, 3).0;
        assert!(
            p_dvd[0] > p_dvd[2] && p_dvd[1] > p_dvd[2],
            "DVD 亮部應呈現暖金色調，實際：{p_dvd:?}"
        );
        assert!(
            p_dvd_dark[2] > p_dvd_dark[1] && p_dvd_dark[0] > p_dvd_dark[1],
            "DVD 暗部應呈現紫色調，實際：{p_dvd_dark:?}"
        );

        // 藍光為藍紫色調，藍色通道顯著高於紅與綠
        assert!(
            p_bd[2] > p_bd[0] && p_bd[2] > p_bd[1],
            "BD 應以藍色通道為主，實際：{p_bd:?}"
        );

        // 驗證保留明暗與細節（非單色平塗）：亮部像素應明顯亮於暗部像素
        for toned in [&cd, &dvd, &bd] {
            let dark = toned.get_pixel(1, 1).0;
            let bright = toned.get_pixel(22, 22).0;
            let dark_sum = dark[0] as u32 + dark[1] as u32 + dark[2] as u32;
            let bright_sum = bright[0] as u32 + bright[1] as u32 + bright[2] as u32;
            assert!(
                bright_sum > dark_sum + 80,
                "應保留原圖明暗對比，暗部={dark:?}，亮部={bright:?}"
            );
        }
    }
}
