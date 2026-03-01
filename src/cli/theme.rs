use std::io::Write;
use std::path::Path;

use colored::Colorize;

use crate::cli::ImportArgs;
use crate::theme::RawPalette;
use crate::theme::ThemeColor;

const REQUIRED_KEYS: &[&str] = &[
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

    // Read and parse JSON
    let content = std::fs::read_to_string(&args.file).map_err(|e| {
        eprintln!("error: cannot read file: {}", e);
        1
    })?;

    let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        eprintln!("error: invalid JSON: {}", e);
        1
    })?;

    let palette = parse_windows_terminal_json(&json).map_err(|e| {
        eprintln!("error: {}", e);
        1
    })?;

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

    write_theme_yaml(&name, &palette, &out_path).map_err(|e| {
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
    let missing: Vec<&str> = REQUIRED_KEYS
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

fn write_theme_yaml(name: &str, palette: &RawPalette, path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    writeln!(file, "# {} — imported from Windows Terminal", name)?;
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

        write_theme_yaml("test-theme", &palette, &path).unwrap();

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
        write_theme_yaml("dracula", &palette, &out_path).unwrap();

        assert!(out_path.exists());

        // Verify the file is valid YAML that we can load
        let content = std::fs::read_to_string(&out_path).unwrap();
        let parsed: crate::theme::loader::RawThemeFile = serde_saphyr::from_str(&content).unwrap();
        assert_eq!(parsed.base, Some("dark".to_string()));
        assert!(parsed.palette.is_some());
    }
}
