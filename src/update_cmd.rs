use std::io::Write;

use anyhow::{Context, Result, bail};

use crate::update::{
    self, Asset, Version, checksum_for, download, extract_binary, extract_from_zip, latest_release,
    parse_checksums, replace_binary, sha256_hex,
};

pub fn run(check_only: bool, retries: usize) -> Result<()> {
    let current = Version::parse(update::Version::current())?;
    println!("当前版本: {current}");

    let release = latest_release(retries)?;
    println!("最新版本: {}", release.version);

    if release.version <= current {
        println!("已是最新版本，无需更新。");
        return Ok(());
    }

    let asset_name = update::asset_name()?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| {
            format!(
                "这个版本没有提供 {} 的产物（该平台暂未提供预编译包）",
                asset_name
            )
        })?;

    if check_only {
        println!("有新版本 {}：{}", release.version, asset.name);
        println!("下载地址: {}", asset.url);
        return Ok(());
    }

    println!(
        "\n下载 {}（{:.1} MB）...",
        asset.name,
        asset.size as f64 / 1_048_576.0
    );
    let archive = download(&asset.url, retries)?;
    println!("下载完成，校验完整性...");

    let expected = expected_checksum(&release.assets, &asset.name)?;
    let actual = sha256_hex(&archive);
    if actual != expected {
        bail!(
            "校验和不匹配，已中止安装。\n  期望: {expected}\n  实际: {actual}\n\
             下载的文件可能损坏或被篡改，请重试或手动下载。"
        );
    }
    println!("校验通过: {}", &actual[..16]);

    let binary_name = binary_name_in(&asset_name);
    let executable = if asset_name.ends_with(".zip") {
        extract_from_zip(&archive, binary_name)?
    } else {
        extract_binary(&archive, binary_name)?
    };
    println!("已解出二进制（{} 字节）", executable.len());

    if cfg!(windows) {
        bail!(
            "Windows 上无法覆盖正在运行的程序。请手动下载 {} 并替换。",
            asset.url
        );
    }

    let current_exe = std::env::current_exe().context("找不到当前程序路径")?;
    println!("正在替换 {} ...", current_exe.display());
    replace_binary(&executable, &current_exe)?;

    println!("\n更新完成：{current} → {}", release.version);
    println!("提示：新版本要重新打开才生效。");

    if !release.notes.trim().is_empty() {
        print_notes(&release.notes);
    }
    Ok(())
}

/// 发布流程打包时统一把二进制重命名为 music-shell，
/// 所以只需要知道归档类型就能确定里面那个文件的名字。
pub fn binary_name_in(asset_name: &str) -> &'static str {
    if asset_name.ends_with(".zip") {
        "music-shell.exe"
    } else {
        "music-shell"
    }
}

fn expected_checksum(assets: &[Asset], asset_name: &str) -> Result<String> {
    let file = checksum_for(assets, asset_name)
        .context("这个 Release 没有提供 checksums.txt，为了安全起见不执行自动更新")?;
    let text = String::from_utf8_lossy(&download(&file.url, 3)?).into_owned();
    let map = parse_checksums(&text);
    map.get(asset_name)
        .cloned()
        .with_context(|| format!("checksums.txt 里找不到 {asset_name}"))
}

fn print_notes(notes: &str) {
    println!("\n--- 本次更新说明 ---");
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in notes.lines().take(40) {
        let _ = writeln!(out, "  {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::binary_name_in;

    #[test]
    fn archive_names_map_to_the_binary_inside() {
        assert_eq!(
            binary_name_in("music-shell-x86_64-unknown-linux-gnu.tar.gz"),
            "music-shell"
        );
        assert_eq!(
            binary_name_in("music-shell-aarch64-apple-darwin.tar.gz"),
            "music-shell"
        );
        assert_eq!(
            binary_name_in("music-shell-x86_64-pc-windows-msvc.zip"),
            "music-shell.exe"
        );
    }
}
