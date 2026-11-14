use tubeamp_skin::SpriteView;

/// Integer rectangle in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    /// Whether the point lies inside (right and bottom edges are exclusive).
    pub const fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// An RGB-ish pixel buffer, `0xAARRGGBB`, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

/// Source-over blend of `src` onto an opaque `dst`. The result stays opaque.
fn composite(src: u32, dst: u32) -> u32 {
    let a = src >> 24;
    match a {
        0 => dst,
        0xFF => src | 0xFF00_0000,
        _ => {
            let mix = |s: u32, d: u32| (s * a + d * (255 - a) + 127) / 255;
            let r = mix((src >> 16) & 0xFF, (dst >> 16) & 0xFF);
            let g = mix((src >> 8) & 0xFF, (dst >> 8) & 0xFF);
            let b = mix(src & 0xFF, dst & 0xFF);
            0xFF00_0000 | (r << 16) | (g << 8) | b
        }
    }
}

impl Canvas {
    /// A `w` × `h` canvas filled with opaque black.
    pub fn new(w: u32, h: u32) -> Self {
        Canvas {
            w,
            h,
            px: vec![0xFF00_0000; (w * h) as usize],
        }
    }

    fn full_clip(&self) -> Rect {
        Rect::new(0, 0, self.w as i32, self.h as i32)
    }

    /// Draws a whole sprite with its top-left corner at `(dx, dy)`.
    pub fn blit(&mut self, sv: &SpriteView<'_>, dx: i32, dy: i32) {
        let clip = self.full_clip();
        self.blit_clipped(sv, dx, dy, clip);
    }

    /// Draws the sub-rectangle `src = (x, y, w, h)` of a sprite at `(dx, dy)`.
    /// The sub-rectangle is relative to the sprite and clamped to it.
    pub fn blit_region(
        &mut self,
        sv: &SpriteView<'_>,
        src: (u32, u32, u32, u32),
        dx: i32,
        dy: i32,
    ) {
        let (sx, sy, sw, sh) = src;
        let sx = sx.min(sv.w);
        let sy = sy.min(sv.h);
        let sub = SpriteView {
            sheet: sv.sheet,
            x: sv.x + sx,
            y: sv.y + sy,
            w: sw.min(sv.w - sx),
            h: sh.min(sv.h - sy),
        };
        self.blit(&sub, dx, dy);
    }

    /// Draws a sprite at `(dx, dy)`, restricted to the `clip` rectangle.
    pub fn blit_clipped(&mut self, sv: &SpriteView<'_>, dx: i32, dy: i32, clip: Rect) {
        let x0 = dx.max(clip.x).max(0);
        let y0 = dy.max(clip.y).max(0);
        let x1 = (dx + sv.w as i32).min(clip.x + clip.w).min(self.w as i32);
        let y1 = (dy + sv.h as i32).min(clip.y + clip.h).min(self.h as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let src = sv.pixel((x - dx) as u32, (y - dy) as u32);
                let i = (y as u32 * self.w + x as u32) as usize;
                self.px[i] = composite(src, self.px[i]);
            }
        }
    }

    /// Draws a whole sprite stretched to `dw` × `dh` with its top-left corner at `(dx, dy)`,
    /// nearest neighbour, clipped to the canvas. Used for the stretched remainder tiles.
    pub fn blit_scaled(&mut self, sv: &SpriteView<'_>, dx: i32, dy: i32, dw: u32, dh: u32) {
        if sv.w == 0 || sv.h == 0 || dw == 0 || dh == 0 {
            return;
        }
        let x1 = (dx + dw as i32).min(self.w as i32);
        let y1 = (dy + dh as i32).min(self.h as i32);
        for y in dy.max(0)..y1 {
            let sy = (((y - dy) as u64 * u64::from(sv.h)) / u64::from(dh)) as u32;
            for x in dx.max(0)..x1 {
                let sx = (((x - dx) as u64 * u64::from(sv.w)) / u64::from(dw)) as u32;
                let src = sv.pixel(sx.min(sv.w - 1), sy.min(sv.h - 1));
                let i = (y as u32 * self.w + x as u32) as usize;
                self.px[i] = composite(src, self.px[i]);
            }
        }
    }

    /// Fills a rectangle (clipped to the canvas) with an opaque colour.
    pub fn fill(&mut self, rect: Rect, color: u32) {
        let clip = self.full_clip();
        let x0 = rect.x.max(clip.x);
        let y0 = rect.y.max(clip.y);
        let x1 = (rect.x + rect.w).min(clip.w);
        let y1 = (rect.y + rect.h).min(clip.h);
        for y in y0..y1 {
            for x in x0..x1 {
                self.px[(y as u32 * self.w + x as u32) as usize] = color;
            }
        }
    }

    /// Nearest-neighbour integer upscale into `dst`, which must hold exactly
    /// `(w * factor) * (h * factor)` pixels. The output is `0x00RRGGBB`.
    pub fn scale_into(&self, dst: &mut [u32], factor: u32) {
        assert!(factor >= 1, "scale factor must be at least 1");
        let out_w = (self.w * factor) as usize;
        let out_h = (self.h * factor) as usize;
        assert_eq!(dst.len(), out_w * out_h, "destination size mismatch");
        for (row, out_row) in dst.chunks_exact_mut(out_w).enumerate() {
            let src_row = &self.px[(row / factor as usize) * self.w as usize..][..self.w as usize];
            for (out, &src) in out_row.iter_mut().zip(
                src_row
                    .iter()
                    .flat_map(|p| std::iter::repeat_n(p, factor as usize)),
            ) {
                *out = src & 0x00FF_FFFF;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_skin::testkit;

    #[test]
    fn composite_respects_alpha_extremes() {
        assert_eq!(composite(0x00FF_0000, 0xFF00_00FF), 0xFF00_00FF);
        assert_eq!(composite(0xFFFF_0000, 0xFF00_00FF), 0xFFFF_0000);
        assert_eq!(composite(0x80FF_FFFF, 0xFF00_0000), 0xFF80_8080);
    }

    #[test]
    fn scale_into_repeats_each_pixel_factor_times() {
        let mut c = Canvas::new(2, 1);
        c.px = vec![0xFF11_2233, 0xFF44_5566];
        let mut dst = vec![0u32; 4 * 2];
        c.scale_into(&mut dst, 2);
        assert_eq!(
            dst,
            vec![
                0x0011_2233,
                0x0011_2233,
                0x0044_5566,
                0x0044_5566,
                0x0011_2233,
                0x0011_2233,
                0x0044_5566,
                0x0044_5566
            ]
        );
    }

    #[test]
    fn blit_clipped_never_writes_outside_the_clip_rect() {
        let skin = testkit::synthetic_skin();
        let sv = skin
            .sprite(tubeamp_skin::Sheet::Cbuttons, "MAIN_PLAY_BUTTON")
            .expect("sprite");
        let mut c = Canvas::new(40, 40);
        c.blit_clipped(&sv, 0, 0, Rect::new(5, 5, 10, 10));
        for y in 0..40 {
            for x in 0..40 {
                let inside = (5..15).contains(&x) && (5..15).contains(&y);
                let drawn = c.px[(y * 40 + x) as usize] != 0xFF00_0000;
                assert_eq!(drawn, inside, "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn blit_scaled_stretches_nearest_neighbour_and_clips() {
        let skin = testkit::synthetic_skin();
        let sv = skin
            .sprite(tubeamp_skin::Sheet::Cbuttons, "MAIN_PLAY_BUTTON")
            .expect("sprite");
        // Twice as wide: each source column covers two canvas columns.
        let mut c = Canvas::new(46, 18);
        c.blit_scaled(&sv, 0, 0, 46, 18);
        for x in 0..46u32 {
            assert_eq!(c.px[x as usize], sv.pixel(x / 2, 0), "column {x}");
        }
        // Partly off the canvas: nothing outside is written and nothing panics.
        let mut small = Canvas::new(10, 10);
        small.blit_scaled(&sv, 5, -4, 40, 30);
        assert_eq!(small.px.len(), 100);
        let mut untouched = Canvas::new(4, 4);
        untouched.blit_scaled(&sv, 0, 0, 0, 5);
        assert_eq!(untouched, Canvas::new(4, 4));
    }

    #[test]
    fn blit_region_copies_the_requested_sub_rectangle() {
        let skin = testkit::synthetic_skin();
        let sv = skin
            .sprite(tubeamp_skin::Sheet::Volume, "MAIN_VOLUME_BACKGROUND")
            .expect("sprite");
        let mut c = Canvas::new(68, 13);
        c.blit_region(&sv, (0, 300, 68, 13), 0, 0);
        assert_eq!(testkit::decode_coord(c.px[0]), (0, 300));
        assert_eq!(testkit::decode_coord(c.px[12 * 68 + 67]), (67, 312));
    }
}
