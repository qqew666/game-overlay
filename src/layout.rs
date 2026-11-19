//! Single source of truth for painted controls, pointer hit testing and tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleAutostart,
    ToggleFollow,
    ToggleOverlay,
    ToggleCompact,
    Scale(u32),
    PreviousTarget,
    NextTarget,
    Elevate,
    HideToTray,
    CheckUpdate,
    Quit,
    Mimi,
    Theme(u32),
    Glass,
    Sound,
}
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}
pub const WIDTH: i32 = 960;
pub const HEIGHT: i32 = 760;
pub const SCALES: [u32; 6] = [40, 60, 80, 100, 125, 150];
pub fn rect(action: Action) -> Rect {
    match action {
        Action::Mimi => Rect::new(36., 100., 216., 216.),
        Action::Theme(i) => Rect::new(36. + i.min(4) as f32 * 43.5, 590., 40., 34.),
        Action::Glass => Rect::new(36., 634., 104., 36.),
        Action::Sound => Rect::new(148., 634., 104., 36.),
        Action::ToggleCompact => Rect::new(724., 594., 204., 48.),
        Action::ToggleAutostart => Rect::new(292., 226., 636., 50.),
        Action::ToggleFollow => Rect::new(292., 284., 636., 50.),
        Action::ToggleOverlay => Rect::new(292., 342., 636., 50.),
        Action::Scale(pct) => {
            let i = SCALES.iter().position(|v| *v == pct).unwrap_or(1);
            Rect::new(292. + i as f32 * 108., 440., 96., 42.)
        }
        Action::PreviousTarget => Rect::new(292., 526., 48., 44.),
        Action::NextTarget => Rect::new(880., 526., 48., 44.),
        Action::Elevate => Rect::new(292., 594., 204., 48.),
        Action::HideToTray => Rect::new(508., 594., 204., 48.),
        Action::CheckUpdate => Rect::new(292., 654., 420., 48.),
        Action::Quit => Rect::new(724., 654., 204., 48.),
    }
}
pub fn actions() -> Vec<Action> {
    let mut a = vec![
        Action::ToggleAutostart,
        Action::ToggleFollow,
        Action::ToggleOverlay,
    ];
    a.extend([
        Action::Mimi,
        Action::Theme(0),
        Action::Theme(1),
        Action::Theme(2),
        Action::Theme(3),
        Action::Theme(4),
        Action::Glass,
        Action::Sound,
    ]);
    a.extend(SCALES.map(Action::Scale));
    a.extend([
        Action::PreviousTarget,
        Action::NextTarget,
        Action::Elevate,
        Action::HideToTray,
        Action::ToggleCompact,
        Action::CheckUpdate,
        Action::Quit,
    ]);
    a
}
pub fn hit_test(x: f32, y: f32) -> Option<Action> {
    actions().into_iter().find(|a| rect(*a).contains(x, y))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_control_is_reachable_and_inside_window() {
        for a in actions() {
            let r = rect(a);
            assert!(
                r.x >= 0. && r.y >= 0. && r.x + r.w <= WIDTH as f32 && r.y + r.h <= HEIGHT as f32
            );
            for (x, y) in [
                (r.x + 1., r.y + 1.),
                (r.x + r.w / 2., r.y + r.h / 2.),
                (r.x + r.w - 1., r.y + r.h - 1.),
            ] {
                assert_eq!(hit_test(x, y), Some(a), "{a:?} at {x},{y}");
            }
        }
    }
    #[test]
    fn controls_never_overlap() {
        let actions = actions();
        for (i, a) in actions.iter().enumerate() {
            for b in &actions[i + 1..] {
                let a = rect(*a);
                let b = rect(*b);
                assert!(
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
                );
            }
        }
    }
    #[test]
    fn titles_and_gaps_do_not_trigger_actions() {
        for (x, y) in [
            (310., 420.),
            (310., 510.),
            (500., 620.),
            (600., 648.),
            (40., 90.),
        ] {
            assert_eq!(hit_test(x, y), None);
        }
    }
}
