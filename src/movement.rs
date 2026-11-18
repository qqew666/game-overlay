//! Frame-rate independent following and monitor work-area bounds.
#[derive(Default)]
pub struct Motion {
    position: Option<(f64, f64)>,
}
impl Motion {
    pub fn update(&mut self, target: (i32, i32), seconds: f64, snap: bool) -> (i32, i32) {
        let target = (target.0 as f64, target.1 as f64);
        let position = match self.position {
            Some(current) if !snap => {
                let blend = 1.0 - (-seconds.clamp(0.0, 0.1) / 0.045).exp();
                let next = (
                    current.0 + (target.0 - current.0) * blend,
                    current.1 + (target.1 - current.1) * blend,
                );
                if (target.0 - next.0).abs().max((target.1 - next.1).abs()) < 0.5 {
                    target
                } else {
                    next
                }
            }
            _ => target,
        };
        self.position = Some(position);
        (position.0.round() as i32, position.1.round() as i32)
    }
}
pub fn constrain(position: (i32, i32), size: (i32, i32), area: (i32, i32, i32, i32)) -> (i32, i32) {
    (
        position.0.clamp(area.0, (area.2 - size.0).max(area.0)),
        position.1.clamp(area.1, (area.3 - size.1).max(area.1)),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movement_converges_without_overshoot() {
        let mut motion = Motion::default();
        assert_eq!(motion.update((0, 0), 0.0, false), (0, 0));
        let mut last = (0, 0);
        for _ in 0..120 {
            let next = motion.update((800, -300), 1.0 / 60.0, false);
            assert!(next.0 >= last.0 && next.0 <= 800 && next.1 <= last.1 && next.1 >= -300);
            last = next;
        }
        assert_eq!(last, (800, -300));
        assert_eq!(motion.update((-1000, 50), 0.0, true), (-1000, 50));
    }
    #[test]
    fn same_duration_at_different_refresh_rates() {
        let run = |rate| {
            let mut m = Motion::default();
            m.update((0, 0), 0.0, false);
            let mut p = (0, 0);
            for _ in 0..rate / 5 {
                p = m.update((1000, 500), 1.0 / rate as f64, false);
            }
            p
        };
        assert_eq!(run(30), run(60));
        assert_eq!(run(60), run(120));
    }
    #[test]
    fn bounds_support_negative_monitors_and_oversized_overlay() {
        assert_eq!(
            constrain((-1800, -90), (420, 210), (-1920, -200, 0, 880)),
            (-1800, -90)
        );
        assert_eq!(
            constrain((100, 900), (420, 210), (-1920, 0, 0, 1080)),
            (-420, 870)
        );
        assert_eq!(constrain((100, 100), (900, 800), (0, 0, 640, 480)), (0, 0));
    }
}
