use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub const REPO: &str = "yao1987825/music-shell";
pub const API_BASE: &str = "https://api.github.com";
pub const UPDATE_URL_ENV: &str = "MUSIC_SHELL_UPDATE_URL";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(text: &str) -> Result<Self> {
        let raw = text.trim().trim_start_matches('v');
        let core = raw.split(['-', '+']).next().unwrap_or(raw);
        let mut parts = core.split('.');
        let major = parts.next().context("版本号缺少主版本号")?;
        let minor = parts.next().context("版本号缺少次版本号")?;
        let patch = parts.next().context("版本号缺少修订号")?;
        Ok(Self {
            major: major.parse().context("主版本号不是数字")?,
            minor: minor.parse().context("次版本号不是数字")?,
            patch: patch.parse().context("修订号不是数字")?,
        })
    }

    pub fn current() -> &'static str {
        env!("MS_VERSION")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// 依据编译期平台推算 Release 里的资产文件名
pub fn asset_name() -> Result<String> {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    let target = match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => bail!("暂不支持的平台: {os}-{arch}"),
    };
    Ok(format!("music-shell-{target}.{ext}"))
}

pub struct Release {
    pub version: Version,
    pub tag: String,
    pub notes: String,
    pub assets: Vec<Asset>,
}

pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

pub struct UpdateInfo {
    pub release: Release,
    pub current: Version,
}

impl UpdateInfo {
    pub fn is_newer(&self) -> bool {
        self.release.version > self.current
    }
}

fn api_base() -> String {
    std::env::var(UPDATE_URL_ENV).unwrap_or_else(|_| API_BASE.to_string())
}

fn agent() -> Result<ureq::Agent> {
    let mut config = ureq::Agent::config_builder();
    config = config
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .user_agent(format!("music-shell/{}", Version::current()));

    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        if let Ok(url) = std::env::var(key)
            && !url.trim().is_empty()
        {
            match ureq::Proxy::new(&url) {
                Ok(proxy) => {
                    config = config.proxy(Some(proxy));
                }
                Err(err) => bail!("{key} 里的代理地址无法识别: {url}（{err}）"),
            }
            break;
        }
    }
    Ok(config.build().new_agent())
}

/// 查最新的 release。GitHub 在部分网络下不稳定，固定重试几次。
pub fn latest_release(retries: usize) -> Result<Release> {
    let url = format!("{}/repos/{REPO}/releases/latest", api_base());
    let body = get_with_retry(&url, retries)?;
    parse_release(&body)
}

fn get_with_retry(url: &str, retries: usize) -> Result<String> {
    let agent = agent()?;
    let mut last = String::new();
    for attempt in 0..retries.max(1) {
        match agent.get(url).call() {
            Ok(resp) => {
                let text = resp.into_body().read_to_string().context("读取响应失败")?;
                return Ok(text);
            }
            Err(ureq::Error::StatusCode(code)) => {
                bail!("GitHub 返回 {code}：{url}")
            }
            Err(err) => {
                last = err.to_string();
                if attempt + 1 < retries.max(1) {
                    std::thread::sleep(std::time::Duration::from_millis(
                        500 * (attempt as u64 + 1),
                    ));
                }
            }
        }
    }
    bail!("连不上 GitHub（已重试 {} 次）：{last}", retries.max(1))
}

fn parse_release(body: &str) -> Result<Release> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("解析 GitHub 返回的 JSON 失败")?;
    let tag = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .context("返回里没有 tag_name")?
        .to_string();
    let notes = value
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let mut assets = Vec::new();
    if let Some(list) = value.get("assets").and_then(|v| v.as_array()) {
        for item in list {
            let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(url) = item.get("browser_download_url").and_then(|v| v.as_str()) else {
                continue;
            };
            assets.push(Asset {
                name: name.to_string(),
                url: url.to_string(),
                size: item.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            });
        }
    }
    Ok(Release {
        version: Version::parse(&tag)?,
        tag,
        notes,
        assets,
    })
}

pub fn download(url: &str, retries: usize) -> Result<Vec<u8>> {
    let agent = agent()?;
    let mut last = String::new();
    for attempt in 0..retries.max(1) {
        match agent.get(url).call() {
            Ok(resp) => {
                let mut buf = Vec::new();
                std::io::copy(&mut resp.into_body().into_reader(), &mut buf)
                    .context("下载过程中断")?;
                return Ok(buf);
            }
            Err(err) => {
                last = err.to_string();
                if attempt + 1 < retries.max(1) {
                    std::thread::sleep(std::time::Duration::from_millis(
                        500 * (attempt as u64 + 1),
                    ));
                }
            }
        }
    }
    bail!("下载失败（已重试 {} 次）：{last}", retries.max(1))
}

/// 从 release 资产里找出校验和文件
pub fn checksum_for<'a>(assets: &'a [Asset], file_name: &str) -> Option<&'a Asset> {
    assets
        .iter()
        .find(|a| a.name == "checksums.txt" || a.name == format!("{file_name}.sha256"))
}

pub fn parse_checksums(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let sum = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            Some((name.to_string(), sum.to_ascii_lowercase()))
        })
        .collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(data);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 把新二进制写到旧文件旁边再原子替换，中途失败不会破坏现有程序
pub fn replace_binary(binary: &[u8], target: &Path) -> Result<()> {
    let target = target.canonicalize().unwrap_or_else(|_| {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("music-shell"))
    });
    let dir = target.parent().context("找不到程序所在目录")?;

    if let Ok(meta) = std::fs::metadata(&target)
        && meta.permissions().readonly()
    {
        bail!(
            "{} 没有写权限，请用管理员权限或改用手动安装",
            target.display()
        );
    }

    let staging = dir.join(format!(
        ".{}.new",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "music-shell".into())
    ));
    std::fs::write(&staging, binary).context("写临时文件失败")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))
            .context("设置可执行权限失败")?;
    }

    if let Err(err) = std::fs::rename(&staging, &target) {
        let _ = std::fs::remove_file(&staging);
        return Err(err).context("替换程序失败");
    }
    Ok(())
}

/// 从 tar.gz 里取出那个二进制
pub fn extract_binary(archive: &[u8], file_name: &str) -> Result<Vec<u8>> {
    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(archive));
    let mut tar = tar::Archive::new(decoder);
    for entry in tar
        .entries()
        .context("解压失败，文件可能不是有效的 tar.gz")?
    {
        let mut entry = entry.context("读取压缩包条目失败")?;
        let path = entry.path().context("压缩包里有非法路径")?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name == file_name {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf).context("读取压缩包内容失败")?;
            return Ok(buf);
        }
    }
    bail!("压缩包里没有找到 {file_name}")
}

pub fn extract_from_zip(archive: &[u8], file_name: &str) -> Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(archive);
    let mut zip = zip::ZipArchive::new(cursor).context("解压失败，文件可能不是有效的 zip")?;
    let mut file = zip
        .by_name(file_name)
        .with_context(|| format!("压缩包里没有找到 {file_name}"))?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut buf).context("读取压缩包内容失败")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_with_and_without_the_v_prefix() {
        assert_eq!(Version::parse("1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(Version::parse("v0.4.4").unwrap().to_string(), "0.4.4");
        assert_eq!(
            Version::parse(" v10.20.30 ").unwrap().to_string(),
            "10.20.30"
        );
        assert_eq!(Version::parse("1.2.3-beta.1").unwrap().to_string(), "1.2.3");
    }

    #[test]
    fn bad_versions_are_rejected() {
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("abc").is_err());
        assert!(Version::parse("").is_err());
    }

    #[test]
    fn version_ordering_is_numeric_not_lexical() {
        let a = Version::parse("0.4.9").unwrap();
        let b = Version::parse("0.4.10").unwrap();
        let c = Version::parse("1.0.0").unwrap();
        assert!(b > a, "0.4.10 应该大于 0.4.9");
        assert!(c > b);
        assert_eq!(a, Version::parse("v0.4.9").unwrap());
    }

    #[test]
    fn the_current_version_is_a_valid_version_string() {
        let cur = Version::parse(Version::current()).expect("编译进来的版本号应当合法");
        assert!(
            Version::current().split('.').count() == 3,
            "版本号应当是 x.y.z 三段：{}",
            Version::current()
        );
        let _ = cur;
    }

    #[test]
    fn asset_name_matches_the_release_naming() {
        let name = asset_name().unwrap();
        assert!(name.starts_with("music-shell-"), "{name}");
        assert!(
            name.ends_with(".tar.gz") || name.ends_with(".zip"),
            "{name}"
        );
    }

    #[test]
    fn release_json_is_parsed_into_version_and_assets() {
        let body = r#"{
            "tag_name": "v1.2.3",
            "body": "some notes",
            "assets": [
                {"name": "music-shell-x86_64-unknown-linux-gnu.tar.gz",
                 "browser_download_url": "https://example.com/a.tar.gz", "size": 123},
                {"name": "checksums.txt",
                 "browser_download_url": "https://example.com/checksums.txt", "size": 10}
            ]
        }"#;
        let r = parse_release(body).unwrap();
        assert_eq!(r.version.to_string(), "1.2.3");
        assert_eq!(r.tag, "v1.2.3");
        assert_eq!(r.notes, "some notes");
        assert_eq!(r.assets.len(), 2);
        assert_eq!(r.assets[0].size, 123);

        let sums = checksum_for(&r.assets, "music-shell-x86_64-unknown-linux-gnu.tar.gz").unwrap();
        assert_eq!(sums.name, "checksums.txt");
    }

    #[test]
    fn malformed_release_json_gives_a_clear_error() {
        assert!(parse_release("not json").is_err());
        assert!(parse_release("{}").is_err());
        assert!(parse_release(r#"{"tag_name":"v1.2.3"}"#).is_ok());
    }

    #[test]
    fn checksums_file_is_parsed() {
        let text = "abc123  music-shell-x86_64-unknown-linux-gnu.tar.gz\ndef456  checksums.txt\n";
        let map = parse_checksums(text);
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("music-shell-x86_64-unknown-linux-gnu.tar.gz")
                .unwrap(),
            "abc123"
        );
    }

    #[test]
    fn sha256_matches_the_known_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
