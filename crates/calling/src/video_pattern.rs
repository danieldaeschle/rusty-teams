use std::time::Duration;

use libwebrtc::video_frame::I420Buffer;
use tokio::task::JoinHandle;
use tokio::time::interval;

use crate::video_send::LocalSink;

pub const CAMERA_WIDTH: u32 = 640;
pub const CAMERA_HEIGHT: u32 = 360;
pub const SCREEN_WIDTH: u32 = 1280;
pub const SCREEN_HEIGHT: u32 = 720;
const SQUARE: u32 = 64;
const BLOCK_ROWS: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    Camera,
    Screen,
}

impl PatternKind {
    pub fn size(self) -> (u32, u32) {
        match self {
            PatternKind::Camera => (CAMERA_WIDTH, CAMERA_HEIGHT),
            PatternKind::Screen => (SCREEN_WIDTH, SCREEN_HEIGHT),
        }
    }

    pub fn fps(self) -> u32 {
        match self {
            PatternKind::Camera => 30,
            PatternKind::Screen => 15,
        }
    }
}

/// A moving test picture: no real camera or screen is ever read.
pub fn pattern_frame(kind: PatternKind, tick: u32) -> I420Buffer {
    let (width, height) = kind.size();
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (y_plane, u_plane, v_plane) = buffer.data_mut();
    match kind {
        PatternKind::Camera => paint_camera(y_plane, u_plane, v_plane, (width, height), (stride_y, stride_u, stride_v), tick),
        PatternKind::Screen => paint_screen(y_plane, u_plane, v_plane, (width, height), (stride_y, stride_u, stride_v), tick),
    }
    buffer
}

fn paint_camera(
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
    (width, height): (u32, u32),
    (stride_y, stride_u, stride_v): (u32, u32, u32),
    tick: u32,
) {
    let square_x = (tick * 6) % (width - SQUARE);
    let square_y = (tick * 4) % (height - SQUARE);
    for row in 0..height {
        for column in 0..width {
            let inside = column >= square_x && column < square_x + SQUARE && row >= square_y && row < square_y + SQUARE;
            let shade = ((column + tick * 3) * 255 / width) as u8;
            y_plane[(row * stride_y + column) as usize] = if inside { 235 } else { shade / 2 + 24 };
        }
    }
    for row in 0..height / 2 {
        for column in 0..width / 2 {
            u_plane[(row * stride_u + column) as usize] = (64 + (column * 128 / (width / 2)) as u8).wrapping_add((tick * 2) as u8 / 4);
            v_plane[(row * stride_v + column) as usize] = (192 - (row * 128 / (height / 2)) as u8).wrapping_add((tick % 64) as u8);
        }
    }
}

fn paint_screen(
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
    (width, height): (u32, u32),
    (stride_y, stride_u, stride_v): (u32, u32, u32),
    tick: u32,
) {
    let band = height / BLOCK_ROWS;
    let cursor_x = (tick * 9) % (width - SQUARE / 2);
    let cursor_y = (tick * 5) % (height - SQUARE / 2);
    for row in 0..height {
        for column in 0..width {
            let on_cursor = column >= cursor_x && column < cursor_x + SQUARE / 2 && row >= cursor_y && row < cursor_y + SQUARE / 2;
            let line = (row / 18) % 2 == 0 && column % 11 < 7 && column > 40 && column < width - 40;
            let title_bar = row < 40;
            y_plane[(row * stride_y + column) as usize] = match (on_cursor, title_bar, line) {
                (true, _, _) => 235,
                (_, true, _) => 60,
                (_, _, true) => 40,
                _ => 225 - (row / band * 6) as u8,
            };
        }
    }
    u_plane[..(stride_u * height / 2) as usize].fill(128);
    v_plane[..(stride_v * height / 2) as usize].fill(128);
}

pub fn spawn_pattern(mut sink: LocalSink, kind: PatternKind) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1) / kind.fps());
        let mut tick = 0u32;
        loop {
            ticker.tick().await;
            sink.push(pattern_frame(kind, tick));
            tick = tick.wrapping_add(1);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma_checksum(kind: PatternKind, tick: u32) -> u64 {
        let buffer = pattern_frame(kind, tick);
        buffer.data().0.iter().map(|&value| u64::from(value)).sum()
    }

    #[test]
    fn the_camera_pattern_moves_between_frames() {
        assert_ne!(luma_checksum(PatternKind::Camera, 0), luma_checksum(PatternKind::Camera, 9));
    }

    #[test]
    fn the_screen_pattern_moves_between_frames() {
        assert_ne!(luma_checksum(PatternKind::Screen, 0), luma_checksum(PatternKind::Screen, 4));
    }

    #[test]
    fn patterns_have_the_documented_sizes() {
        assert_eq!(pattern_frame(PatternKind::Camera, 1).strides().0, CAMERA_WIDTH);
        assert_eq!(PatternKind::Screen.size(), (SCREEN_WIDTH, SCREEN_HEIGHT));
    }
}
