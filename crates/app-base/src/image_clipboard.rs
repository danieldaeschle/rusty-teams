use std::io::Cursor;

use gpui_kit::{App, ClipboardItem, Image, ImageFormat};

const DIB_HEADER_SIZE: u32 = 40;
const DIB_BITS_PER_PIXEL: u16 = 32;

/// Windows apps such as Paint, Office and Teams only paste `CF_DIB`, which gpui does not write.
pub struct ClipboardImage {
    png: Vec<u8>,
    #[cfg(windows)]
    dib: Vec<u8>,
}

impl ClipboardImage {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let decoded = image::load_from_memory(bytes).ok()?;
        let png = if image::guess_format(bytes).ok()? == image::ImageFormat::Png {
            bytes.to_vec()
        } else {
            let mut png = Vec::new();
            decoded
                .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
                .ok()?;
            png
        };
        Some(ClipboardImage {
            png,
            #[cfg(windows)]
            dib: dib_bytes(&decoded.into_rgba8()),
        })
    }

    pub fn write(self, cx: &mut App) {
        #[cfg(windows)]
        if windows_clipboard::write(&self.png, &self.dib).is_ok() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            self.png,
        )));
    }
}

pub fn dib_bytes(rgba: &image::RgbaImage) -> Vec<u8> {
    let (width, height) = rgba.dimensions();
    let pixel_bytes = width as usize * height as usize * 4;
    let mut dib = Vec::with_capacity(DIB_HEADER_SIZE as usize + pixel_bytes);
    dib.extend_from_slice(&DIB_HEADER_SIZE.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    dib.extend_from_slice(&(height as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&DIB_BITS_PER_PIXEL.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    dib.extend_from_slice(&[0; 16]);
    for row in rgba.rows().rev() {
        for pixel in row {
            let [red, green, blue, alpha] = pixel.0;
            let on_white = |channel: u8| {
                let alpha = u16::from(alpha);
                ((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
            };
            dib.extend_from_slice(&[on_white(blue), on_white(green), on_white(red), 255]);
        }
    }
    dib
}

#[cfg(windows)]
mod windows_clipboard {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_DIB;
    use windows::core::{Owned, Result, w};

    pub fn write(png: &[u8], dib: &[u8]) -> Result<()> {
        unsafe {
            OpenClipboard(None)?;
            let result = (|| {
                EmptyClipboard()?;
                set_bytes(png, RegisterClipboardFormatW(w!("PNG")))?;
                set_bytes(dib, u32::from(CF_DIB.0))
            })();
            CloseClipboard().ok();
            result
        }
    }

    unsafe fn set_bytes(data: &[u8], format: u32) -> Result<()> {
        unsafe {
            let global = Owned::new(GlobalAlloc(GMEM_MOVEABLE, data.len())?);
            let pointer = GlobalLock(*global);
            if pointer.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), pointer.cast(), data.len());
            GlobalUnlock(*global).ok();
            SetClipboardData(format, Some(HANDLE(global.0)))?;
            std::mem::forget(global);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn dib_is_a_bottom_up_bgra_bitmap_on_white_behind_a_40_byte_header() {
        let mut rgba = RgbaImage::new(2, 2);
        rgba.put_pixel(0, 0, Rgba([10, 20, 30, 255]));
        rgba.put_pixel(1, 1, Rgba([40, 50, 60, 128]));
        let dib = dib_bytes(&rgba);
        assert_eq!(dib.len(), 40 + 16);
        assert_eq!(&dib[0..4], &40u32.to_le_bytes());
        assert_eq!(&dib[4..8], &2i32.to_le_bytes());
        assert_eq!(&dib[8..12], &2i32.to_le_bytes());
        assert_eq!(&dib[14..16], &32u16.to_le_bytes());
        assert_eq!(&dib[44..48], &[157, 152, 147, 255]);
        assert_eq!(&dib[48..52], &[30, 20, 10, 255]);
    }

    #[test]
    fn a_jpeg_is_kept_as_png_for_the_clipboard() {
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 1, Rgba([1, 2, 3, 255])))
            .to_rgb8()
            .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        let image = ClipboardImage::from_bytes(&jpeg).unwrap();
        assert_eq!(
            image::guess_format(&image.png).unwrap(),
            image::ImageFormat::Png
        );
    }
}
