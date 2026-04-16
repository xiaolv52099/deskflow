use anyhow::Result;
use core_topology::{EdgeDirection, TopologyLayout};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InputEvent {
    MouseMove { position: CursorPosition },
    MouseWheel { delta_x: f64, delta_y: f64 },
    MouseButtonPress { button: MouseButton },
    MouseButtonRelease { button: MouseButton },
    KeyPress { scancode: u32 },
    KeyRelease { scancode: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputInjectionRequest {
    pub target_device_id: Uuid,
    pub event: InputEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimedInputEvent {
    pub event: InputEvent,
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct InputTuningProfile {
    pub pointer_speed_multiplier: f64,
    pub wheel_speed_multiplier: f64,
    pub wheel_smoothing_factor: f64,
}

impl Default for InputTuningProfile {
    fn default() -> Self {
        Self {
            pointer_speed_multiplier: 1.0,
            wheel_speed_multiplier: 1.0,
            wheel_smoothing_factor: 0.0,
        }
    }
}

impl InputTuningProfile {
    pub fn normalized(self) -> Self {
        Self {
            pointer_speed_multiplier: self.pointer_speed_multiplier.clamp(0.25, 3.0),
            wheel_speed_multiplier: self.wheel_speed_multiplier.clamp(0.25, 3.0),
            wheel_smoothing_factor: self.wheel_smoothing_factor.clamp(0.0, 0.95),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActiveInputTarget {
    Local(Uuid),
    Remote(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputRouteResult {
    pub target: ActiveInputTarget,
    pub forwarded_event: Option<InputInjectionRequest>,
    pub switched_direction: Option<EdgeDirection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TunedInputInjectionRequest {
    pub target_device_id: Uuid,
    pub events: Vec<TimedInputEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TunedInputRouteResult {
    pub target: ActiveInputTarget,
    pub forwarded_events: Option<TunedInputInjectionRequest>,
    pub switched_direction: Option<EdgeDirection>,
}

#[derive(Debug, Clone, Default)]
pub struct InputTuningProfiles {
    profiles: HashMap<Uuid, InputTuningProfile>,
}

impl InputTuningProfiles {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, device_id: Uuid, profile: InputTuningProfile) {
        self.profiles.insert(device_id, profile.normalized());
    }

    pub fn get(&self, device_id: Uuid) -> InputTuningProfile {
        self.profiles.get(&device_id).copied().unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct InputRouter {
    pub local_device_id: Uuid,
    pub active_target: ActiveInputTarget,
    pub screen_width: f64,
    pub screen_height: f64,
}

impl InputRouter {
    pub fn new(local_device_id: Uuid, screen_width: f64, screen_height: f64) -> Self {
        Self {
            local_device_id,
            active_target: ActiveInputTarget::Local(local_device_id),
            screen_width,
            screen_height,
        }
    }

    pub fn route_event(&mut self, topology: &TopologyLayout, event: InputEvent) -> Result<InputRouteResult> {
        match event.clone() {
            InputEvent::MouseMove { position } => self.route_mouse_move(topology, position, event),
            _ => Ok(self.forward_or_keep(event)),
        }
    }

    pub fn route_event_with_tuning(
        &mut self,
        topology: &TopologyLayout,
        event: InputEvent,
        profiles: &InputTuningProfiles,
    ) -> Result<TunedInputRouteResult> {
        let result = self.route_event(topology, event)?;
        let forwarded_events = result.forwarded_event.map(|request| {
            let profile = profiles.get(request.target_device_id);
            TunedInputInjectionRequest {
                target_device_id: request.target_device_id,
                events: tune_input_event_batch(&request.event, profile),
            }
        });

        Ok(TunedInputRouteResult {
            target: result.target,
            forwarded_events,
            switched_direction: result.switched_direction,
        })
    }

    pub fn return_to_local(&mut self) {
        self.active_target = ActiveInputTarget::Local(self.local_device_id);
    }

    fn route_mouse_move(
        &mut self,
        topology: &TopologyLayout,
        position: CursorPosition,
        event: InputEvent,
    ) -> Result<InputRouteResult> {
        if let Some(direction) = detect_boundary_crossing(position, self.screen_width, self.screen_height) {
            if let Some(neighbor) = topology.neighbor(self.local_device_id, direction) {
                self.active_target = ActiveInputTarget::Remote(neighbor.device_id);
                return Ok(InputRouteResult {
                    target: self.active_target,
                    forwarded_event: Some(InputInjectionRequest {
                        target_device_id: neighbor.device_id,
                        event,
                    }),
                    switched_direction: Some(direction),
                });
            }
        }

        Ok(self.forward_or_keep(event))
    }

    fn forward_or_keep(&self, event: InputEvent) -> InputRouteResult {
        match self.active_target {
            ActiveInputTarget::Local(device_id) => InputRouteResult {
                target: ActiveInputTarget::Local(device_id),
                forwarded_event: None,
                switched_direction: None,
            },
            ActiveInputTarget::Remote(target_device_id) => InputRouteResult {
                target: ActiveInputTarget::Remote(target_device_id),
                forwarded_event: Some(InputInjectionRequest {
                    target_device_id,
                    event,
                }),
                switched_direction: None,
            },
        }
    }
}

pub fn tune_input_event(event: &InputEvent, profile: InputTuningProfile) -> InputEvent {
    let profile = profile.normalized();
    match event {
        InputEvent::MouseMove { position } => InputEvent::MouseMove {
            position: CursorPosition {
                x: position.x * profile.pointer_speed_multiplier,
                y: position.y * profile.pointer_speed_multiplier,
            },
        },
        InputEvent::MouseWheel { delta_x, delta_y } => InputEvent::MouseWheel {
            delta_x: apply_smoothing(
                *delta_x * profile.wheel_speed_multiplier,
                profile.wheel_smoothing_factor,
            ),
            delta_y: apply_smoothing(
                *delta_y * profile.wheel_speed_multiplier,
                profile.wheel_smoothing_factor,
            ),
        },
        other => other.clone(),
    }
}

pub fn tune_input_event_batch(event: &InputEvent, profile: InputTuningProfile) -> Vec<TimedInputEvent> {
    let profile = profile.normalized();
    match event {
        InputEvent::MouseWheel { delta_x, delta_y } => smooth_wheel_events(*delta_x, *delta_y, profile),
        _ => vec![TimedInputEvent {
            event: tune_input_event(event, profile),
            delay_ms: 0,
        }],
    }
}

pub fn smooth_wheel_events(delta_x: f64, delta_y: f64, profile: InputTuningProfile) -> Vec<TimedInputEvent> {
    let profile = profile.normalized();
    let scaled_x = delta_x * profile.wheel_speed_multiplier;
    let scaled_y = delta_y * profile.wheel_speed_multiplier;
    if profile.wheel_smoothing_factor <= 0.0 {
        return vec![TimedInputEvent {
            event: InputEvent::MouseWheel {
                delta_x: scaled_x,
                delta_y: scaled_y,
            },
            delay_ms: 0,
        }];
    }

    let steps = (1.0 + profile.wheel_smoothing_factor * 7.0).round().clamp(2.0, 8.0) as u64;
    let delay_step_ms = (2.0 + profile.wheel_smoothing_factor * 6.0).round() as u64;
    (0..steps)
        .map(|index| TimedInputEvent {
            event: InputEvent::MouseWheel {
                delta_x: scaled_x / steps as f64,
                delta_y: scaled_y / steps as f64,
            },
            delay_ms: index * delay_step_ms,
        })
        .collect()
}

fn apply_smoothing(value: f64, factor: f64) -> f64 {
    if factor <= 0.0 {
        value
    } else {
        let clamped = factor.clamp(0.0, 0.95);
        let sign = value.signum();
        let magnitude = value.abs();
        sign * (magnitude * (1.0 + clamped))
    }
}

pub fn detect_boundary_crossing(
    position: CursorPosition,
    screen_width: f64,
    screen_height: f64,
) -> Option<EdgeDirection> {
    if position.x <= 0.0 {
        return Some(EdgeDirection::Left);
    }
    if position.x >= screen_width {
        return Some(EdgeDirection::Right);
    }
    if position.y <= 0.0 {
        return Some(EdgeDirection::Up);
    }
    if position.y >= screen_height {
        return Some(EdgeDirection::Down);
    }
    None
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlatformPermissionState {
    Granted,
    RequiresUserAction,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlatformInputStatus {
    pub platform: String,
    pub capture_ready: bool,
    pub injection_ready: bool,
    pub cursor_query_ready: bool,
    pub permission_state: PlatformPermissionState,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CursorSample {
    pub position: CursorPosition,
    pub observed_at_unix_ms: u128,
}

#[cfg(target_os = "windows")]
pub fn current_platform_input_status() -> PlatformInputStatus {
    PlatformInputStatus {
        platform: "windows".into(),
        capture_ready: true,
        injection_ready: true,
        cursor_query_ready: true,
        permission_state: PlatformPermissionState::Granted,
        note: "Win32 cursor query and SendInput bridge available".into(),
    }
}

#[cfg(target_os = "macos")]
pub fn current_platform_input_status() -> PlatformInputStatus {
    PlatformInputStatus {
        platform: "macos".into(),
        capture_ready: false,
        injection_ready: false,
        cursor_query_ready: false,
        permission_state: PlatformPermissionState::RequiresUserAction,
        note: "macOS adapter requires Accessibility and Input Monitoring bridge on macOS host".into(),
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn current_platform_input_status() -> PlatformInputStatus {
    PlatformInputStatus {
        platform: std::env::consts::OS.into(),
        capture_ready: false,
        injection_ready: false,
        cursor_query_ready: false,
        permission_state: PlatformPermissionState::Unsupported,
        note: "platform adapter not implemented".into(),
    }
}

#[cfg(target_os = "windows")]
pub fn sample_cursor_position() -> Result<CursorSample> {
    windows_platform::sample_cursor_position()
}

#[cfg(not(target_os = "windows"))]
pub fn sample_cursor_position() -> Result<CursorSample> {
    anyhow::bail!("cursor sampling is only implemented on Windows in this environment")
}

#[cfg(target_os = "windows")]
pub fn inject_remote_event(event: &InputEvent) -> Result<()> {
    windows_platform::inject_remote_event(event)
}

#[cfg(not(target_os = "windows"))]
pub fn inject_remote_event(_event: &InputEvent) -> Result<()> {
    anyhow::bail!("input injection is only implemented on Windows in this environment")
}

#[cfg(target_os = "windows")]
pub fn windows_capture_ready() -> bool {
    true
}

#[cfg(not(target_os = "windows"))]
pub fn windows_capture_ready() -> bool {
    false
}

#[cfg(target_os = "windows")]
mod windows_platform {
    use super::{CursorPosition, CursorSample, InputEvent, MouseButton};
    use anyhow::{Context, Result};
    use std::mem::size_of;
    use std::time::{SystemTime, UNIX_EPOCH};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
        MOUSEINPUT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    pub fn sample_cursor_position() -> Result<CursorSample> {
        let mut point = POINT { x: 0, y: 0 };
        let ok = unsafe { GetCursorPos(&mut point) };
        if ok == 0 {
            anyhow::bail!("GetCursorPos failed");
        }

        Ok(CursorSample {
            position: CursorPosition {
                x: point.x as f64,
                y: point.y as f64,
            },
            observed_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("current time before unix epoch")?
                .as_millis(),
        })
    }

    pub fn inject_remote_event(event: &InputEvent) -> Result<()> {
        match event {
            InputEvent::MouseMove { position } => {
                let ok = unsafe { SetCursorPos(position.x.round() as i32, position.y.round() as i32) };
                if ok == 0 {
                    anyhow::bail!("SetCursorPos failed");
                }
                Ok(())
            }
            InputEvent::MouseWheel { delta_x: _, delta_y } => {
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: wheel_delta(*delta_y),
                            dwFlags: MOUSEEVENTF_WHEEL,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                send_inputs(&[input])
            }
            InputEvent::MouseButtonPress { button } => {
                let input = mouse_button_input(*button, true);
                send_inputs(&[input])
            }
            InputEvent::MouseButtonRelease { button } => {
                let input = mouse_button_input(*button, false);
                send_inputs(&[input])
            }
            InputEvent::KeyPress { scancode } => {
                let input = keyboard_input(*scancode, false);
                send_inputs(&[input])
            }
            InputEvent::KeyRelease { scancode } => {
                let input = keyboard_input(*scancode, true);
                send_inputs(&[input])
            }
        }
    }

    fn send_inputs(inputs: &[INPUT]) -> Result<()> {
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            anyhow::bail!("SendInput sent {sent} of {} events", inputs.len());
        }
        Ok(())
    }

    fn keyboard_input(scancode: u32, key_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scancode as u16,
                    dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn mouse_button_input(button: MouseButton, pressed: bool) -> INPUT {
        let flags = match (button, pressed) {
            (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
            (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
            (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
            (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
            (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
            (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
        };

        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags | MOUSEEVENTF_MOVE & 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn wheel_delta(delta_y: f64) -> u32 {
        let scaled = (delta_y * 120.0).round() as i32;
        scaled as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_topology::{GridPosition, TopologyLayout};

    fn sample_topology() -> (TopologyLayout, Uuid, Uuid) {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut topology = TopologyLayout::new(controller, "controller");
        topology.add_pending_device(client, "client").expect("add pending");
        topology
            .place_device(client, GridPosition { x: 3, y: 1 })
            .expect("place client");
        (topology, controller, client)
    }

    #[test]
    fn boundary_detection_maps_screen_edges() {
        assert_eq!(
            detect_boundary_crossing(CursorPosition { x: 0.0, y: 100.0 }, 1920.0, 1080.0),
            Some(EdgeDirection::Left)
        );
        assert_eq!(
            detect_boundary_crossing(CursorPosition { x: 1920.0, y: 100.0 }, 1920.0, 1080.0),
            Some(EdgeDirection::Right)
        );
        assert_eq!(
            detect_boundary_crossing(CursorPosition { x: 100.0, y: 0.0 }, 1920.0, 1080.0),
            Some(EdgeDirection::Up)
        );
        assert_eq!(
            detect_boundary_crossing(CursorPosition { x: 100.0, y: 1080.0 }, 1920.0, 1080.0),
            Some(EdgeDirection::Down)
        );
    }

    #[test]
    fn mouse_crossing_switches_to_neighbor_device() {
        let (topology, controller, client) = sample_topology();
        let mut router = InputRouter::new(controller, 1920.0, 1080.0);

        let result = router
            .route_event(
                &topology,
                InputEvent::MouseMove {
                    position: CursorPosition {
                        x: 1920.0,
                        y: 400.0,
                    },
                },
            )
            .expect("route move");

        assert_eq!(result.target, ActiveInputTarget::Remote(client));
        assert_eq!(result.switched_direction, Some(EdgeDirection::Right));
        assert!(result.forwarded_event.is_some());
    }

    #[test]
    fn keyboard_event_is_forwarded_when_remote_target_is_active() {
        let (topology, controller, client) = sample_topology();
        let mut router = InputRouter::new(controller, 1920.0, 1080.0);
        router.active_target = ActiveInputTarget::Remote(client);

        let result = router
            .route_event(&topology, InputEvent::KeyPress { scancode: 30 })
            .expect("route key press");

        assert_eq!(result.target, ActiveInputTarget::Remote(client));
        assert_eq!(
            result.forwarded_event,
            Some(InputInjectionRequest {
                target_device_id: client,
                event: InputEvent::KeyPress { scancode: 30 },
            })
        );
    }

    #[test]
    fn local_target_keeps_event_local_until_boundary_hit() {
        let (topology, controller, _) = sample_topology();
        let mut router = InputRouter::new(controller, 1920.0, 1080.0);

        let result = router
            .route_event(
                &topology,
                InputEvent::MouseMove {
                    position: CursorPosition { x: 500.0, y: 400.0 },
                },
            )
            .expect("route move");

        assert_eq!(result.target, ActiveInputTarget::Local(controller));
        assert!(result.forwarded_event.is_none());
    }

    #[test]
    fn return_to_local_resets_remote_target() {
        let (_, controller, client) = sample_topology();
        let mut router = InputRouter::new(controller, 1920.0, 1080.0);
        router.active_target = ActiveInputTarget::Remote(client);

        router.return_to_local();

        assert_eq!(router.active_target, ActiveInputTarget::Local(controller));
    }

    #[test]
    fn routing_completes_under_reasonable_latency_bound() {
        let (topology, controller, _) = sample_topology();
        let mut router = InputRouter::new(controller, 1920.0, 1080.0);
        let started = std::time::Instant::now();

        let result = router
            .route_event(
                &topology,
                InputEvent::MouseMove {
                    position: CursorPosition { x: 500.0, y: 300.0 },
                },
            )
            .expect("route move");

        let elapsed = started.elapsed();
        println!("windows input route elapsed: {elapsed:?}");
        assert!(matches!(result.target, ActiveInputTarget::Local(_)));
        assert!(
            elapsed < std::time::Duration::from_millis(20),
            "input routing took {elapsed:?}"
        );
    }

    #[test]
    fn platform_status_reports_supported_runtime_shape() {
        let status = current_platform_input_status();
        assert!(!status.platform.is_empty());

        #[cfg(target_os = "windows")]
        {
            assert!(status.capture_ready);
            assert!(status.cursor_query_ready);
        }
    }

    #[test]
    fn cursor_sampling_probe_is_fast_when_available() {
        #[cfg(target_os = "windows")]
        {
            let started = std::time::Instant::now();
            let Ok(sample) = sample_cursor_position() else {
                eprintln!("cursor sample unavailable in this execution context");
                return;
            };
            let elapsed = started.elapsed();
            println!("windows cursor sample elapsed: {elapsed:?}");
            assert!(sample.position.x >= 0.0);
            assert!(sample.position.y >= 0.0);
            assert!(
                elapsed < std::time::Duration::from_millis(20),
                "cursor sample took {elapsed:?}"
            );
        }
    }

    #[test]
    fn input_tuning_scales_pointer_and_wheel_events() {
        let profile = InputTuningProfile {
            pointer_speed_multiplier: 1.5,
            wheel_speed_multiplier: 1.25,
            wheel_smoothing_factor: 0.2,
        };

        let pointer = tune_input_event(
            &InputEvent::MouseMove {
                position: CursorPosition { x: 10.0, y: 20.0 },
            },
            profile,
        );
        let wheel = tune_input_event(
            &InputEvent::MouseWheel {
                delta_x: 0.0,
                delta_y: 2.0,
            },
            profile,
        );

        assert_eq!(
            pointer,
            InputEvent::MouseMove {
                position: CursorPosition { x: 15.0, y: 30.0 }
            }
        );
        match wheel {
            InputEvent::MouseWheel { delta_y, .. } => assert!(delta_y > 2.0),
            _ => panic!("expected wheel event"),
        }
    }

    #[test]
    fn wheel_smoothing_subdivides_delta_without_losing_total_distance() {
        let profile = InputTuningProfile {
            pointer_speed_multiplier: 1.0,
            wheel_speed_multiplier: 1.5,
            wheel_smoothing_factor: 0.75,
        };

        let events = smooth_wheel_events(0.0, 8.0, profile);
        let total_delta = events
            .iter()
            .map(|event| match event.event {
                InputEvent::MouseWheel { delta_y, .. } => delta_y,
                _ => 0.0,
            })
            .sum::<f64>();

        assert!(events.len() > 1);
        assert!((total_delta - 12.0).abs() < f64::EPSILON);
        assert!(events.windows(2).all(|pair| pair[1].delay_ms > pair[0].delay_ms));
    }

    #[test]
    fn routed_remote_event_applies_target_device_tuning() {
        let (topology, controller, client) = sample_topology();
        let mut router = InputRouter::new(controller, 1920.0, 1080.0);
        let mut profiles = InputTuningProfiles::new();
        profiles.set(
            client,
            InputTuningProfile {
                pointer_speed_multiplier: 2.0,
                wheel_speed_multiplier: 1.0,
                wheel_smoothing_factor: 0.0,
            },
        );

        let result = router
            .route_event_with_tuning(
                &topology,
                InputEvent::MouseMove {
                    position: CursorPosition {
                        x: 1920.0,
                        y: 400.0,
                    },
                },
                &profiles,
            )
            .expect("route tuned move");

        let forwarded = result.forwarded_events.expect("forwarded event");
        assert_eq!(forwarded.target_device_id, client);
        assert_eq!(
            forwarded.events[0].event,
            InputEvent::MouseMove {
                position: CursorPosition {
                    x: 3840.0,
                    y: 800.0,
                },
            }
        );
    }
}
