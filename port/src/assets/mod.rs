//! Runtime loading of the user's own Assassin's Creed data (iw4L-style: nothing is redistributed).
//! Formats are documented in RE/08 (.forge) and RE/09 (Mesh / Skeleton / TextureMap).

pub mod ac_actions;
pub mod ac_anim;
pub mod ac_formats;
pub mod altair;
pub mod anims;
pub mod forge;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod probe;

use std::path::PathBuf;

/// Game install folder: `AC_GAME_DIR` env var, else the default location used in this project.
pub fn game_dir() -> PathBuf {
    std::env::var_os("AC_GAME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\benja\Desktop\Claude\Assassin's Creed"))
}
