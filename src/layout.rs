use smithay::utils::{Logical, Point, Rectangle, Size};

pub type Rect = Rectangle<i32, Logical>;

/// Master + stack, with integer partitioning so the final row ends exactly at the bottom.
pub fn tiled(
    area: Size<i32, Logical>,
    count: usize,
    gap: i32,
    border: i32,
    ratio: f64,
) -> Vec<Rect> {
    if count == 0 {
        return vec![];
    }
    let w = (area.w - 2 * gap).max(1);
    let h = (area.h - 2 * gap).max(1);
    let inset = |x, y, width: i32, height: i32| {
        Rectangle::new(
            (x + border, y + border).into(),
            ((width - 2 * border).max(1), (height - 2 * border).max(1)).into(),
        )
    };
    if count == 1 {
        return vec![inset(gap, gap, w, h)];
    }
    let master = ((w - gap) as f64 * ratio).round() as i32;
    let mut result = vec![inset(gap, gap, master, h)];
    let rows = (count - 1) as i32;
    let available = (h - gap * (rows - 1)).max(rows);
    for i in 0..rows {
        let top = available * i / rows;
        let bottom = available * (i + 1) / rows;
        result.push(inset(
            gap + master + gap,
            gap + top + i * gap,
            w - master - gap,
            bottom - top,
        ));
    }
    result
}

pub fn selection(a: Point<f64, Logical>, b: Point<f64, Logical>, area: Size<i32, Logical>) -> Rect {
    let x1 = a.x.min(b.x).round().clamp(0., area.w as f64) as i32;
    let x2 = a.x.max(b.x).round().clamp(0., area.w as f64) as i32;
    let y1 = a.y.min(b.y).round().clamp(0., area.h as f64) as i32;
    let y2 = a.y.max(b.y).round().clamp(0., area.h as f64) as i32;
    Rectangle::new((x1, y1).into(), (x2 - x1, y2 - y1).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_fit_and_do_not_overlap() {
        for count in 1..=12 {
            let rects = tiled((1280, 800).into(), count, 12, 2, 0.58);
            for (i, a) in rects.iter().enumerate() {
                assert!(a.loc.x >= 0 && a.loc.y >= 0);
                assert!(a.loc.x + a.size.w <= 1280 && a.loc.y + a.size.h <= 800);
                for b in &rects[i + 1..] {
                    assert!(!a.overlaps(*b));
                }
            }
        }
    }
    #[test]
    fn drag_works_backwards_and_clips_to_output() {
        let r = selection((500., 400.).into(), (-20., 50.).into(), (800, 600).into());
        assert_eq!(r, Rectangle::new((0, 50).into(), (500, 350).into()));
    }
}
