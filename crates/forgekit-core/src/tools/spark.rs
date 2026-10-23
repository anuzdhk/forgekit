//! Spark: the query engine behind the launcher. Turns a query into ranked
//! results (calculator, unit conversion, apps, Teku install, web search).
//! The UI only renders results and calls [`run`].

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Action {
    Copy { text: String },
    Launch { exec: String, terminal: bool },
    Install { file: PathBuf },
    Open { uri: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    pub action: Action,
}

pub fn query(q: &str, apps: &[App]) -> Vec<Item> {
    let q = q.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();

    if let Some(file) = q.strip_prefix("install ") {
        let file = expand_home(file.trim());
        out.push(Item {
            title: format!("Install {}", file.display()),
            subtitle: "with Teku".into(),
            icon: "system-software-install".into(),
            action: Action::Install { file },
        });
        return out;
    }
    if let Some(v) = convert(q) {
        out.push(Item {
            title: v.clone(),
            subtitle: "Unit conversion · Enter to copy".into(),
            icon: "accessories-calculator".into(),
            action: Action::Copy { text: v },
        });
    } else if let Some(v) = calc(q) {
        let text = fmt_num(v);
        out.push(Item {
            title: format!("= {text}"),
            subtitle: "Calculator · Enter to copy".into(),
            icon: "accessories-calculator".into(),
            action: Action::Copy { text },
        });
    }

    let ql = q.to_lowercase();
    let mut scored: Vec<(u32, &App)> = apps.iter().filter_map(|a| a.score(&ql).map(|s| (s, a))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    out.extend(scored.into_iter().take(8).map(|(_, a)| Item {
        title: a.name.clone(),
        subtitle: a.comment.clone(),
        icon: a.icon.clone(),
        action: Action::Launch { exec: a.exec.clone(), terminal: a.terminal },
    }));

    out.push(Item {
        title: format!("Search the web for “{q}”"),
        subtitle: "DuckDuckGo".into(),
        icon: "web-browser".into(),
        action: Action::Open { uri: format!("https://duckduckgo.com/?q={}", url_encode(q)) },
    });
    out
}

pub fn run(action: &Action) -> anyhow::Result<()> {
    let detached = |mut c: Command| -> anyhow::Result<()> {
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        Ok(())
    };
    match action {
        Action::Copy { text } => {
            if !super::copy_to_clipboard(&crate::Capabilities::detect(), text)? {
                anyhow::bail!("No clipboard helper found (install wl-clipboard or xclip)");
            }
            Ok(())
        }
        Action::Launch { exec, terminal } => {
            let mut c = if *terminal {
                let mut c = Command::new("x-terminal-emulator");
                c.arg("-e").arg("sh").arg("-c").arg(exec);
                c
            } else {
                let mut c = Command::new("sh");
                c.arg("-c").arg(exec);
                c
            };
            c.process_group(0);
            detached(c)
        }
        Action::Install { file } => {
            let mut c = Command::new("teku");
            c.arg(file);
            detached(c)
        }
        Action::Open { uri } => {
            let mut c = Command::new("xdg-open");
            c.arg(uri);
            detached(c)
        }
    }
}

use std::os::unix::process::CommandExt;

// ---------------------------------------------------------------- apps

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub keywords: Vec<String>,
    pub icon: String,
    /// Exec line with field codes removed.
    pub exec: String,
    pub terminal: bool,
}

impl App {
    fn score(&self, q: &str) -> Option<u32> {
        let name = self.name.to_lowercase();
        if name == q {
            Some(100)
        } else if name.starts_with(q) {
            Some(80)
        } else if name.split_whitespace().any(|w| w.starts_with(q)) {
            Some(60)
        } else if name.contains(q) {
            Some(40)
        } else if self.keywords.iter().any(|k| k.to_lowercase().starts_with(q)) {
            Some(30)
        } else if self.comment.to_lowercase().contains(q) {
            Some(10)
        } else {
            None
        }
    }
}

/// Load every visible `.desktop` application from the XDG data dirs.
/// Earlier dirs win, matching the spec's override rules.
pub fn load_apps() -> Vec<App> {
    let mut dirs = vec![dirs::data_dir().unwrap_or_default()];
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));

    let desktops: Vec<String> =
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').map(str::to_string).collect();
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let root = dir.join("applications");
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "desktop") {
                    continue;
                }
                let id = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('/', "-");
                if !seen.insert(id.clone()) {
                    continue;
                }
                if let Some(app) = std::fs::read_to_string(&p).ok().and_then(|t| parse_desktop(&id, &t, &desktops)) {
                    apps.push(app);
                }
            }
        }
    }
    apps
}

pub fn parse_desktop(id: &str, text: &str, desktops: &[String]) -> Option<App> {
    let mut in_entry = false;
    let mut get = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            get.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    let val = |k: &str| get.get(k).cloned().unwrap_or_default();
    let list = |k: &str| val(k).split(';').filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>();
    if val("Type") != "Application" || val("NoDisplay") == "true" || val("Hidden") == "true" {
        return None;
    }
    let only = list("OnlyShowIn");
    if !only.is_empty() && !only.iter().any(|d| desktops.contains(d)) {
        return None;
    }
    if list("NotShowIn").iter().any(|d| desktops.contains(d)) {
        return None;
    }
    let exec = strip_field_codes(&val("Exec"));
    if exec.is_empty() {
        return None;
    }
    Some(App {
        id: id.to_string(),
        name: val("Name"),
        comment: val("Comment"),
        keywords: list("Keywords"),
        icon: val("Icon"),
        exec,
        terminal: val("Terminal") == "true",
    })
}

fn strip_field_codes(exec: &str) -> String {
    let mut out = String::new();
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if chars.next() == Some('%') {
                out.push('%');
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- calculator

/// Evaluate `+ - * / % ^`, parentheses, and a few functions/constants.
/// Returns `None` for anything that isn't clearly maths (so app names never
/// turn into calculator results).
pub fn calc(src: &str) -> Option<f64> {
    if !src.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut p = Parser { s: src.as_bytes(), i: 0 };
    let v = p.expr()?;
    p.ws();
    // A lone number isn't a calculation.
    let is_plain_number = src.trim().parse::<f64>().is_ok();
    (p.i == p.s.len() && v.is_finite() && !is_plain_number).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            if self.eat(b'*') || self.eat(b'x') {
                v *= self.power()?;
            } else if self.eat(b'/') {
                v /= self.power()?;
            } else if self.eat(b'%') {
                v %= self.power()?;
            } else {
                return Some(v);
            }
        }
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat(b'^') { Some(base.powf(self.power()?)) } else { Some(base) }
    }
    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.atom()
    }
    fn atom(&mut self) -> Option<f64> {
        self.ws();
        if self.eat(b'(') {
            let v = self.expr()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            self.i += 1;
        }
        if self.i > start {
            return std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok();
        }
        while self.s.get(self.i).is_some_and(u8::is_ascii_alphabetic) {
            self.i += 1;
        }
        let word = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        match word {
            "pi" => Some(std::f64::consts::PI),
            "e" => Some(std::f64::consts::E),
            "sqrt" | "sin" | "cos" | "tan" | "ln" | "log" | "abs" => {
                if !self.eat(b'(') {
                    return None;
                }
                let a = self.expr()?;
                if !self.eat(b')') {
                    return None;
                }
                Some(match word {
                    "sqrt" => a.sqrt(),
                    "sin" => a.sin(),
                    "cos" => a.cos(),
                    "tan" => a.tan(),
                    "ln" => a.ln(),
                    "log" => a.log10(),
                    _ => a.abs(),
                })
            }
            _ => None,
        }
    }
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// ---------------------------------------------------------------- units

/// `10 km to mi`, `72 f in c`, `1.5 gb to mb`.
pub fn convert(q: &str) -> Option<String> {
    let q = q.to_lowercase();
    let (lhs, to) = q.split_once(" to ").or_else(|| q.split_once(" in "))?;
    let lhs = lhs.trim();
    let split = lhs.find(|c: char| c.is_ascii_alphabetic() || c == '°')?;
    let value: f64 = lhs[..split].trim().parse().ok()?;
    let from = lhs[split..].trim().trim_start_matches('°');
    let to = to.trim().trim_start_matches('°');

    if let (Some(a), Some(b)) = (temp(from), temp(to)) {
        let c = match a {
            'c' => value,
            'f' => (value - 32.0) * 5.0 / 9.0,
            _ => value - 273.15,
        };
        let r = match b {
            'c' => c,
            'f' => c * 9.0 / 5.0 + 32.0,
            _ => c + 273.15,
        };
        return Some(format!("{} {}", fmt_num(round(r)), to));
    }
    let (fa, ka) = unit(from)?;
    let (fb, kb) = unit(to)?;
    (ka == kb).then(|| format!("{} {}", fmt_num(round(value * fa / fb)), to))
}

fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

fn temp(u: &str) -> Option<char> {
    match u {
        "c" | "celsius" => Some('c'),
        "f" | "fahrenheit" => Some('f'),
        "k" | "kelvin" => Some('k'),
        _ => None,
    }
}

/// Factor to the base unit, and the quantity kind.
fn unit(u: &str) -> Option<(f64, char)> {
    Some(match u {
        "mm" => (0.001, 'L'),
        "cm" => (0.01, 'L'),
        "m" | "meter" | "meters" => (1.0, 'L'),
        "km" => (1000.0, 'L'),
        "in" | "inch" | "inches" => (0.0254, 'L'),
        "ft" | "foot" | "feet" => (0.3048, 'L'),
        "yd" => (0.9144, 'L'),
        "mi" | "mile" | "miles" => (1609.344, 'L'),
        "mg" => (1e-6, 'M'),
        "g" => (0.001, 'M'),
        "kg" => (1.0, 'M'),
        "oz" => (0.028349523125, 'M'),
        "lb" | "lbs" => (0.45359237, 'M'),
        "ml" => (0.001, 'V'),
        "l" => (1.0, 'V'),
        "gal" => (3.785411784, 'V'),
        "b" | "bytes" => (1.0, 'D'),
        "kb" => (1e3, 'D'),
        "mb" => (1e6, 'D'),
        "gb" => (1e9, 'D'),
        "tb" => (1e12, 'D'),
        "kib" => (1024.0, 'D'),
        "mib" => (1048576.0, 'D'),
        "gib" => (1073741824.0, 'D'),
        _ => return None,
    })
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator() {
        assert_eq!(calc("2+3*4"), Some(14.0));
        assert_eq!(calc("(1+2)^2"), Some(9.0));
        assert_eq!(calc("2^3^2"), Some(512.0));
        assert_eq!(calc("sqrt(16) - -1"), Some(5.0));
        assert_eq!(calc("10 % 3"), Some(1.0));
        assert_eq!(calc("firefox"), None);
        assert_eq!(calc("42"), None);
        assert_eq!(calc("2+"), None);
        assert_eq!(fmt_num(0.1 + 0.2), "0.3");
    }

    #[test]
    fn units() {
        assert_eq!(convert("10 km to mi").as_deref(), Some("6.213712 mi"));
        assert_eq!(convert("212 F in C").as_deref(), Some("100 c"));
        assert_eq!(convert("1 gib to mb").as_deref(), Some("1073.741824 mb"));
        assert_eq!(convert("1 kg to km"), None);
    }

    #[test]
    fn desktop_files() {
        let t = "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus --new-window %U\nIcon=org.gnome.Nautilus\nKeywords=folder;manager;\n[Desktop Action new]\nName=Other\n";
        let a = parse_desktop("org.gnome.Nautilus.desktop", t, &[]).unwrap();
        assert_eq!(a.exec, "nautilus --new-window");
        assert_eq!(a.name, "Files");
        assert_eq!(a.keywords, ["folder", "manager"]);
        assert!(
            parse_desktop(
                "x",
                "[Desktop Entry]\nType=Application\nName=X\nExec=x\nOnlyShowIn=KDE;\n",
                &["GNOME".into()]
            )
            .is_none()
        );
    }

    #[test]
    fn ranking() {
        let app = |n: &str| App {
            id: n.into(),
            name: n.into(),
            comment: String::new(),
            keywords: vec![],
            icon: String::new(),
            exec: n.into(),
            terminal: false,
        };
        let apps = vec![app("Firefox"), app("GNOME Files"), app("Fire Starter")];
        let r = query("fi", &apps);
        let titles: Vec<_> = r.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(&titles[..3], ["Fire Starter", "Firefox", "GNOME Files"]);
        assert!(matches!(query("2*21", &apps)[0].action, Action::Copy { ref text } if text == "42"));
    }
}
