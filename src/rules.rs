//! Event-driven window placement rules, with bounded, allocation-free glob matching.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub workspace: Option<usize>,
    pub floating: Option<bool>,
    pub fullscreen: Option<bool>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub center: Option<bool>,
    pub follow: Option<bool>,
    pub scroll_factor: Option<f64>,
}
impl Default for Rule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            app_id: None,
            title: None,
            workspace: None,
            floating: None,
            fullscreen: None,
            width: None,
            height: None,
            x: None,
            y: None,
            center: None,
            follow: None,
            scroll_factor: None,
        }
    }
}
fn enabled_default() -> bool {
    true
}
pub fn validate(rules: &[Rule], workspaces: usize) -> Result<(), String> {
    if rules.len() > 64 {
        return Err("at most 64 window rules are supported".into());
    }
    for r in rules {
        if r.scroll_factor
            .is_some_and(|f| !f.is_finite() || !(0.1..=10.0).contains(&f))
        {
            return Err("window rule scroll_factor must be between 0.1 and 10".into());
        }
        if r.name.len() > 80 || r.name.chars().any(char::is_control) {
            return Err("window rule name is too long or contains control characters".into());
        }
        if r.app_id.is_none() && r.title.is_none() {
            return Err("window rules need an app_id or title matcher".into());
        }
        for p in [&r.app_id, &r.title].into_iter().flatten() {
            if p.is_empty() || p.len() > 256 || p.chars().any(char::is_control) {
                return Err("rule matchers must be 1..256 bytes without control characters".into());
            }
        }
        if r.workspace
            .is_some_and(|n| n == 0 || n > 10 || r.enabled && n > workspaces)
        {
            return Err("rule workspace is outside the configured workspace range".into());
        }
        if [r.width, r.height]
            .into_iter()
            .flatten()
            .any(|n| !(64..=8192).contains(&n))
            || [r.x, r.y]
                .into_iter()
                .flatten()
                .any(|n| !(0..=16384).contains(&n))
        {
            return Err("rule size/position is out of range".into());
        }
        let geometry = r.width.is_some()
            || r.height.is_some()
            || r.x.is_some()
            || r.y.is_some()
            || r.center == Some(true);
        if geometry && r.floating == Some(false) {
            return Err("window rule geometry requires floating mode".into());
        }
        if r.center == Some(true) && (r.x.is_some() || r.y.is_some()) {
            return Err("use rule center or x/y, not both".into());
        }
    }
    Ok(())
}
impl Rule {
    pub fn matches(&self, app_id: Option<&str>, title: Option<&str>) -> bool {
        self.enabled
            && self
                .app_id
                .as_deref()
                .is_none_or(|p| app_id.is_some_and(|s| glob(p, s)))
            && self
                .title
                .as_deref()
                .is_none_or(|p| title.is_some_and(|s| glob(p, s)))
    }
}
#[derive(Default, Debug)]
pub struct Placement {
    pub workspace: Option<usize>,
    pub floating: Option<bool>,
    pub fullscreen: Option<bool>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub center: Option<bool>,
    pub follow: Option<bool>,
}
impl Placement {
    pub fn from_rules(rules: &[Rule], bits: u64) -> Self {
        let mut p = Self::default();
        for (i, r) in rules.iter().enumerate() {
            if bits & (1u64 << i) == 0 {
                continue;
            }
            macro_rules! copy {($($field:ident),*)=>{$(if r.$field.is_some(){p.$field=r.$field;})*};}
            if r.width.is_some()
                || r.height.is_some()
                || r.x.is_some()
                || r.y.is_some()
                || r.center == Some(true)
            {
                p.floating = Some(true);
            }
            copy!(workspace, floating, fullscreen, width, height, x, y, center, follow);
            if r.center == Some(true) {
                p.x = None;
                p.y = None;
            }
            if r.x.is_some() || r.y.is_some() {
                p.center = Some(false);
            }
        }
        p
    }
}
pub fn matching(rules: &[Rule], app_id: Option<&str>, title: Option<&str>) -> u64 {
    rules.iter().enumerate().fold(0, |bits, (i, r)| {
        if r.matches(app_id, title) {
            bits | (1u64 << i)
        } else {
            bits
        }
    })
}
/// `*` matches any run and `?` one Unicode scalar. Other characters are literal.
fn glob(pattern: &str, text: &str) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star = None;
    while t < text.len() {
        let pc = pattern[p..].chars().next();
        let tc = text[t..].chars().next().unwrap();
        match pc {
            Some('*') => {
                p += 1;
                star = Some((p, t));
            }
            Some(c) if c == '?' || c == tc => {
                p += c.len_utf8();
                t += tc.len_utf8();
            }
            _ => {
                if let Some((resume, retry)) = star {
                    if retry >= text.len() {
                        return false;
                    }
                    let next = retry + text[retry..].chars().next().unwrap().len_utf8();
                    star = Some((resume, next));
                    p = resume;
                    t = next;
                } else {
                    return false;
                }
            }
        }
    }
    while pattern[p..].starts_with('*') {
        p += 1;
    }
    p == pattern.len()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn globs_match_unicode_and_literal_dots_without_regex_allocations() {
        for (p, s, yes) in [
            ("org.*.Sober", "org.vinegarhq.Sober", true),
            ("*Roblox*", "Playing Roblox", true),
            ("?ą*", "xąę", true),
            ("?", "界", true),
            ("org.test", "orgXtest", false),
            ("a*b*c", "axybzc", true),
            ("*a*b", "a", false),
            ("*", "", true),
            ("a?", "a", false),
        ] {
            assert_eq!(glob(p, s), yes, "{p} / {s}");
        }
    }
    #[test]
    fn validates_bounds_and_combines_rules_in_order() {
        let mut r: Rule =
            toml::from_str("app_id = 'org.*'\nworkspace = 10\nwidth = 640\ncenter = true").unwrap();
        assert!(r.enabled);
        assert!(validate(std::slice::from_ref(&r), 10).is_ok());
        assert!(!r.matches(None, Some("title")));
        let last: Rule = toml::from_str("title = '*Game*'\nworkspace = 2\nx = 12").unwrap();
        let rules = vec![r.clone(), last];
        let bits = matching(&rules, Some("org.test"), Some("Game"));
        assert_eq!(bits, 3);
        let p = Placement::from_rules(&rules, bits);
        assert_eq!(p.workspace, Some(2));
        assert_eq!(p.floating, Some(true));
        assert_eq!(p.center, Some(false));
        r.workspace = Some(11);
        assert!(validate(std::slice::from_ref(&r), 10).is_err());
        r.workspace = None;
        r.floating = Some(false);
        assert!(validate(&[r], 10).is_err());
    }
}
