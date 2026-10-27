//! Be right back, and the Scenes the menu bar panel shows.
//!
//! Be right back lives here, not in the window: the window, the menu bar panel and a global
//! shortcut all switch it, and meetings must follow it with the window closed. While it is
//! on, the meeting camera shows a still with a card (see `studio_engine::away`).
//!
//! Scenes are saved in the window. It publishes their names here, the panel lists them, and
//! a choice in the panel goes back to the window, which applies it.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

/// Tells the window and the panel that Be right back changed; the payload is the new value.
pub const AWAY_EVENT: &str = "away-changed";
/// Tells the panel the list of scenes changed.
pub const SCENES_EVENT: &str = "scenes-changed";
/// Asks the window to apply a scene; the payload is its id.
pub const APPLY_SCENE_EVENT: &str = "apply-scene";
/// The global shortcut for Be right back.
pub const SHORTCUT: &str = "Control+Alt+KeyB";

pub fn set(app: &AppHandle, away: bool) -> bool {
    app.state::<AppState>().set_away(away);
    log::info!("Be right back {}", if away { "on" } else { "off" });
    let _ = app.emit(AWAY_EVENT, away);
    away
}

#[tauri::command]
pub fn away(state: State<'_, AppState>) -> bool {
    state.is_away()
}

#[tauri::command]
pub fn set_away(app: AppHandle, away: bool) -> bool {
    set(&app, away)
}

#[tauri::command]
pub fn toggle_away(app: AppHandle) -> bool {
    let now = app.state::<AppState>().is_away();
    set(&app, !now)
}

/// The card the window drew for Be right back: a PNG with transparency, at the output size.
#[tauri::command]
pub fn set_away_card(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("set_away_card needs the PNG bytes".into());
    };
    state.set_away_card(bytes)
}

/// Registers the global shortcut. A shortcut another app already owns is only logged.
pub fn register_shortcut(app: &tauri::App) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let result = app
        .global_shortcut()
        .on_shortcut(SHORTCUT, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                let now = app.state::<AppState>().is_away();
                set(app, !now);
            }
        });
    if let Err(e) = result {
        log::warn!("Could not register the Be right back shortcut: {e}");
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneName {
    pub id: String,
    pub name: String,
}

/// What the panel shows: the scenes, and the one in use (if the look matches one).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneBoard {
    pub scenes: Vec<SceneName>,
    pub active: Option<String>,
}

#[derive(Default)]
pub struct Scenes(Mutex<SceneBoard>);

/// At most this many scenes, with names this long, are kept.
const MAX_SCENES: usize = 12;
const MAX_NAME: usize = 40;

impl SceneBoard {
    fn cleaned(mut self) -> Self {
        self.scenes.truncate(MAX_SCENES);
        for scene in &mut self.scenes {
            scene.name = scene.name.chars().take(MAX_NAME).collect();
        }
        if !self
            .scenes
            .iter()
            .any(|s| Some(&s.id) == self.active.as_ref())
        {
            self.active = None;
        }
        self
    }
}

#[tauri::command]
pub fn publish_scenes(app: AppHandle, scenes: State<'_, Scenes>, board: SceneBoard) {
    let board = board.cleaned();
    *scenes.0.lock().unwrap_or_else(|e| e.into_inner()) = board.clone();
    let _ = app.emit(SCENES_EVENT, board);
}

#[tauri::command]
pub fn scene_board(scenes: State<'_, Scenes>) -> SceneBoard {
    scenes.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
pub fn apply_scene(app: AppHandle, id: String) {
    let _ = app.emit_to("main", APPLY_SCENE_EVENT, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_is_kept_small_and_consistent() {
        let board = SceneBoard {
            scenes: (0..20)
                .map(|i| SceneName {
                    id: i.to_string(),
                    name: "x".repeat(100),
                })
                .collect(),
            active: Some("99".into()),
        }
        .cleaned();
        assert_eq!(board.scenes.len(), MAX_SCENES);
        assert_eq!(board.scenes[0].name.len(), MAX_NAME);
        assert_eq!(board.active, None);
    }
}
