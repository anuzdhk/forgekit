//! Caliper: measuring on a frozen screenshot. From a point, walk outwards
//! until the color changes, which finds the edges of whatever is under the
//! pointer (a button, a panel, a gap between two boxes).

/// An RGBA image.
pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// Distances in pixels from the point to the nearest edge in each direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spacing {
    pub left: u32,
    pub right: u32,
    pub up: u32,
    pub down: u32,
}

impl Spacing {
    /// Width and height of the region found, including the start pixel.
    pub fn size(&self) -> (u32, u32) {
        (self.left + self.right + 1, self.up + self.down + 1)
    }
}

impl Image<'_> {
    fn px(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }
}

fn differs(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).any(|(x, y)| x.abs_diff(y) > tolerance)
}

/// How far the start pixel's color continues in each direction. `tolerance`
/// is the largest per-channel difference that still counts as "the same"
/// (antialiasing and gradients need a little slack).
pub fn spacing(img: &Image, x: u32, y: u32, tolerance: u8) -> Spacing {
    let x = x.min(img.width.saturating_sub(1));
    let y = y.min(img.height.saturating_sub(1));
    let start = img.px(x, y);
    let walk = |dx: i64, dy: i64| -> u32 {
        let (mut cx, mut cy, mut n) = (x as i64, y as i64, 0u32);
        loop {
            cx += dx;
            cy += dy;
            if cx < 0 || cy < 0 || cx >= img.width as i64 || cy >= img.height as i64 {
                return n;
            }
            if differs(img.px(cx as u32, cy as u32), start, tolerance) {
                return n;
            }
            n += 1;
        }
    };
    Spacing { left: walk(-1, 0), right: walk(1, 0), up: walk(0, -1), down: walk(0, 1) }
}

/// "120 × 48" for copying.
pub fn describe(w: u32, h: u32) -> String {
    format!("{w} × {h}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white 20×10 image with a black 6×4 box at (5, 3).
    fn sample() -> Vec<u8> {
        let (w, h) = (20u32, 10u32);
        let mut v = vec![255u8; (w * h * 4) as usize];
        for y in 3..7 {
            for x in 5..11 {
                let i = ((y * w + x) * 4) as usize;
                v[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        v
    }

    #[test]
    fn finds_box_edges() {
        let data = sample();
        let img = Image { width: 20, height: 10, rgba: &data };
        let inside = spacing(&img, 7, 4, 16);
        assert_eq!(inside, Spacing { left: 2, right: 3, up: 1, down: 2 });
        assert_eq!(inside.size(), (6, 4));
        // From the white area left of the box: up to the image edges and to the box.
        let outside = spacing(&img, 2, 4, 16);
        assert_eq!(outside, Spacing { left: 2, right: 2, up: 4, down: 5 });
        // Out-of-range points are clamped, not panics.
        assert_eq!(spacing(&img, 99, 99, 16).size(), (20, 10));
        assert_eq!(describe(6, 4), "6 × 4");
    }
}
