use vesa_event::{InputEvent, Position};

pub const EDGE_PUSH_THRESHOLD: u32 = 3;

const EDGE_MARGIN: f64 = 2.0;

pub fn detect_edge_push(
    event: &InputEvent,
    cursor_x: f64,
    cursor_y: f64,
    screen_x: f64,
    screen_y: f64,
    screen_w: f64,
    screen_h: f64,
) -> Option<Position> {
    if let InputEvent::PointerMotion { dx, dy, .. } = event {
        let adx = dx.abs();
        let ady = dy.abs();

        if adx < 0.5 && ady < 0.5 {
            return None;
        }

        let at_right = cursor_x >= screen_x + screen_w - EDGE_MARGIN;
        let at_left = cursor_x <= screen_x + EDGE_MARGIN;
        let at_bottom = cursor_y >= screen_y + screen_h - EDGE_MARGIN;
        let at_top = cursor_y <= screen_y + EDGE_MARGIN;

        if at_right && *dx > 0.0 && adx > ady {
            return Some(Position::Right);
        }
        if at_left && *dx < 0.0 && adx > ady {
            return Some(Position::Left);
        }
        if at_bottom && *dy > 0.0 && ady > adx {
            return Some(Position::Bottom);
        }
        if at_top && *dy < 0.0 && ady > adx {
            return Some(Position::Top);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SX: f64 = 0.0;
    const SY: f64 = 0.0;
    const SW: f64 = 1920.0;
    const SH: f64 = 1080.0;

    fn motion(dx: f64, dy: f64) -> InputEvent {
        InputEvent::PointerMotion { time: 0, dx, dy }
    }

    #[test]
    fn push_right() {
        let result = detect_edge_push(&motion(5.0, 0.0), SW - 1.0, 500.0, SX, SY, SW, SH);
        assert_eq!(result, Some(Position::Right));
    }

    #[test]
    fn push_left() {
        let result = detect_edge_push(&motion(-5.0, 0.0), 1.0, 500.0, SX, SY, SW, SH);
        assert_eq!(result, Some(Position::Left));
    }

    #[test]
    fn push_bottom() {
        let result = detect_edge_push(&motion(0.0, 5.0), 960.0, SH - 1.0, SX, SY, SW, SH);
        assert_eq!(result, Some(Position::Bottom));
    }

    #[test]
    fn push_top() {
        let result = detect_edge_push(&motion(0.0, -5.0), 960.0, 1.0, SX, SY, SW, SH);
        assert_eq!(result, Some(Position::Top));
    }

    #[test]
    fn not_at_edge() {
        let result = detect_edge_push(&motion(5.0, 0.0), 960.0, 540.0, SX, SY, SW, SH);
        assert_eq!(result, None);
    }

    #[test]
    fn at_edge_but_moving_away() {
        let result = detect_edge_push(&motion(-5.0, 0.0), SW - 1.0, 500.0, SX, SY, SW, SH);
        assert_eq!(result, None);
    }

    #[test]
    fn delta_too_small() {
        let result = detect_edge_push(&motion(0.3, 0.2), SW - 1.0, 500.0, SX, SY, SW, SH);
        assert_eq!(result, None);
    }

    #[test]
    fn diagonal_prefers_dominant_axis() {
        let result = detect_edge_push(&motion(5.0, 3.0), SW - 1.0, SH - 1.0, SX, SY, SW, SH);
        assert_eq!(result, Some(Position::Right));

        let result = detect_edge_push(&motion(3.0, 5.0), SW - 1.0, SH - 1.0, SX, SY, SW, SH);
        assert_eq!(result, Some(Position::Bottom));
    }

    #[test]
    fn non_motion_event_returns_none() {
        let event = InputEvent::KeyboardKey {
            time: 0,
            key: 30,
            state: vesa_event::KeyState::Press,
        };
        let result = detect_edge_push(&event, SW - 1.0, 500.0, SX, SY, SW, SH);
        assert_eq!(result, None);
    }

    #[test]
    fn offset_screen_origin() {
        let result = detect_edge_push(
            &motion(5.0, 0.0),
            1919.0,
            500.0,
            100.0,
            200.0,
            1920.0,
            1080.0,
        );
        assert_eq!(result, None);

        let result = detect_edge_push(
            &motion(5.0, 0.0),
            2019.0,
            500.0,
            100.0,
            200.0,
            1920.0,
            1080.0,
        );
        assert_eq!(result, Some(Position::Right));
    }

    #[test]
    fn edge_margin_boundary() {
        let result = detect_edge_push(&motion(5.0, 0.0), SW - 2.0, 500.0, SX, SY, SW, SH);
        assert_eq!(result, Some(Position::Right));

        let result = detect_edge_push(&motion(5.0, 0.0), SW - 3.0, 500.0, SX, SY, SW, SH);
        assert_eq!(result, None);
    }
}
