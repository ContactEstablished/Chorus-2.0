//! Where the fleet strip goes (P7): its remembered position, kept on a screen. Physical pixels.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Area {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Squared distance from a point to the nearest point of the area.
    fn distance2(&self, x: i32, y: i32) -> i64 {
        let dx = i64::from((self.x - x).max(0).max(x - (self.x + self.width - 1)));
        let dy = i64::from((self.y - y).max(0).max(y - (self.y + self.height - 1)));
        dx * dx + dy * dy
    }
}

/// Gap from the top of the screen for a strip that has never been moved.
pub const TOP_GAP: i32 = 12;

/// The strip's top-left corner. A saved position stays where it was if the strip's centre is on a
/// screen's work area; otherwise it moves onto the nearest one (a monitor was unplugged or the
/// resolution changed). Without a saved position it goes top-centre of the first (primary) area.
/// The whole strip is kept inside the chosen area when it fits.
pub fn place(saved: Option<(i32, i32)>, size: (i32, i32), areas: &[Area]) -> (i32, i32) {
    let (w, h) = size;
    let Some(first) = areas.first() else { return saved.unwrap_or((80, 80)) };
    let Some((x, y)) = saved else { return (first.x + (first.width - w) / 2, first.y + TOP_GAP) };
    let (cx, cy) = (x + w / 2, y + h / 2);
    let area = areas
        .iter()
        .find(|a| a.contains(cx, cy))
        .or_else(|| areas.iter().min_by_key(|a| a.distance2(cx, cy)))
        .unwrap_or(first);
    let clamp = |v: i32, lo: i32, len: i32, size: i32| if len <= size { lo } else { v.clamp(lo, lo + len - size) };
    (clamp(x, area.x, area.width, w), clamp(y, area.y, area.height, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Area = Area { x: 0, y: 0, width: 1920, height: 1040 };
    const RIGHT: Area = Area { x: 1920, y: 0, width: 2560, height: 1400 };

    #[test]
    fn first_show_is_top_centre_of_the_primary_screen() {
        assert_eq!(place(None, (420, 64), &[MAIN, RIGHT]), ((1920 - 420) / 2, TOP_GAP));
    }

    #[test]
    fn a_saved_position_on_screen_is_kept() {
        assert_eq!(place(Some((100, 200)), (420, 64), &[MAIN]), (100, 200));
        assert_eq!(place(Some((2500, 900)), (420, 64), &[MAIN, RIGHT]), (2500, 900));
    }

    #[test]
    fn a_strip_hanging_off_an_edge_is_pulled_back_in() {
        assert_eq!(place(Some((1800, 1020)), (420, 64), &[MAIN]), (1920 - 420, 1040 - 64));
        assert_eq!(place(Some((-300, -30)), (420, 64), &[MAIN]), (0, 0));
    }

    #[test]
    fn an_unplugged_monitor_moves_the_strip_to_the_nearest_screen() {
        // It was on the right monitor, which is gone.
        assert_eq!(place(Some((3000, 500)), (420, 64), &[MAIN]), (1920 - 420, 500));
    }

    #[test]
    fn no_screens_known_keeps_the_saved_place() {
        assert_eq!(place(Some((5, 6)), (420, 64), &[]), (5, 6));
        assert_eq!(place(None, (420, 64), &[]), (80, 80));
    }
}
