use std::io::Write;
use std::path::Path;

use colored::Colorize;

use crate::cli::ImportArgs;
use crate::theme::RawPalette;
use crate::theme::ThemeColor;

const REQUIRED_WT_KEYS: &[&str] = &[
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "purple",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightPurple",
    "brightCyan",
    "brightWhite",
    "foreground",
    "background",
];

enum ThemeFormat {
    WindowsTerminal,
    Alacritty,
    Ghostty,
    ITerm2,
}

impl ThemeFormat {
    fn label(&self) -> &'static str {
        match self {
            Self::WindowsTerminal => "Windows Terminal",
            Self::Alacritty => "Alacritty",
            Self::Ghostty => "Ghostty",
            Self::ITerm2 => "iTerm2",
        }
    }
}

fn detect_format(path: &Path, content: &str) -> Result<ThemeFormat, String> {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_lowercase().as_str() {
            "json" => return Ok(ThemeFormat::WindowsTerminal),
            "toml" => return Ok(ThemeFormat::Alacritty),
            "itermcolors" => return Ok(ThemeFormat::ITerm2),
            "conf" => return Ok(ThemeFormat::Ghostty),
            _ => {}
        }
    }

    let trimmed = content.trim_start();
    if trimmed.starts_with("<?xml")
        || trimmed.starts_with("<plist")
        || trimmed.starts_with("<!DOCTYPE plist")
    {
        return Ok(ThemeFormat::ITerm2);
    }
    if trimmed.starts_with('{') {
        return Ok(ThemeFormat::WindowsTerminal);
    }
    if content.contains("[colors.") {
        return Ok(ThemeFormat::Alacritty);
    }
    // Ghostty: extensionless files with key=value lines
    if content.lines().any(|l| {
        let l = l.trim();
        !l.is_empty() && !l.starts_with('#') && l.contains(" = ")
    }) {
        return Ok(ThemeFormat::Ghostty);
    }

    Err(
        "cannot determine theme format. Supported: .json (Windows Terminal), .toml (Alacritty), \
         .itermcolors (iTerm2), .conf or no extension (Ghostty)"
            .to_string(),
    )
}

pub fn run_import(args: ImportArgs) -> Result<(), i32> {
    let name = args
        .name
        .unwrap_or_else(|| {
            args.file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("theme")
                .to_string()
        })
        .to_lowercase()
        .replace(' ', "-");

    let content = std::fs::read_to_string(&args.file).map_err(|e| {
        eprintln!("error: cannot read file: {}", e);
        1
    })?;

    let format = detect_format(&args.file, &content).map_err(|e| {
        eprintln!("error: {}", e);
        1
    })?;

    let format_label = format.label();
    let palette = match format {
        ThemeFormat::WindowsTerminal => {
            let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                eprintln!("error: invalid JSON: {}", e);
                1
            })?;
            parse_windows_terminal_json(&json).map_err(|e| {
                eprintln!("error: {}", e);
                1
            })?
        }
        ThemeFormat::Alacritty => parse_alacritty_toml(&content).map_err(|e| {
            eprintln!("error: {}", e);
            1
        })?,
        ThemeFormat::Ghostty => parse_ghostty(&content).map_err(|e| {
            eprintln!("error: {}", e);
            1
        })?,
        ThemeFormat::ITerm2 => parse_iterm2_plist(&content).map_err(|e| {
            eprintln!("error: {}", e);
            1
        })?,
    };

    // Ensure global themes dir exists
    let themes_dir = match crate::source::lazytail_dir() {
        Some(dir) => dir.join("themes"),
        None => {
            eprintln!("error: cannot determine config directory");
            return Err(1);
        }
    };
    std::fs::create_dir_all(&themes_dir).map_err(|e| {
        eprintln!("error: cannot create themes directory: {}", e);
        1
    })?;

    let out_path = themes_dir.join(format!("{}.yaml", name));
    if out_path.exists() {
        eprintln!(
            "{}: {} already exists, overwriting",
            "warning".yellow(),
            out_path.display()
        );
    }

    write_theme_yaml(&name, format_label, &palette, &out_path).map_err(|e| {
        eprintln!("error: cannot write theme file: {}", e);
        1
    })?;

    println!(
        "{} Imported theme '{}' to {}",
        "ok:".green(),
        name.cyan(),
        out_path.display().to_string().dimmed()
    );

    Ok(())
}

pub fn run_list() -> Result<(), i32> {
    let discovery = crate::config::discover();
    let themes_dirs = crate::theme::loader::collect_themes_dirs(discovery.project_root.as_deref());

    println!("{}:", "Built-in".cyan());
    println!("  dark");
    println!("  light");

    let external = crate::theme::loader::discover_themes(&themes_dirs);
    if !external.is_empty() {
        println!();
        println!("{}:", "External".cyan());
        for name in &external {
            println!("  {}", name);
        }
    }

    Ok(())
}

pub fn parse_windows_terminal_json(json: &serde_json::Value) -> Result<RawPalette, String> {
    let obj = json
        .as_object()
        .ok_or_else(|| "expected a JSON object".to_string())?;

    // Check for required keys
    let missing: Vec<&str> = REQUIRED_WT_KEYS
        .iter()
        .filter(|&&k| !obj.contains_key(k))
        .copied()
        .collect();

    if !missing.is_empty() {
        return Err(format!("missing required keys: {}", missing.join(", ")));
    }

    let get = |key: &str| -> Result<ThemeColor, String> {
        let val = obj
            .get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("key '{}' must be a string", key))?;
        crate::theme::parse_color(val)
            .map(ThemeColor)
            .map_err(|e| format!("key '{}': {}", key, e))
    };

    // Selection: use selectionBackground if available, fall back to brightBlack
    let selection_key = if obj.contains_key("selectionBackground") {
        "selectionBackground"
    } else {
        "brightBlack"
    };

    Ok(RawPalette {
        black: Some(get("black")?),
        red: Some(get("red")?),
        green: Some(get("green")?),
        yellow: Some(get("yellow")?),
        blue: Some(get("blue")?),
        magenta: Some(get("purple")?),
        cyan: Some(get("cyan")?),
        white: Some(get("white")?),
        bright_black: Some(get("brightBlack")?),
        bright_red: Some(get("brightRed")?),
        bright_green: Some(get("brightGreen")?),
        bright_yellow: Some(get("brightYellow")?),
        bright_blue: Some(get("brightBlue")?),
        bright_magenta: Some(get("brightPurple")?),
        bright_cyan: Some(get("brightCyan")?),
        bright_white: Some(get("brightWhite")?),
        foreground: Some(get("foreground")?),
        background: Some(get("background")?),
        selection: Some(get(selection_key)?),
    })
}

pub fn parse_alacritty_toml(content: &str) -> Result<RawPalette, String> {
    let mut palette = RawPalette::default();
    let mut section = String::new();
    let mut found_any = false;

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                section = line[1..end].trim().to_string();
            }
            continue;
        }

        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim();
            let raw_val = line[eq_pos + 1..].trim();

            // Strip surrounding quotes (TOML strings)
            let val = if let Some(stripped) = raw_val.strip_prefix('"') {
                stripped.split('"').next().unwrap_or(stripped)
            } else if let Some(stripped) = raw_val.strip_prefix('\'') {
                stripped.split('\'').next().unwrap_or(stripped)
            } else {
                raw_val
            };

            let color = match crate::theme::parse_color(val) {
                Ok(c) => ThemeColor(c),
                Err(_) => continue,
            };

            match (section.as_str(), key) {
                ("colors.primary", "foreground") => palette.foreground = Some(color),
                ("colors.primary", "background") => palette.background = Some(color),
                ("colors.selection", "background") => palette.selection = Some(color),
                ("colors.normal", "black") => palette.black = Some(color),
                ("colors.normal", "red") => palette.red = Some(color),
                ("colors.normal", "green") => palette.green = Some(color),
                ("colors.normal", "yellow") => palette.yellow = Some(color),
                ("colors.normal", "blue") => palette.blue = Some(color),
                ("colors.normal", "magenta") => palette.magenta = Some(color),
                ("colors.normal", "cyan") => palette.cyan = Some(color),
                ("colors.normal", "white") => palette.white = Some(color),
                ("colors.bright", "black") => palette.bright_black = Some(color),
                ("colors.bright", "red") => palette.bright_red = Some(color),
                ("colors.bright", "green") => palette.bright_green = Some(color),
                ("colors.bright", "yellow") => palette.bright_yellow = Some(color),
                ("colors.bright", "blue") => palette.bright_blue = Some(color),
                ("colors.bright", "magenta") => palette.bright_magenta = Some(color),
                ("colors.bright", "cyan") => palette.bright_cyan = Some(color),
                ("colors.bright", "white") => palette.bright_white = Some(color),
                _ => continue,
            }
            found_any = true;
        }
    }

    if !found_any {
        return Err("no recognized color keys found in Alacritty config".to_string());
    }

    Ok(palette)
}

pub fn parse_ghostty(content: &str) -> Result<RawPalette, String> {
    let mut palette = RawPalette::default();
    let mut found_any = false;

    // Ghostty colors can be bare hex (no # prefix) or with # prefix
    let parse_hex = |val: &str| -> Result<ThemeColor, String> {
        let val = val.trim();
        let normalized = if val.starts_with('#') {
            val.to_string()
        } else {
            format!("#{}", val)
        };
        crate::theme::parse_color(&normalized).map(ThemeColor)
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim();
            let val = line[eq_pos + 1..].trim();

            match key {
                "background" => palette.background = Some(parse_hex(val)?),
                "foreground" => palette.foreground = Some(parse_hex(val)?),
                "selection-background" => palette.selection = Some(parse_hex(val)?),
                "palette" => {
                    if let Some(eq2) = val.find('=') {
                        if let Ok(idx) = val[..eq2].trim().parse::<usize>() {
                            let hex = val[eq2 + 1..].trim();
                            let color = parse_hex(hex)?;
                            match idx {
                                0 => palette.black = Some(color),
                                1 => palette.red = Some(color),
                                2 => palette.green = Some(color),
                                3 => palette.yellow = Some(color),
                                4 => palette.blue = Some(color),
                                5 => palette.magenta = Some(color),
                                6 => palette.cyan = Some(color),
                                7 => palette.white = Some(color),
                                8 => palette.bright_black = Some(color),
                                9 => palette.bright_red = Some(color),
                                10 => palette.bright_green = Some(color),
                                11 => palette.bright_yellow = Some(color),
                                12 => palette.bright_blue = Some(color),
                                13 => palette.bright_magenta = Some(color),
                                14 => palette.bright_cyan = Some(color),
                                15 => palette.bright_white = Some(color),
                                _ => continue,
                            }
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                _ => continue,
            }
            found_any = true;
        }
    }

    if !found_any {
        return Err("no recognized color keys found in Ghostty config".to_string());
    }

    Ok(palette)
}

pub fn parse_iterm2_plist(content: &str) -> Result<RawPalette, String> {
    let mut palette = RawPalette::default();
    let mut found_any = false;

    let keys: &[(&str, usize)] = &[
        ("Ansi 0 Color", 0),
        ("Ansi 1 Color", 1),
        ("Ansi 2 Color", 2),
        ("Ansi 3 Color", 3),
        ("Ansi 4 Color", 4),
        ("Ansi 5 Color", 5),
        ("Ansi 6 Color", 6),
        ("Ansi 7 Color", 7),
        ("Ansi 8 Color", 8),
        ("Ansi 9 Color", 9),
        ("Ansi 10 Color", 10),
        ("Ansi 11 Color", 11),
        ("Ansi 12 Color", 12),
        ("Ansi 13 Color", 13),
        ("Ansi 14 Color", 14),
        ("Ansi 15 Color", 15),
        ("Background Color", 16),
        ("Foreground Color", 17),
        ("Selection Color", 18),
    ];

    for &(name, idx) in keys {
        let key_tag = format!("<key>{}</key>", name);
        let key_pos = match content.find(&key_tag) {
            Some(pos) => pos,
            None => continue,
        };

        let after_key = &content[key_pos + key_tag.len()..];
        let dict_start = match after_key.find("<dict>") {
            Some(pos) => pos,
            None => continue,
        };

        let dict_content = &after_key[dict_start..];
        let dict_end = match dict_content.find("</dict>") {
            Some(pos) => pos,
            None => continue,
        };

        let dict_block = &dict_content[..dict_end];
        let r = extract_plist_component(dict_block, "Red Component")?;
        let g = extract_plist_component(dict_block, "Green Component")?;
        let b = extract_plist_component(dict_block, "Blue Component")?;

        let color = ThemeColor(ratatui::style::Color::Rgb(
            (r * 255.0).round().clamp(0.0, 255.0) as u8,
            (g * 255.0).round().clamp(0.0, 255.0) as u8,
            (b * 255.0).round().clamp(0.0, 255.0) as u8,
        ));

        match idx {
            0 => palette.black = Some(color),
            1 => palette.red = Some(color),
            2 => palette.green = Some(color),
            3 => palette.yellow = Some(color),
            4 => palette.blue = Some(color),
            5 => palette.magenta = Some(color),
            6 => palette.cyan = Some(color),
            7 => palette.white = Some(color),
            8 => palette.bright_black = Some(color),
            9 => palette.bright_red = Some(color),
            10 => palette.bright_green = Some(color),
            11 => palette.bright_yellow = Some(color),
            12 => palette.bright_blue = Some(color),
            13 => palette.bright_magenta = Some(color),
            14 => palette.bright_cyan = Some(color),
            15 => palette.bright_white = Some(color),
            16 => palette.background = Some(color),
            17 => palette.foreground = Some(color),
            18 => palette.selection = Some(color),
            _ => unreachable!(),
        }
        found_any = true;
    }

    if !found_any {
        return Err("no recognized color entries found in iTerm2 plist".to_string());
    }

    Ok(palette)
}

fn extract_plist_component(dict_block: &str, component: &str) -> Result<f64, String> {
    let key_tag = format!("<key>{}</key>", component);
    let key_pos = dict_block
        .find(&key_tag)
        .ok_or_else(|| format!("missing {} in color entry", component))?;

    let after = &dict_block[key_pos + key_tag.len()..];
    let real_start = after
        .find("<real>")
        .ok_or_else(|| format!("missing <real> value for {}", component))?;
    let val_start = real_start + "<real>".len();
    let real_end = after[val_start..]
        .find("</real>")
        .ok_or_else(|| format!("missing </real> for {}", component))?;

    after[val_start..val_start + real_end]
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("invalid float for {}", component))
}

fn write_theme_yaml(
    name: &str,
    format_label: &str,
    palette: &RawPalette,
    path: &Path,
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    writeln!(file, "# {} — imported from {}", name, format_label)?;
    writeln!(file, "base: dark")?;
    writeln!(file, "palette:")?;

    macro_rules! write_field {
        ($file:expr, $palette:expr, $field:ident, $label:expr) => {
            if let Some(ThemeColor(color)) = $palette.$field {
                writeln!($file, "  {}: \"{}\"", $label, format_color(color))?;
            }
        };
    }

    write_field!(file, palette, black, "black");
    write_field!(file, palette, red, "red");
    write_field!(file, palette, green, "green");
    write_field!(file, palette, yellow, "yellow");
    write_field!(file, palette, blue, "blue");
    write_field!(file, palette, magenta, "magenta");
    write_field!(file, palette, cyan, "cyan");
    write_field!(file, palette, white, "white");
    write_field!(file, palette, bright_black, "bright_black");
    write_field!(file, palette, bright_red, "bright_red");
    write_field!(file, palette, bright_green, "bright_green");
    write_field!(file, palette, bright_yellow, "bright_yellow");
    write_field!(file, palette, bright_blue, "bright_blue");
    write_field!(file, palette, bright_magenta, "bright_magenta");
    write_field!(file, palette, bright_cyan, "bright_cyan");
    write_field!(file, palette, bright_white, "bright_white");
    write_field!(file, palette, foreground, "foreground");
    write_field!(file, palette, background, "background");
    write_field!(file, palette, selection, "selection");

    Ok(())
}

fn format_color(color: ratatui::style::Color) -> String {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        ratatui::style::Color::Reset => "default".to_string(),
        other => format!("{:?}", other).to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn test_parse_windows_terminal_json_dracula() {
        let json: serde_json::Value = serde_json::from_str(
            r##"{
                "black": "#21222C",
                "red": "#FF5555",
                "green": "#50FA7B",
                "yellow": "#F1FA8C",
                "blue": "#BD93F9",
                "purple": "#FF79C6",
                "cyan": "#8BE9FD",
                "white": "#F8F8F2",
                "brightBlack": "#6272A4",
                "brightRed": "#FF6E6E",
                "brightGreen": "#69FF94",
                "brightYellow": "#FFFFA5",
                "brightBlue": "#D6ACFF",
                "brightPurple": "#FF92DF",
                "brightCyan": "#A4FFFF",
                "brightWhite": "#FFFFFF",
                "foreground": "#F8F8F2",
                "background": "#282A36",
                "selectionBackground": "#44475A",
                "cursorColor": "#F8F8F2"
            }"##,
        )
        .unwrap();

        let palette = parse_windows_terminal_json(&json).unwrap();

        assert_eq!(palette.black.unwrap().0, Color::Rgb(0x21, 0x22, 0x2C));
        assert_eq!(palette.red.unwrap().0, Color::Rgb(0xFF, 0x55, 0x55));
        assert_eq!(palette.magenta.unwrap().0, Color::Rgb(0xFF, 0x79, 0xC6));
        assert_eq!(
            palette.bright_magenta.unwrap().0,
            Color::Rgb(0xFF, 0x92, 0xDF)
        );
        assert_eq!(palette.foreground.unwrap().0, Color::Rgb(0xF8, 0xF8, 0xF2));
        assert_eq!(palette.background.unwrap().0, Color::Rgb(0x28, 0x2A, 0x36));
        assert_eq!(palette.selection.unwrap().0, Color::Rgb(0x44, 0x47, 0x5A));
    }

    #[test]
    fn test_parse_windows_terminal_json_missing_keys() {
        let json: serde_json::Value = serde_json::from_str(
            r##"{
                "black": "#000000",
                "foreground": "#FFFFFF"
            }"##,
        )
        .unwrap();

        let result = parse_windows_terminal_json(&json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("missing required keys"));
        assert!(err.contains("red"));
    }

    #[test]
    fn test_parse_windows_terminal_json_purple_maps_to_magenta() {
        let json: serde_json::Value = serde_json::from_str(
            r##"{
                "black": "#000000",
                "red": "#FF0000",
                "green": "#00FF00",
                "yellow": "#FFFF00",
                "blue": "#0000FF",
                "purple": "#AA00AA",
                "cyan": "#00FFFF",
                "white": "#FFFFFF",
                "brightBlack": "#555555",
                "brightRed": "#FF5555",
                "brightGreen": "#55FF55",
                "brightYellow": "#FFFF55",
                "brightBlue": "#5555FF",
                "brightPurple": "#FF55FF",
                "brightCyan": "#55FFFF",
                "brightWhite": "#FFFFFF",
                "foreground": "#FFFFFF",
                "background": "#000000"
            }"##,
        )
        .unwrap();

        let palette = parse_windows_terminal_json(&json).unwrap();
        assert_eq!(palette.magenta.unwrap().0, Color::Rgb(0xAA, 0x00, 0xAA));
        assert_eq!(
            palette.bright_magenta.unwrap().0,
            Color::Rgb(0xFF, 0x55, 0xFF)
        );
        // Selection falls back to brightBlack when selectionBackground is missing
        assert_eq!(palette.selection.unwrap().0, Color::Rgb(0x55, 0x55, 0x55));
    }

    #[test]
    fn test_write_theme_yaml_roundtrip() {
        let palette = RawPalette {
            black: Some(ThemeColor(Color::Rgb(0x00, 0x00, 0x00))),
            red: Some(ThemeColor(Color::Rgb(0xFF, 0x00, 0x00))),
            green: Some(ThemeColor(Color::Rgb(0x00, 0xFF, 0x00))),
            yellow: Some(ThemeColor(Color::Rgb(0xFF, 0xFF, 0x00))),
            blue: Some(ThemeColor(Color::Rgb(0x00, 0x00, 0xFF))),
            magenta: Some(ThemeColor(Color::Rgb(0xFF, 0x00, 0xFF))),
            cyan: Some(ThemeColor(Color::Rgb(0x00, 0xFF, 0xFF))),
            white: Some(ThemeColor(Color::Rgb(0xFF, 0xFF, 0xFF))),
            bright_black: Some(ThemeColor(Color::Rgb(0x55, 0x55, 0x55))),
            bright_red: Some(ThemeColor(Color::Rgb(0xFF, 0x55, 0x55))),
            bright_green: Some(ThemeColor(Color::Rgb(0x55, 0xFF, 0x55))),
            bright_yellow: Some(ThemeColor(Color::Rgb(0xFF, 0xFF, 0x55))),
            bright_blue: Some(ThemeColor(Color::Rgb(0x55, 0x55, 0xFF))),
            bright_magenta: Some(ThemeColor(Color::Rgb(0xFF, 0x55, 0xFF))),
            bright_cyan: Some(ThemeColor(Color::Rgb(0x55, 0xFF, 0xFF))),
            bright_white: Some(ThemeColor(Color::Rgb(0xFF, 0xFF, 0xFF))),
            foreground: Some(ThemeColor(Color::Rgb(0xF0, 0xF0, 0xF0))),
            background: Some(ThemeColor(Color::Rgb(0x10, 0x10, 0x10))),
            selection: Some(ThemeColor(Color::Rgb(0x44, 0x44, 0x44))),
        };

        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("test-theme.yaml");

        write_theme_yaml("test-theme", "test", &palette, &path).unwrap();

        // Read back and verify it parses
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: crate::theme::loader::RawThemeFile = serde_saphyr::from_str(&content).unwrap();

        assert_eq!(parsed.base, Some("dark".to_string()));
        let pp = parsed.palette.unwrap();
        assert_eq!(pp.black.unwrap().0, Color::Rgb(0x00, 0x00, 0x00));
        assert_eq!(pp.red.unwrap().0, Color::Rgb(0xFF, 0x00, 0x00));
        assert_eq!(pp.foreground.unwrap().0, Color::Rgb(0xF0, 0xF0, 0xF0));
        assert_eq!(pp.selection.unwrap().0, Color::Rgb(0x44, 0x44, 0x44));
    }

    #[test]
    #[ignore] // Slow: creates temp files and dirs
    fn test_import_creates_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let json_path = temp.path().join("Dracula.json");

        std::fs::write(
            &json_path,
            r##"{
                "black": "#21222C",
                "red": "#FF5555",
                "green": "#50FA7B",
                "yellow": "#F1FA8C",
                "blue": "#BD93F9",
                "purple": "#FF79C6",
                "cyan": "#8BE9FD",
                "white": "#F8F8F2",
                "brightBlack": "#6272A4",
                "brightRed": "#FF6E6E",
                "brightGreen": "#69FF94",
                "brightYellow": "#FFFFA5",
                "brightBlue": "#D6ACFF",
                "brightPurple": "#FF92DF",
                "brightCyan": "#A4FFFF",
                "brightWhite": "#FFFFFF",
                "foreground": "#F8F8F2",
                "background": "#282A36",
                "selectionBackground": "#44475A"
            }"##,
        )
        .unwrap();

        // Parse and write manually (can't call run_import since it uses lazytail_dir)
        let content = std::fs::read_to_string(&json_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        let palette = parse_windows_terminal_json(&json).unwrap();

        let out_path = temp.path().join("dracula.yaml");
        write_theme_yaml("dracula", "Windows Terminal", &palette, &out_path).unwrap();

        assert!(out_path.exists());

        // Verify the file is valid YAML that we can load
        let content = std::fs::read_to_string(&out_path).unwrap();
        let parsed: crate::theme::loader::RawThemeFile = serde_saphyr::from_str(&content).unwrap();
        assert_eq!(parsed.base, Some("dark".to_string()));
        assert!(parsed.palette.is_some());
    }

    #[test]
    fn test_parse_alacritty_toml_dracula() {
        let toml = r##"
[colors.primary]
foreground = "#f8f8f2"
background = "#282a36"

[colors.selection]
background = "#44475a"

[colors.normal]
black = "#21222c"
red = "#ff5555"
green = "#50fa7b"
yellow = "#f1fa8c"
blue = "#bd93f9"
magenta = "#ff79c6"
cyan = "#8be9fd"
white = "#f8f8f2"

[colors.bright]
black = "#6272a4"
red = "#ff6e6e"
green = "#69ff94"
yellow = "#ffffa5"
blue = "#d6acff"
magenta = "#ff92df"
cyan = "#a4ffff"
white = "#ffffff"
"##;

        let palette = parse_alacritty_toml(toml).unwrap();

        assert_eq!(palette.black.unwrap().0, Color::Rgb(0x21, 0x22, 0x2c));
        assert_eq!(palette.red.unwrap().0, Color::Rgb(0xff, 0x55, 0x55));
        assert_eq!(palette.foreground.unwrap().0, Color::Rgb(0xf8, 0xf8, 0xf2));
        assert_eq!(palette.background.unwrap().0, Color::Rgb(0x28, 0x2a, 0x36));
        assert_eq!(palette.selection.unwrap().0, Color::Rgb(0x44, 0x47, 0x5a));
        assert_eq!(
            palette.bright_magenta.unwrap().0,
            Color::Rgb(0xff, 0x92, 0xdf)
        );
    }

    #[test]
    fn test_parse_alacritty_toml_no_colors() {
        let toml = "[font]\nsize = 12\n";
        let result = parse_alacritty_toml(toml);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no recognized"));
    }

    #[test]
    fn test_parse_ghostty_dracula() {
        let ghostty = "\
background = 282a36
foreground = f8f8f2
selection-background = 44475a
palette = 0=21222c
palette = 1=ff5555
palette = 2=50fa7b
palette = 3=f1fa8c
palette = 4=bd93f9
palette = 5=ff79c6
palette = 6=8be9fd
palette = 7=f8f8f2
palette = 8=6272a4
palette = 9=ff6e6e
palette = 10=69ff94
palette = 11=ffffa5
palette = 12=d6acff
palette = 13=ff92df
palette = 14=a4ffff
palette = 15=ffffff
";

        let palette = parse_ghostty(ghostty).unwrap();

        assert_eq!(palette.black.unwrap().0, Color::Rgb(0x21, 0x22, 0x2c));
        assert_eq!(palette.red.unwrap().0, Color::Rgb(0xff, 0x55, 0x55));
        assert_eq!(palette.foreground.unwrap().0, Color::Rgb(0xf8, 0xf8, 0xf2));
        assert_eq!(palette.background.unwrap().0, Color::Rgb(0x28, 0x2a, 0x36));
        assert_eq!(palette.selection.unwrap().0, Color::Rgb(0x44, 0x47, 0x5a));
        assert_eq!(
            palette.bright_white.unwrap().0,
            Color::Rgb(0xff, 0xff, 0xff)
        );
    }

    #[test]
    fn test_parse_ghostty_with_hash_prefix() {
        let ghostty = "background = #282a36\nforeground = #f8f8f2\n";
        let palette = parse_ghostty(ghostty).unwrap();
        assert_eq!(palette.background.unwrap().0, Color::Rgb(0x28, 0x2a, 0x36));
        assert_eq!(palette.foreground.unwrap().0, Color::Rgb(0xf8, 0xf8, 0xf2));
    }

    #[test]
    fn test_parse_ghostty_no_colors() {
        let ghostty = "# just a comment\nfont-size = 12\n";
        let result = parse_ghostty(ghostty);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no recognized"));
    }

    #[test]
    fn test_parse_iterm2_plist() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Ansi 0 Color</key>
	<dict>
		<key>Blue Component</key>
		<real>0.17254902422428131</real>
		<key>Green Component</key>
		<real>0.13333334028720856</real>
		<key>Red Component</key>
		<real>0.12941177189350128</real>
	</dict>
	<key>Ansi 1 Color</key>
	<dict>
		<key>Blue Component</key>
		<real>0.33333333333333331</real>
		<key>Green Component</key>
		<real>0.33333333333333331</real>
		<key>Red Component</key>
		<real>1</real>
	</dict>
	<key>Background Color</key>
	<dict>
		<key>Blue Component</key>
		<real>0.21176470588235294</real>
		<key>Green Component</key>
		<real>0.16470588235294117</real>
		<key>Red Component</key>
		<real>0.15686274509803921</real>
	</dict>
	<key>Foreground Color</key>
	<dict>
		<key>Blue Component</key>
		<real>0.94901960784313721</real>
		<key>Green Component</key>
		<real>0.97254901960784312</real>
		<key>Red Component</key>
		<real>0.97254901960784312</real>
	</dict>
</dict>
</plist>"#;

        let palette = parse_iterm2_plist(plist).unwrap();

        // Ansi 0: RGB ~(33, 34, 44) = #21222c
        let black = palette.black.unwrap().0;
        assert!(matches!(black, Color::Rgb(33, 34, 44)));

        // Ansi 1: RGB ~(255, 85, 85) = #ff5555
        let red = palette.red.unwrap().0;
        assert!(matches!(red, Color::Rgb(255, 85, 85)));

        // Background: RGB ~(40, 42, 54) = #282a36
        let bg = palette.background.unwrap().0;
        assert!(matches!(bg, Color::Rgb(40, 42, 54)));

        // Foreground: RGB ~(248, 248, 242) = #f8f8f2
        let fg = palette.foreground.unwrap().0;
        assert!(matches!(fg, Color::Rgb(248, 248, 242)));
    }

    #[test]
    fn test_parse_iterm2_plist_no_colors() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>Some Other Key</key>
	<string>value</string>
</dict>
</plist>"#;

        let result = parse_iterm2_plist(plist);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no recognized"));
    }

    #[test]
    fn test_detect_format_by_extension() {
        use std::path::PathBuf;

        assert!(matches!(
            detect_format(&PathBuf::from("theme.json"), "{}"),
            Ok(ThemeFormat::WindowsTerminal)
        ));
        assert!(matches!(
            detect_format(&PathBuf::from("theme.toml"), ""),
            Ok(ThemeFormat::Alacritty)
        ));
        assert!(matches!(
            detect_format(&PathBuf::from("theme.itermcolors"), ""),
            Ok(ThemeFormat::ITerm2)
        ));
        assert!(matches!(
            detect_format(&PathBuf::from("theme.conf"), ""),
            Ok(ThemeFormat::Ghostty)
        ));
    }

    #[test]
    fn test_detect_format_by_content() {
        use std::path::PathBuf;
        let no_ext = PathBuf::from("dracula");

        assert!(matches!(
            detect_format(&no_ext, "<?xml version=\"1.0\"?><plist>"),
            Ok(ThemeFormat::ITerm2)
        ));
        assert!(matches!(
            detect_format(&no_ext, "{ \"black\": \"#000\" }"),
            Ok(ThemeFormat::WindowsTerminal)
        ));
        assert!(matches!(
            detect_format(&no_ext, "[colors.primary]\nforeground = \"#fff\""),
            Ok(ThemeFormat::Alacritty)
        ));
        assert!(matches!(
            detect_format(&no_ext, "background = 282a36\nforeground = f8f8f2\n"),
            Ok(ThemeFormat::Ghostty)
        ));
    }

    #[test]
    fn test_detect_format_unknown() {
        use std::path::PathBuf;
        let result = detect_format(&PathBuf::from("theme.xyz"), "random binary data\x00\x01");
        assert!(result.is_err());
    }
}
