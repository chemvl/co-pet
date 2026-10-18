//! D4: Windows has no drag pasteboard, so an OS drag is inferred: the left button went down
//! outside the pet's hit regions and the cursor then moved at least 4 px with it held.
#![cfg_attr(not(windows), allow(dead_code))]

/// The pet's own press-and-drag threshold.
const MOVE_SQ: f64 = 4.0 * 4.0;

#[derive(Default, Debug)]
pub struct DragProbe {
    press: Option<(f64, f64)>,
    outside: bool,
    active: bool,
}

impl DragProbe {
    pub fn update(&mut self, left_down: bool, cursor: (f64, f64), over_hit_region: bool) -> bool {
        if !left_down {
            *self = DragProbe::default();
            return false;
        }
        let press = match self.press {
            Some(p) => p,
            None => {
                self.press = Some(cursor);
                self.outside = !over_hit_region;
                cursor
            }
        };
        if self.outside && !self.active {
            let (dx, dy) = (cursor.0 - press.0, cursor.1 - press.1);
            self.active = dx * dx + dy * dy >= MOVE_SQ;
        }
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::DragProbe;

    #[test]
    fn a_press_outside_that_moves_is_a_drag_until_release() {
        let mut p = DragProbe::default();
        assert!(!p.update(true, (0.0, 0.0), false));
        assert!(!p.update(true, (3.0, 0.0), false));
        assert!(p.update(true, (3.0, 3.0), false));
        assert!(p.update(true, (3.0, 3.0), true), "stays a drag over the pet");
        assert!(!p.update(false, (3.0, 3.0), true));
    }

    #[test]
    fn a_press_on_the_pet_is_never_an_os_drag() {
        let mut p = DragProbe::default();
        assert!(!p.update(true, (0.0, 0.0), true));
        assert!(!p.update(true, (50.0, 0.0), false));
    }
}
