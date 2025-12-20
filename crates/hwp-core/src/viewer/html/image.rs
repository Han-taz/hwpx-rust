/// 이미지 렌더링 모듈 / Image rendering module
use crate::types::INT32;
use crate::viewer::html::styles::{int32_to_mm, round_to_2dp};

/// 이미지를 HTML로 렌더링 / Render image to HTML
pub fn render_image(
    image_url: &str,
    left: INT32,
    top: INT32,
    width: INT32,
    height: INT32,
) -> String {
    let left_mm = round_to_2dp(int32_to_mm(left));
    let top_mm = round_to_2dp(int32_to_mm(top));
    let width_mm = round_to_2dp(int32_to_mm(width));
    let height_mm = round_to_2dp(int32_to_mm(height));

    format!(
        r#"<div class="hsR" style="top:{top_mm}mm;left:{left_mm}mm;width:{width_mm}mm;height:{height_mm}mm;background-repeat:no-repeat;background-size:contain;background-image:url('{image_url}');"></div>"#
    )
}

/// 이미지를 배경 이미지로 렌더링 (인라인 스타일 포함) / Render image as background image (with inline styles)
pub fn render_image_with_style(
    image_url: &str,
    left: INT32,
    top: INT32,
    width: INT32,
    height: INT32,
    margin_bottom: INT32,
    margin_right: INT32,
) -> String {
    let left_mm = round_to_2dp(int32_to_mm(left));
    let top_mm = round_to_2dp(int32_to_mm(top));
    let width_mm = round_to_2dp(int32_to_mm(width));
    let height_mm = round_to_2dp(int32_to_mm(height));
    let margin_bottom_mm = round_to_2dp(int32_to_mm(margin_bottom));
    let margin_right_mm = round_to_2dp(int32_to_mm(margin_right));

    format!(
        r#"<div class="hsR" style="top:{top_mm}mm;left:{left_mm}mm;margin-bottom:{margin_bottom_mm}mm;margin-right:{margin_right_mm}mm;width:{width_mm}mm;height:{height_mm}mm;display:inline-block;position:relative;vertical-align:middle;background-repeat:no-repeat;background-size:contain;background-image:url('{image_url}');"></div>"#
    )
}
