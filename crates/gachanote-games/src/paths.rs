//! 常见的游戏日志与安装位置（[`PathProvider`] 的默认实现）。
//!
//! 只列出候选路径、过滤掉不存在的，不访问注册表；用户手动选择的目录通过
//! [`StandardPaths::with_extra`] 放在最前面。路径都由构造参数决定，便于在 Linux CI 上测试。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gachanote_core::plugin::PathProvider;
use gachanote_core::GameId;

/// 鸣潮在各盘符下的常见安装目录（官方启动器、Steam、Epic）。
const WUWA_DIRS: [&str; 7] = [
    "Wuthering Waves",
    "Program Files/Wuthering Waves",
    "Games/Wuthering Waves",
    "Program Files (x86)/Steam/steamapps/common/Wuthering Waves",
    "SteamLibrary/steamapps/common/Wuthering Waves",
    "Program Files/Epic Games/WutheringWavesj3oFh",
    "Epic Games/WutheringWavesj3oFh",
];

/// 米哈游三款的 Unity 日志（相对 `%USERPROFILE%/AppData/LocalLow`）。
fn unity_logs(game: &str) -> &'static [&'static str] {
    match game {
        GameId::GENSHIN => &[
            "miHoYo/原神/output_log.txt",
            "miHoYo/Genshin Impact/output_log.txt",
        ],
        GameId::STARRAIL => &[
            "miHoYo/崩坏：星穹铁道/Player.log",
            "Cognosphere/Star Rail/Player.log",
        ],
        GameId::ZZZ => &[
            "miHoYo/绝区零/Player.log",
            "miHoYo/ZenlessZoneZero/Player.log",
        ],
        _ => &[],
    }
}

/// 从 Steam 的 `libraryfolders.vdf` 取出各游戏库目录。
fn steam_libraries(vdf: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(vdf) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("\"path\"")?.trim();
            let v = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(PathBuf::from(v.replace("\\\\", "\\")))
        })
        .collect()
}

/// 默认路径表。
#[derive(Debug, Clone, Default)]
pub struct StandardPaths {
    home: Option<PathBuf>,
    drives: Vec<PathBuf>,
    extra: BTreeMap<GameId, Vec<PathBuf>>,
}

impl StandardPaths {
    /// 指定用户目录与盘符根目录。
    pub fn new(home: Option<PathBuf>, drives: Vec<PathBuf>) -> Self {
        Self {
            home,
            drives,
            extra: BTreeMap::new(),
        }
    }

    /// 当前系统：Windows 上为 `%USERPROFILE%` 与存在的 C–Z 盘；其他系统为空。
    pub fn from_env() -> Self {
        if !cfg!(windows) {
            return Self::default();
        }
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
        let drives = (b'C'..=b'Z')
            .map(|d| PathBuf::from(format!("{}:\\", d as char)))
            .filter(|p| p.exists())
            .collect();
        Self::new(home, drives)
    }

    /// 加入用户手动选择的目录或文件，优先于默认位置。
    pub fn with_extra(mut self, game: impl Into<GameId>, path: impl Into<PathBuf>) -> Self {
        self.extra.entry(game.into()).or_default().push(path.into());
        self
    }
}

impl PathProvider for StandardPaths {
    fn game_paths(&self, game: &GameId) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.extra.get(game).cloned().unwrap_or_default();
        if let Some(home) = &self.home {
            let low = home.join("AppData").join("LocalLow");
            out.extend(unity_logs(game.as_str()).iter().map(|p| low.join(p)));
        }
        if game == GameId::WUWA {
            for d in &self.drives {
                out.extend(WUWA_DIRS.iter().map(|p| d.join(p)));
                let vdf = d.join("Program Files (x86)/Steam/steamapps/libraryfolders.vdf");
                out.extend(
                    steam_libraries(&vdf)
                        .into_iter()
                        .map(|lib| lib.join("steamapps/common/Wuthering Waves")),
                );
            }
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|p| p.exists() && seen.insert(p.clone()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn lists_existing_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let drive = dir.path().join("D");
        let log = home.join("AppData/LocalLow/miHoYo/原神/output_log.txt");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(&log, "").unwrap();
        let steam_lib = dir.path().join("E/SteamLibrary");
        let wuwa_steam = steam_lib.join("steamapps/common/Wuthering Waves");
        fs::create_dir_all(&wuwa_steam).unwrap();
        let wuwa = drive.join("Wuthering Waves");
        fs::create_dir_all(&wuwa).unwrap();
        let vdf = drive.join("Program Files (x86)/Steam/steamapps/libraryfolders.vdf");
        fs::create_dir_all(vdf.parent().unwrap()).unwrap();
        fs::write(
            &vdf,
            format!(
                "\"libraryfolders\"\n{{\n  \"1\"\n  {{\n    \"path\"\t\t\"{}\"\n  }}\n}}\n",
                steam_lib.display()
            ),
        )
        .unwrap();
        let manual = dir.path().join("manual");
        fs::create_dir_all(&manual).unwrap();

        let paths = StandardPaths::new(Some(home), vec![drive])
            .with_extra(GameId::WUWA, &manual)
            .with_extra(GameId::WUWA, &manual);
        assert_eq!(paths.game_paths(&GameId::GENSHIN.into()), vec![log]);
        assert_eq!(
            paths.game_paths(&GameId::WUWA.into()),
            vec![manual, wuwa, wuwa_steam]
        );
        assert!(paths.game_paths(&GameId::ARKNIGHTS.into()).is_empty());
        assert!(StandardPaths::default()
            .game_paths(&GameId::ZZZ.into())
            .is_empty());
        let _ = StandardPaths::from_env();
        assert!(steam_libraries(Path::new("missing.vdf")).is_empty());
    }
}
