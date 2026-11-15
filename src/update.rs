//! Проверка обновлений и автоматическая загрузка с GitHub Releases.
//!
//! Запрос уходит в `https://api.github.com/repos/qqew666/game-overlay/releases/latest`.
//! Ничего не выполняется само по себе: модуль только узнаёт версию и качает файл,
//! решение об установке принимает панель.

use std::path::PathBuf;
use std::time::Duration;

const API: &str = "https://api.github.com/repos/qqew666/game-overlay/releases/latest";
const UA: &str = "GameOverlay-updater";

/// Что удалось вытащить из последнего релиза.
pub struct Release {
    /// Тег без ведущей `v`, например `1.3.0`.
    pub version: String,
    /// Ссылка на установщик MSI, если он приложен к релизу.
    pub msi_url: Option<String>,
    /// Ссылка на portable ZIP, если он приложен к релизу.
    pub zip_url: Option<String>,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())
}

/// Все значения строкового поля `"key": "value"` в теле ответа.
fn string_values(body: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(pos) = rest.find(key) {
        rest = &rest[pos + key.len()..];
        let Some(colon) = rest.find(':') else { break };
        let after = rest[colon + 1..].trim_start();
        if !after.starts_with('"') {
            continue;
        }
        let inner = &after[1..];
        let Some(end) = inner.find('"') else { break };
        out.push(inner[..end].to_string());
    }
    out
}

/// Разбирает `1.2.3`, `v1.2.3`, `1.2.3-beta` в сравнимый кортеж.
pub fn version_tuple(v: &str) -> (u32, u32, u32) {
    let trimmed = v.trim().trim_start_matches(['v', 'V']);
    let mut parts = trimmed.split('.');
    let mut next = || -> u32 {
        parts
            .next()
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    };
    (next(), next(), next())
}

/// `true`, если версия из релиза новее локальной.
pub fn is_newer(remote: &str, local: &str) -> bool {
    version_tuple(remote) > version_tuple(local)
}

/// Запрашивает последний релиз. Возвращает `Err` с текстом причины.
pub fn check() -> Result<Release, String> {
    let body = client()?
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("запрос: {e}"))?
        .text()
        .map_err(|e| format!("чтение: {e}"))?;

    let version = string_values(&body, "\"tag_name\"")
        .into_iter()
        .next()
        .ok_or_else(|| "в ответе нет tag_name".to_string())?;

    let mut msi_url = None;
    let mut zip_url = None;
    for url in string_values(&body, "\"browser_download_url\"") {
        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".msi") && msi_url.is_none() {
            msi_url = Some(url);
        } else if lower.ends_with(".zip") && zip_url.is_none() {
            zip_url = Some(url);
        }
    }

    Ok(Release {
        version,
        msi_url,
        zip_url,
    })
}

/// Качает файл обновления во временный каталог и возвращает его путь.
pub fn download(url: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("GameOverlay-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("каталог: {e}"))?;
    let name = url.rsplit('/').next().unwrap_or("update.bin");
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let path = dir.join(if safe.is_empty() { "update.bin".to_string() } else { safe });

    let mut response = client()?
        .get(url)
        .send()
        .map_err(|e| format!("загрузка: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("загрузка: HTTP {}", response.status()));
    }

    let mut file = std::fs::File::create(&path).map_err(|e| format!("файл: {e}"))?;
    std::io::copy(&mut response, &mut file).map_err(|e| format!("запись: {e}"))?;
    Ok(path)
}

/// Запускает скачанный установщик. MSI ставится через msiexec в тихом режиме.
pub fn launch_installer(path: &str) -> bool {
    let is_msi = std::path::Path::new(path)
        .extension()
        .map(|e| e.eq_ignore_ascii_case("msi"))
        .unwrap_or(false);
    if is_msi {
        std::process::Command::new("msiexec")
            .arg("/i")
            .arg(path)
            .arg("/passive")
            .spawn()
            .is_ok()
    } else {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .is_ok()
    }
}
