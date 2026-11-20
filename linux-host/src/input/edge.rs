use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::input::x11::cursor::X11CursorController;
use crate::input::x11::display::X11DisplaySession;
use crate::input::x11::grab::X11Grabber;
use crate::protocol::frames::{EdgeReturnRecord, OutboundFrame};
use crate::protocol::wire::EdgeSide;
use crate::transport::multiplexer::SeamTransportMultiplexer;
use crate::transport::{now_monotonic_us, SendReliability};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorTarget {
    Linux,
    Android,
}

pub struct SpatialEdgeEngine {
    multiplexer: Arc<SeamTransportMultiplexer>,
    active_edge: EdgeSide,
    velocity_gain: f64,
    current_target: CursorTarget,
    outward_push: f64,
    last_return_us: u64,
    running: Arc<AtomicBool>,
}

impl SpatialEdgeEngine {
    pub fn new(multiplexer: Arc<SeamTransportMultiplexer>) -> Self {
        Self {
            multiplexer,
            active_edge: EdgeSide::Right,
            velocity_gain: 1.85,
            current_target: CursorTarget::Linux,
            outward_push: 0.0,
            last_return_us: 0,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_active_edge(&mut self, edge: EdgeSide) {
        self.active_edge = edge;
    }

    pub fn set_velocity_gain(&mut self, gain: f64) {
        self.velocity_gain = gain.clamp(0.5, 5.0);
    }

    pub fn current_target(&self) -> CursorTarget {
        self.current_target
    }

    /// Evaluates whether a cursor location at (x, y) should transition into Android
    pub fn evaluate_crossing(&mut self, x: i32, y: i32, dx: f64, dy: f64, width: i32, height: i32) -> bool {
        if self.current_target != CursorTarget::Linux {
            return false;
        }

        // Anti-bounce cooldown (180 ms) after returning to Linux
        let now = now_monotonic_us();
        if self.last_return_us > 0 && now.saturating_sub(self.last_return_us) < 180_000 {
            self.outward_push = 0.0;
            return false;
        }

        let in_safe_vertical = y > 24 && y < height - 16;
        let in_safe_horizontal = x > 16 && x < width - 16;

        let (in_zone, in_band, outward_delta) = match self.active_edge {
            EdgeSide::Right => (width - x <= 3, in_safe_vertical, if dx > 0.0 { dx } else { 0.0 }),
            EdgeSide::Left => (x <= 3, in_safe_vertical, if dx < 0.0 { -dx } else { 0.0 }),
            EdgeSide::Bottom => (height - y <= 3, in_safe_horizontal, if dy > 0.0 { dy } else { 0.0 }),
            EdgeSide::Top => (y <= 3, in_safe_horizontal, if dy < 0.0 { -dy } else { 0.0 }),
        };

        if in_zone && in_band {
            self.outward_push += if outward_delta > 0.0 { outward_delta } else { 0.6 };
            if self.outward_push >= 2.0 {
                self.outward_push = 0.0;
                return true;
            }
        } else {
            self.outward_push = 0.0;
        }
        false
    }

    /// Run the interactive input loop in a background thread or active session
    pub fn run_session(&mut self) {
        let display_session = match X11DisplaySession::open() {
            Some(s) => s,
            None => {
                eprintln!("[EdgeEngine] Could not open X11 display (is DISPLAY set?)");
                return;
            }
        };

        let (width, height) = display_session.dimensions();
        let cursor_ctrl = X11CursorController::new(display_session.display_ptr(), display_session.root_window());
        let mut grabber = X11Grabber::new(
            display_session.display_ptr(),
            display_session.root_window(),
            cursor_ctrl.blank_cursor(),
        );

        self.running.store(true, Ordering::SeqCst);
        let mut prev_loc = display_session.query_pointer().unwrap_or((width / 2, height / 2));

        while self.running.load(Ordering::Relaxed) {
            match self.current_target {
                CursorTarget::Linux => {
                    if let Some((cur_x, cur_y)) = display_session.query_pointer() {
                        let dx = (cur_x - prev_loc.0) as f64;
                        let dy = (cur_y - prev_loc.1) as f64;
                        prev_loc = (cur_x, cur_y);

                        if self.evaluate_crossing(cur_x, cur_y, dx, dy, width, height) {
                            // Compute normalized entry point
                            let ratio_y = (cur_y.clamp(0, height) as f64 / height as f64) * 65535.0;
                            let entry_norm_y = ratio_y.round() as u16;

                            let enter_frame = OutboundFrame::CursorEnter {
                                edge: EdgeSide::Left, // Android entry is Left if Linux exit is Right
                                norm_x: 360,
                                norm_y: entry_norm_y,
                                flags: 0,
                            };
                            let _ = self.multiplexer.send(&enter_frame, SendReliability::ReliableOrdered);

                            // Grab input
                            if grabber.grab(cur_x, cur_y) {
                                cursor_ctrl.hide();
                                self.current_target = CursorTarget::Android;
                                println!("[EdgeEngine] Switched input target -> Android (Remote)");
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(8)); // ~120 Hz edge poll
                }
                CursorTarget::Android => {
                    let (frames, panic_escape) = grabber.process_events();

                    if panic_escape {
                        println!("[EdgeEngine] Panic escape key pressed! Returning to Linux...");
                        self.force_return_to_linux(&mut grabber, &cursor_ctrl, width, height);
                        continue;
                    }

                    for frame in frames {
                        match frame {
                            OutboundFrame::MouseReport { buttons, dx, dy, v_wheel, h_wheel } => {
                                let scaled_dx = ((dx as f64) * self.velocity_gain).round() as i16;
                                let scaled_dy = ((dy as f64) * self.velocity_gain).round() as i16;
                                let report = OutboundFrame::MouseReport {
                                    buttons,
                                    dx: scaled_dx,
                                    dy: scaled_dy,
                                    v_wheel,
                                    h_wheel,
                                };
                                let rel = if buttons == 0 && v_wheel == 0 && h_wheel == 0 {
                                    SendReliability::UnreliableSequenced
                                } else {
                                    SendReliability::ReliableOrdered
                                };
                                let _ = self.multiplexer.send(&report, rel);
                            }
                            other => {
                                let _ = self.multiplexer.send(&other, SendReliability::ReliableOrdered);
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(2)); // High-rate input pump
                }
            }
        }

        grabber.ungrab();
        cursor_ctrl.show();
    }

    pub fn handle_android_edge_return(&mut self, ret: &EdgeReturnRecord, width: i32, height: i32) {
        if self.current_target != CursorTarget::Android {
            return;
        }

        let return_y = ((ret.norm_y as f64 / 65535.0) * height as f64).round() as i32;
        let return_x = match self.active_edge {
            EdgeSide::Right => width - 6,
            EdgeSide::Left => 6,
            EdgeSide::Bottom => ((ret.norm_x as f64 / 65535.0) * width as f64).round() as i32,
            EdgeSide::Top => ((ret.norm_x as f64 / 65535.0) * width as f64).round() as i32,
        };

        if let Some(session) = X11DisplaySession::open() {
            let cursor_ctrl = X11CursorController::new(session.display_ptr(), session.root_window());
            let mut grabber = X11Grabber::new(session.display_ptr(), session.root_window(), cursor_ctrl.blank_cursor());
            grabber.ungrab();
            cursor_ctrl.warp(return_x, return_y);
            cursor_ctrl.show();
        }

        self.current_target = CursorTarget::Linux;
        self.last_return_us = now_monotonic_us();
        self.outward_push = 0.0;
        println!("[EdgeEngine] Switched input target -> Linux (Local) at ({}, {})", return_x, return_y);
    }

    fn force_return_to_linux(
        &mut self,
        grabber: &mut X11Grabber,
        cursor_ctrl: &X11CursorController,
        width: i32,
        height: i32,
    ) {
        let _ = self.multiplexer.send(&OutboundFrame::CursorLeave { reason: 1 }, SendReliability::ReliableOrdered);
        grabber.ungrab();
        cursor_ctrl.warp(width / 2, height / 2);
        cursor_ctrl.show();
        self.current_target = CursorTarget::Linux;
        self.last_return_us = now_monotonic_us();
        self.outward_push = 0.0;
    }
}
