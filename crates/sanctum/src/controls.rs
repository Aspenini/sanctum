//! On-screen overlay and gamepad binding presets.

use gilrs::{Axis, Button, EventType, Gilrs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlKind {
    Dpad,
    Button,
    Stick,
    MousePad,
}

impl ControlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dpad => "dpad",
            Self::Button => "button",
            Self::Stick => "stick",
            Self::MousePad => "mouse",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "dpad" => Self::Dpad,
            "stick" => Self::Stick,
            "mouse" => Self::MousePad,
            _ => Self::Button,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ControlRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl ControlRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }.clamped()
    }

    pub fn clamped(self) -> Self {
        let w = self.w.clamp(0.08, 1.0);
        let h = self.h.clamp(0.08, 1.0);
        Self {
            x: self.x.clamp(0.0, (1.0 - w).max(0.0)),
            y: self.y.clamp(0.0, (1.0 - h).max(0.0)),
            w,
            h,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Action {
    Key {
        name: String,
    },
    Dpad {
        up: String,
        down: String,
        left: String,
        right: String,
    },
    MouseLeft,
    MouseRight,
    MouseMove,
}

impl Action {
    fn key(name: &str) -> Self {
        Self::Key {
            name: name.to_string(),
        }
    }

    fn arrows() -> Self {
        Self::Dpad {
            up: "up".into(),
            down: "down".into(),
            left: "left".into(),
            right: "right".into(),
        }
    }

    fn wasd() -> Self {
        Self::Dpad {
            up: "w".into(),
            down: "s".into(),
            left: "a".into(),
            right: "d".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OverlayControl {
    pub id: String,
    pub kind: ControlKind,
    pub label: String,
    pub portrait: ControlRect,
    pub landscape: ControlRect,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GamepadSource {
    South,
    East,
    West,
    North,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Start,
    Select,
    LeftStickClick,
    RightStickClick,
    LeftStick,
    RightStick,
}

impl GamepadSource {
    pub fn id(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
            Self::North => "north",
            Self::DpadUp => "dpad-up",
            Self::DpadDown => "dpad-down",
            Self::DpadLeft => "dpad-left",
            Self::DpadRight => "dpad-right",
            Self::LeftShoulder => "left-shoulder",
            Self::RightShoulder => "right-shoulder",
            Self::LeftTrigger => "left-trigger",
            Self::RightTrigger => "right-trigger",
            Self::Start => "start",
            Self::Select => "select",
            Self::LeftStickClick => "left-stick-click",
            Self::RightStickClick => "right-stick-click",
            Self::LeftStick => "left-stick",
            Self::RightStick => "right-stick",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "south" => Self::South,
            "east" => Self::East,
            "west" => Self::West,
            "north" => Self::North,
            "dpad-up" => Self::DpadUp,
            "dpad-down" => Self::DpadDown,
            "dpad-left" => Self::DpadLeft,
            "dpad-right" => Self::DpadRight,
            "left-shoulder" => Self::LeftShoulder,
            "right-shoulder" => Self::RightShoulder,
            "left-trigger" => Self::LeftTrigger,
            "right-trigger" => Self::RightTrigger,
            "start" => Self::Start,
            "select" => Self::Select,
            "left-stick-click" => Self::LeftStickClick,
            "right-stick-click" => Self::RightStickClick,
            "left-stick" => Self::LeftStick,
            "right-stick" => Self::RightStick,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::South => "A / South",
            Self::East => "B / East",
            Self::West => "X / West",
            Self::North => "Y / North",
            Self::DpadUp => "D-pad up",
            Self::DpadDown => "D-pad down",
            Self::DpadLeft => "D-pad left",
            Self::DpadRight => "D-pad right",
            Self::LeftShoulder => "Left bumper",
            Self::RightShoulder => "Right bumper",
            Self::LeftTrigger => "Left trigger",
            Self::RightTrigger => "Right trigger",
            Self::Start => "Start",
            Self::Select => "Select",
            Self::LeftStickClick => "Left stick click",
            Self::RightStickClick => "Right stick click",
            Self::LeftStick => "Left stick",
            Self::RightStick => "Right stick",
        }
    }

    fn from_button(button: Button) -> Option<Self> {
        Some(match button {
            Button::South => Self::South,
            Button::East => Self::East,
            Button::West => Self::West,
            Button::North => Self::North,
            Button::DPadUp => Self::DpadUp,
            Button::DPadDown => Self::DpadDown,
            Button::DPadLeft => Self::DpadLeft,
            Button::DPadRight => Self::DpadRight,
            Button::LeftTrigger => Self::LeftShoulder,
            Button::RightTrigger => Self::RightShoulder,
            Button::LeftTrigger2 => Self::LeftTrigger,
            Button::RightTrigger2 => Self::RightTrigger,
            Button::Start => Self::Start,
            Button::Select => Self::Select,
            Button::LeftThumb => Self::LeftStickClick,
            Button::RightThumb => Self::RightStickClick,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GamepadMap {
    pub source: GamepadSource,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InputPreset {
    pub name: String,
    pub overlay: Vec<OverlayControl>,
    pub gamepad: Vec<GamepadMap>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputSettings {
    #[serde(default = "default_overlay_enabled")]
    pub overlay_enabled: bool,
    #[serde(default = "default_preset_name")]
    pub active_preset: String,
    #[serde(default = "default_presets")]
    pub presets: Vec<InputPreset>,
}

fn default_overlay_enabled() -> bool {
    cfg!(target_os = "android")
}

fn default_preset_name() -> String {
    "TempleOS".into()
}

fn default_presets() -> Vec<InputPreset> {
    vec![templeos_preset(), wasd_preset()]
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            overlay_enabled: default_overlay_enabled(),
            active_preset: default_preset_name(),
            presets: default_presets(),
        }
    }
}

impl InputSettings {
    pub fn ensure_defaults(&mut self) {
        if self.presets.is_empty() {
            self.presets = default_presets();
        }
        if !self
            .presets
            .iter()
            .any(|preset| preset.name == self.active_preset)
        {
            self.active_preset = self.presets[0].name.clone();
        }
    }

    pub fn active(&self) -> &InputPreset {
        self.presets
            .iter()
            .find(|preset| preset.name == self.active_preset)
            .or_else(|| self.presets.first())
            .expect("input presets are never empty")
    }

    pub fn active_mut(&mut self) -> &mut InputPreset {
        self.ensure_defaults();
        let name = self.active_preset.clone();
        let index = self
            .presets
            .iter()
            .position(|preset| preset.name == name)
            .unwrap_or(0);
        &mut self.presets[index]
    }

    pub fn select(&mut self, name: &str) -> bool {
        if self.presets.iter().any(|preset| preset.name == name) {
            self.active_preset = name.to_string();
            true
        } else {
            false
        }
    }

    pub fn save_as(&mut self, name: String) {
        let mut preset = self.active().clone();
        preset.name = name.clone();
        if let Some(existing) = self
            .presets
            .iter_mut()
            .find(|candidate| candidate.name == name)
        {
            *existing = preset;
        } else {
            self.presets.push(preset);
        }
        self.active_preset = name;
    }

    pub fn delete_active(&mut self) {
        if self.presets.len() < 2 {
            return;
        }
        let name = self.active_preset.clone();
        self.presets.retain(|preset| preset.name != name);
        self.active_preset = self.presets[0].name.clone();
    }

    pub fn reset_active(&mut self) {
        let name = self.active_preset.clone();
        if let Some(stock) = default_presets()
            .into_iter()
            .find(|preset| preset.name == name)
        {
            if let Some(current) = self
                .presets
                .iter_mut()
                .find(|preset| preset.name == name)
            {
                *current = stock;
            }
        }
    }
}

fn overlay(
    id: &str,
    kind: ControlKind,
    label: &str,
    portrait: ControlRect,
    landscape: ControlRect,
    action: Action,
) -> OverlayControl {
    OverlayControl {
        id: id.into(),
        kind,
        label: label.into(),
        portrait,
        landscape,
        action,
    }
}

fn templeos_preset() -> InputPreset {
    InputPreset {
        name: "TempleOS".into(),
        overlay: vec![
            overlay(
                "dpad",
                ControlKind::Dpad,
                "D-pad",
                ControlRect::new(0.04, 0.28, 0.40, 0.66),
                ControlRect::new(0.03, 0.56, 0.24, 0.38),
                Action::arrows(),
            ),
            overlay(
                "a",
                ControlKind::Button,
                "A",
                ControlRect::new(0.74, 0.46, 0.22, 0.28),
                ControlRect::new(0.82, 0.64, 0.14, 0.18),
                Action::key("space"),
            ),
            overlay(
                "b",
                ControlKind::Button,
                "B",
                ControlRect::new(0.52, 0.62, 0.20, 0.24),
                ControlRect::new(0.68, 0.74, 0.13, 0.16),
                Action::key("esc"),
            ),
            overlay(
                "start",
                ControlKind::Button,
                "Start",
                ControlRect::new(0.52, 0.22, 0.20, 0.16),
                ControlRect::new(0.42, 0.88, 0.16, 0.10),
                Action::key("enter"),
            ),
            overlay(
                "mouse",
                ControlKind::MousePad,
                "Mouse",
                ControlRect::new(0.52, 0.04, 0.44, 0.16),
                ControlRect::new(0.70, 0.16, 0.26, 0.36),
                Action::MouseMove,
            ),
        ],
        gamepad: default_gamepad(Action::arrows()),
    }
}

fn wasd_preset() -> InputPreset {
    let mut preset = templeos_preset();
    preset.name = "WASD".into();
    for control in &mut preset.overlay {
        if control.id == "dpad" {
            control.action = Action::wasd();
        }
    }
    preset.gamepad = default_gamepad(Action::wasd());
    preset
}

fn default_gamepad(dpad: Action) -> Vec<GamepadMap> {
    vec![
        GamepadMap {
            source: GamepadSource::LeftStick,
            action: dpad.clone(),
        },
        GamepadMap {
            source: GamepadSource::DpadUp,
            action: Action::key("up"),
        },
        GamepadMap {
            source: GamepadSource::DpadDown,
            action: Action::key("down"),
        },
        GamepadMap {
            source: GamepadSource::DpadLeft,
            action: Action::key("left"),
        },
        GamepadMap {
            source: GamepadSource::DpadRight,
            action: Action::key("right"),
        },
        GamepadMap {
            source: GamepadSource::South,
            action: Action::key("space"),
        },
        GamepadMap {
            source: GamepadSource::East,
            action: Action::key("esc"),
        },
        GamepadMap {
            source: GamepadSource::West,
            action: Action::key("ctrl"),
        },
        GamepadMap {
            source: GamepadSource::North,
            action: Action::key("shift"),
        },
        GamepadMap {
            source: GamepadSource::Start,
            action: Action::key("enter"),
        },
        GamepadMap {
            source: GamepadSource::Select,
            action: Action::key("esc"),
        },
        GamepadMap {
            source: GamepadSource::RightStick,
            action: Action::MouseMove,
        },
        GamepadMap {
            source: GamepadSource::RightTrigger,
            action: Action::MouseLeft,
        },
        GamepadMap {
            source: GamepadSource::LeftTrigger,
            action: Action::MouseRight,
        },
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamedKey {
    pub name: &'static str,
    pub label: &'static str,
    pub ch: i64,
    pub scan: i64,
}

pub fn named_keys() -> Vec<NamedKey> {
    let mut keys = vec![
        NamedKey {
            name: "up",
            label: "Up",
            ch: 0,
            scan: 0x48,
        },
        NamedKey {
            name: "down",
            label: "Down",
            ch: 0,
            scan: 0x50,
        },
        NamedKey {
            name: "left",
            label: "Left",
            ch: 0,
            scan: 0x4b,
        },
        NamedKey {
            name: "right",
            label: "Right",
            ch: 0,
            scan: 0x4d,
        },
        NamedKey {
            name: "space",
            label: "Space",
            ch: b' ' as i64,
            scan: 0x39,
        },
        NamedKey {
            name: "enter",
            label: "Enter",
            ch: b'\n' as i64,
            scan: 0x1c,
        },
        NamedKey {
            name: "esc",
            label: "Esc",
            ch: 0x1b,
            scan: 0x01,
        },
        NamedKey {
            name: "tab",
            label: "Tab",
            ch: b'\t' as i64,
            scan: 0x0f,
        },
        NamedKey {
            name: "shift",
            label: "Shift",
            ch: 0,
            scan: 0x2a,
        },
        NamedKey {
            name: "ctrl",
            label: "Ctrl",
            ch: 0,
            scan: 0x1d,
        },
        NamedKey {
            name: "alt",
            label: "Alt",
            ch: 0,
            scan: 0x38,
        },
    ];
    let letters = [
        ("a", 0x1e),
        ("b", 0x30),
        ("c", 0x2e),
        ("d", 0x20),
        ("e", 0x12),
        ("f", 0x21),
        ("g", 0x22),
        ("h", 0x23),
        ("i", 0x17),
        ("j", 0x24),
        ("k", 0x25),
        ("l", 0x26),
        ("m", 0x32),
        ("n", 0x31),
        ("o", 0x18),
        ("p", 0x19),
        ("q", 0x10),
        ("r", 0x13),
        ("s", 0x1f),
        ("t", 0x14),
        ("u", 0x16),
        ("v", 0x2f),
        ("w", 0x11),
        ("x", 0x2d),
        ("y", 0x15),
        ("z", 0x2c),
    ];
    for (name, scan) in letters {
        keys.push(NamedKey {
            name,
            label: name,
            ch: name.as_bytes()[0] as i64,
            scan,
        });
    }
    keys
}

pub fn lookup_key(name: &str) -> Option<(i64, i64)> {
    named_keys()
        .into_iter()
        .find(|key| key.name.eq_ignore_ascii_case(name))
        .map(|key| (key.ch, key.scan))
}

pub fn action_label(action: &Action) -> String {
    match action {
        Action::Key { name } => named_keys()
            .into_iter()
            .find(|key| key.name == name)
            .map(|key| key.label.to_string())
            .unwrap_or_else(|| name.clone()),
        Action::Dpad {
            up,
            down,
            left,
            right,
        } => format!("{up}/{left}/{down}/{right}"),
        Action::MouseLeft => "Mouse left".into(),
        Action::MouseRight => "Mouse right".into(),
        Action::MouseMove => "Mouse move".into(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PadDir {
    Up,
    Down,
    Left,
    Right,
}

pub fn dpad_direction(nx: f32, ny: f32) -> Option<PadDir> {
    let dx = nx.clamp(0.0, 1.0) - 0.5;
    let dy = ny.clamp(0.0, 1.0) - 0.5;
    if dx * dx + dy * dy < 0.04 {
        return None;
    }
    if dx.abs() > dy.abs() {
        if dx < 0.0 {
            Some(PadDir::Left)
        } else {
            Some(PadDir::Right)
        }
    } else if dy < 0.0 {
        Some(PadDir::Up)
    } else {
        Some(PadDir::Down)
    }
}

fn action_key(action: &Action, dir: Option<PadDir>) -> Option<(i64, i64)> {
    match (action, dir) {
        (Action::Key { name }, _) => lookup_key(name),
        (
            Action::Dpad {
                up,
                down,
                left,
                right,
            },
            Some(dir),
        ) => lookup_key(match dir {
            PadDir::Up => up,
            PadDir::Down => down,
            PadDir::Left => left,
            PadDir::Right => right,
        }),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    Key(i64, i64),
    Mouse {
        x: i64,
        y: i64,
        left: bool,
        right: bool,
    },
}

#[derive(Clone, Debug)]
pub enum ListenTarget {
    Overlay(String),
    Gamepad(GamepadSource),
}

struct HeldKey {
    ch: i64,
    scan: i64,
    last: Instant,
}

pub struct InputRuntime {
    gilrs: Option<Gilrs>,
    held: HashMap<String, HeldKey>,
    mouse_x: i64,
    mouse_y: i64,
    mouse_left: bool,
    mouse_right: bool,
    left_stick: (f32, f32),
    right_stick: (f32, f32),
    pub listen: Option<ListenTarget>,
    pub last_capture: Option<String>,
}

impl InputRuntime {
    pub fn new() -> Self {
        Self {
            gilrs: Gilrs::new().ok(),
            held: HashMap::new(),
            mouse_x: 320,
            mouse_y: 240,
            mouse_left: false,
            mouse_right: false,
            left_stick: (0.0, 0.0),
            right_stick: (0.0, 0.0),
            listen: None,
            last_capture: None,
        }
    }

    pub fn overlay_press(
        &mut self,
        preset: &InputPreset,
        id: &str,
        nx: f32,
        ny: f32,
        frame_width: i64,
        frame_height: i64,
    ) -> Vec<InputEvent> {
        let Some(control) = preset.overlay.iter().find(|item| item.id == id) else {
            return Vec::new();
        };
        self.apply_control(
            &format!("overlay:{id}"),
            &control.action,
            Some((nx, ny)),
            true,
            frame_width,
            frame_height,
        )
    }

    pub fn overlay_move(
        &mut self,
        preset: &InputPreset,
        id: &str,
        nx: f32,
        ny: f32,
        frame_width: i64,
        frame_height: i64,
    ) -> Vec<InputEvent> {
        let Some(control) = preset.overlay.iter().find(|item| item.id == id) else {
            return Vec::new();
        };
        self.apply_control(
            &format!("overlay:{id}"),
            &control.action,
            Some((nx, ny)),
            true,
            frame_width,
            frame_height,
        )
    }

    pub fn overlay_release(&mut self, id: &str) -> Vec<InputEvent> {
        self.held.remove(&format!("overlay:{id}"));
        if id == "mouse" {
            self.mouse_left = false;
            return vec![self.mouse_event()];
        }
        Vec::new()
    }

    pub fn repeats(&mut self) -> Vec<InputEvent> {
        let now = Instant::now();
        let mut events = Vec::new();
        for held in self.held.values_mut() {
            if now.duration_since(held.last) >= Duration::from_millis(40) {
                held.last = now;
                events.push(InputEvent::Key(held.ch, held.scan));
            }
        }
        events
    }

    pub fn poll_gamepad(
        &mut self,
        preset: &InputPreset,
        frame_width: i64,
        frame_height: i64,
    ) -> Vec<InputEvent> {
        let mut pressed = Vec::new();
        let mut released = Vec::new();
        let mut axes = Vec::new();
        if let Some(gilrs) = self.gilrs.as_mut() {
            while let Some(event) = gilrs.next_event() {
                match event.event {
                    EventType::ButtonPressed(button, _) => {
                        if let Some(source) = GamepadSource::from_button(button) {
                            pressed.push(source);
                        }
                    }
                    EventType::ButtonReleased(button, _) => {
                        if let Some(source) = GamepadSource::from_button(button) {
                            released.push(source);
                        }
                    }
                    EventType::AxisChanged(axis, value, _) => axes.push((axis, value)),
                    _ => {}
                }
            }
        }
        for (axis, value) in axes {
            match axis {
                Axis::LeftStickX => self.left_stick.0 = value,
                Axis::LeftStickY => self.left_stick.1 = -value,
                Axis::RightStickX => self.right_stick.0 = value,
                Axis::RightStickY => self.right_stick.1 = -value,
                _ => {}
            }
        }
        let mut events = Vec::new();
        for source in pressed {
            if self.listen.is_some() {
                self.last_capture = Some(source.label().into());
                continue;
            }
            if let Some(map) = preset.gamepad.iter().find(|item| item.source == source) {
                events.extend(self.apply_control(
                    &format!("pad:{source:?}"),
                    &map.action,
                    None,
                    true,
                    frame_width,
                    frame_height,
                ));
            }
        }
        for source in released {
            self.held.remove(&format!("pad:{source:?}"));
            if source == GamepadSource::RightTrigger {
                self.mouse_left = false;
                events.push(self.mouse_event());
            } else if source == GamepadSource::LeftTrigger {
                self.mouse_right = false;
                events.push(self.mouse_event());
            }
        }
        events.extend(self.apply_stick(
            preset,
            GamepadSource::LeftStick,
            self.left_stick,
            frame_width,
            frame_height,
        ));
        events.extend(self.apply_stick(
            preset,
            GamepadSource::RightStick,
            self.right_stick,
            frame_width,
            frame_height,
        ));
        events
    }

    fn apply_stick(
        &mut self,
        preset: &InputPreset,
        source: GamepadSource,
        stick: (f32, f32),
        frame_width: i64,
        frame_height: i64,
    ) -> Vec<InputEvent> {
        let Some(map) = preset.gamepad.iter().find(|item| item.source == source) else {
            return Vec::new();
        };
        let nx = 0.5 + stick.0 / 2.0;
        let ny = 0.5 + stick.1 / 2.0;
        if stick.0 * stick.0 + stick.1 * stick.1 < 0.04 {
            self.held.remove(&format!("pad:{source:?}"));
            return Vec::new();
        }
        self.apply_control(
            &format!("pad:{source:?}"),
            &map.action,
            Some((nx, ny)),
            true,
            frame_width,
            frame_height,
        )
    }

    fn apply_control(
        &mut self,
        hold_id: &str,
        action: &Action,
        pos: Option<(f32, f32)>,
        pressed: bool,
        frame_width: i64,
        frame_height: i64,
    ) -> Vec<InputEvent> {
        match action {
            Action::MouseMove => {
                let (nx, ny) = pos.unwrap_or((0.5, 0.5));
                self.mouse_x = ((nx.clamp(0.0, 1.0) as f64) * (frame_width.saturating_sub(1) as f64))
                    .round() as i64;
                self.mouse_y = ((ny.clamp(0.0, 1.0) as f64)
                    * (frame_height.saturating_sub(1) as f64))
                    .round() as i64;
                if hold_id.contains("mouse") || hold_id.contains("overlay:mouse") {
                    self.mouse_left = pressed;
                }
                vec![self.mouse_event()]
            }
            Action::MouseLeft => {
                self.mouse_left = pressed;
                vec![self.mouse_event()]
            }
            Action::MouseRight => {
                self.mouse_right = pressed;
                vec![self.mouse_event()]
            }
            Action::Key { .. } | Action::Dpad { .. } => {
                let dir = pos.and_then(|(x, y)| dpad_direction(x, y));
                if let Some((ch, scan)) = action_key(action, dir) {
                    self.held.insert(
                        hold_id.into(),
                        HeldKey {
                            ch,
                            scan,
                            last: Instant::now(),
                        },
                    );
                    vec![InputEvent::Key(ch, scan)]
                } else {
                    self.held.remove(hold_id);
                    Vec::new()
                }
            }
        }
    }

    fn mouse_event(&self) -> InputEvent {
        InputEvent::Mouse {
            x: self.mouse_x,
            y: self.mouse_y,
            left: self.mouse_left,
            right: self.mouse_right,
        }
    }

    pub fn bind_overlay_key(preset: &mut InputPreset, id: &str, key_name: &str) -> bool {
        let Some(control) = preset.overlay.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        match control.kind {
            ControlKind::Dpad | ControlKind::Stick => false,
            _ => {
                control.action = Action::key(key_name);
                true
            }
        }
    }

    pub fn bind_overlay_action(preset: &mut InputPreset, id: &str, action: Action) -> bool {
        let Some(control) = preset.overlay.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        control.action = action;
        true
    }

    pub fn bind_gamepad(preset: &mut InputPreset, source: GamepadSource, action: Action) {
        if let Some(existing) = preset
            .gamepad
            .iter_mut()
            .find(|item| item.source == source)
        {
            existing.action = action;
        } else {
            preset.gamepad.push(GamepadMap { source, action });
        }
    }

    pub fn move_overlay(preset: &mut InputPreset, id: &str, landscape: bool, rect: ControlRect) {
        if let Some(control) = preset.overlay.iter_mut().find(|item| item.id == id) {
            if landscape {
                control.landscape = rect.clamped();
            } else {
                control.portrait = rect.clamped();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpad_picks_cardinal_direction() {
        assert_eq!(dpad_direction(0.9, 0.5), Some(PadDir::Right));
        assert_eq!(dpad_direction(0.1, 0.5), Some(PadDir::Left));
        assert_eq!(dpad_direction(0.5, 0.1), Some(PadDir::Up));
        assert_eq!(dpad_direction(0.5, 0.9), Some(PadDir::Down));
        assert_eq!(dpad_direction(0.5, 0.5), None);
    }

    #[test]
    fn templeos_preset_round_trips() {
        let settings = InputSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let loaded: InputSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.active_preset, "TempleOS");
        assert_eq!(loaded.active().overlay.len(), 5);
        assert!(
            loaded
                .active()
                .gamepad
                .iter()
                .any(|item| item.source == GamepadSource::South)
        );
    }

    #[test]
    fn overlay_space_button_emits_templeos_space() {
        let preset = templeos_preset();
        let mut runtime = InputRuntime {
            gilrs: None,
            ..InputRuntime::new()
        };
        runtime.gilrs = None;
        let events = runtime.overlay_press(&preset, "a", 0.5, 0.5, 640, 480);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, InputEvent::Key(ch, scan) if *ch == b' ' as i64 && *scan == 0x39))
        );
    }

    #[test]
    fn saving_a_preset_keeps_the_new_name_selected() {
        let mut settings = InputSettings::default();
        settings.save_as("Arcade".into());
        assert_eq!(settings.active_preset, "Arcade");
        assert_eq!(settings.presets.len(), 3);
        settings.delete_active();
        assert_eq!(settings.presets.len(), 2);
    }

    #[test]
    fn lookup_covers_arrows_and_letters() {
        assert_eq!(lookup_key("up"), Some((0, 0x48)));
        assert_eq!(lookup_key("w"), Some((b'w' as i64, 0x11)));
        assert_eq!(lookup_key("enter"), Some((b'\n' as i64, 0x1c)));
    }
}
