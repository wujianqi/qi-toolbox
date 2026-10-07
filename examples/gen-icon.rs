//! 开发工具：把 `src/logo.svg` 光栅化成多尺寸 Windows 图标 `src/logo.ico`。
//!
//! exe 内嵌的图标资源来自 `src/logo.ico`（build.rs 经 winresource 嵌入），而
//! ICO 只能装位图，装不下 SVG——所以 logo 更新后要跑一次：
//!
//! ```text
//! cargo run --example gen-icon
//! ```
//!
//! 光栅化复用 windui 的 SVG 解码（与窗口图标 `shell::app_icon` 同一条渲染
//! 路径），不引入额外依赖。尺寸档位对齐 Windows 消费场景：任务栏 16/24/32、
//! 桌面/Alt+Tab 32/48、高 DPI 64/128、大图标 256。
//!
//! 编码约定与 Pillow/ImageMagick 一致：<256 的档位用原生 BMP 条目（兼容性最
//! 广），256 用 PNG 压缩条目（Vista 起支持，省 ~250KB）。

use std::path::Path;

const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let svg = std::fs::read(root.join("src/logo.svg")).expect("读取 src/logo.svg");
    let out_path = root.join("src/logo.ico");

    // 先光栅并编码每档条目（PNG 条目 = 原样 PNG 字节；BMP 条目 = 手工拼装）
    let mut entries: Vec<(u32, Vec<u8>)> = Vec::new();
    for &size in &SIZES {
        let img = windui::render::image::Image::from_svg_bytes(&svg, Some(size))
            .unwrap_or_else(|e| panic!("SVG 光栅化 {size}px 失败: {e:?}"));
        let (w, h) = (img.width(), img.height());
        assert_eq!((w, h), (size, size), "SVG 不是正方形视口，ICO 档位会错位");
        let rgba = img.to_rgba();
        let encoded = if size < 256 {
            encode_bmp_entry(size, &rgba)
        } else {
            encode_png_entry(size, &rgba)
        };
        entries.push((size, encoded));
    }

    // ICONDIR + ICONDIRENTRY 表 + 条目数据。256 在宽/高字节里记 0（规范如此）。
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + entries.len() * 16;
    for (size, data) in &entries {
        let dim = if *size == 256 { 0u8 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]); // 宽、高、色板数、保留
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bpp
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }

    std::fs::write(&out_path, &out).expect("写 src/logo.ico");
    println!(
        "已生成 {}（{} 档，{} KB）",
        out_path.display(),
        entries.len(),
        out.len() / 1024
    );
}

/// 32bpp BMP 条目：BITMAPINFOHEADER + 自底向上 BGRA 像素 + 全零 AND 掩码
/// （alpha 通道已表达透明度，掩码按规范仍需占位）。
fn encode_bmp_entry(size: u32, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(rgba.len(), (size * size * 4) as usize);
    let mask_row = size.div_ceil(32) * 4;

    let mut buf = Vec::with_capacity(40 + rgba.len() + (mask_row * size) as usize);
    // BITMAPINFOHEADER：高度写 2×（XOR 面与 AND 掩码共用一个高度字段）
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(size as i32).to_le_bytes());
    buf.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&32u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    buf.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage（BI_RGB 可为 0）
    buf.extend_from_slice(&0u32.to_le_bytes()); // X pels/meter
    buf.extend_from_slice(&0u32.to_le_bytes()); // Y pels/meter
    buf.extend_from_slice(&0u32.to_le_bytes()); // clrUsed
    buf.extend_from_slice(&0u32.to_le_bytes()); // clrImportant

    // XOR 面：BGRA，自底向上
    for y in (0..size).rev() {
        let row = &rgba[(y * size * 4) as usize..((y + 1) * size * 4) as usize];
        for px in row.as_chunks::<4>().0 {
            buf.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    // AND 掩码：1bpp 全零，自底向上
    buf.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    buf
}

/// 256px PNG 压缩条目（png 已是项目依赖，直接复用）。
fn encode_png_entry(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut enc = png::Encoder::new(&mut buf, size, size);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("写 PNG 头");
    writer.write_image_data(rgba).expect("写 PNG 数据");
    writer.finish().expect("收尾 PNG");
    buf
}
