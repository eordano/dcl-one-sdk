//! A build's own output, colours kept: tsc and esbuild write SGR escapes
//! when they think a terminal is listening, and the failure panel used to
//! show them as `ESC[96m` litter. The eight colours, bright or not, bold and
//! dim become spans the sheet styles; every other escape sequence is
//! dropped. Text is HTML-escaped here, so callers pass the raw string.

use std::fmt::Write;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    dim: bool,
    /// The SGR code of the foreground colour (30–37, 90–97), if any.
    color: Option<u8>,
}

impl Style {
    fn apply(&mut self, code: u8) {
        match code {
            0 => *self = Style::default(),
            1 => self.bold = true,
            2 => self.dim = true,
            22 => (self.bold, self.dim) = (false, false),
            39 => self.color = None,
            30..=37 | 90..=97 => self.color = Some(code),
            _ => {}
        }
    }

    fn open_tag(self) -> Option<String> {
        let mut classes = Vec::new();
        if self.bold {
            classes.push("ansi-b".to_string());
        }
        if self.dim {
            classes.push("ansi-d".to_string());
        }
        if let Some(c) = self.color {
            classes.push(format!("ansi-{c}"));
        }
        (!classes.is_empty()).then(|| format!(r#"<span class="{}">"#, classes.join(" ")))
    }
}

pub fn to_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut style = Style::default();
    let mut open = false;
    let mut rest = s;
    while let Some(i) = rest.find('\u{1b}') {
        push_escaped(&mut out, &rest[..i]);
        rest = &rest[i + 1..];
        let Some(after) = rest.strip_prefix('[') else {
            continue;
        };
        let end = after
            .char_indices()
            .find(|(_, c)| ('\u{40}'..='\u{7e}').contains(c));
        let Some((end, fin)) = end else {
            rest = "";
            break;
        };
        let params = &after[..end];
        rest = &after[end + fin.len_utf8()..];
        if fin != 'm' {
            continue;
        }
        let mut next = style;
        if params.is_empty() {
            next.apply(0);
        }
        let mut codes = params.split(';').map(|c| c.parse::<u8>().unwrap_or(0));
        while let Some(code) = codes.next() {
            if code == 38 || code == 48 {
                match codes.next() {
                    Some(5) => {
                        codes.next();
                    }
                    Some(2) => {
                        codes.nth(2);
                    }
                    _ => {}
                }
                continue;
            }
            next.apply(code);
        }
        if next != style {
            if open {
                out.push_str("</span>");
                open = false;
            }
            if let Some(tag) = next.open_tag() {
                out.push_str(&tag);
                open = true;
            }
            style = next;
        }
    }
    push_escaped(&mut out, rest);
    if open {
        out.push_str("</span>");
    }
    out
}

pub fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('\u{1b}') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let Some(after) = rest.strip_prefix('[') else {
            continue;
        };
        match after.find(|c: char| ('\u{40}'..='\u{7e}').contains(&c)) {
            Some(end) => rest = &after[end + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => {
                let _ = write!(out, "{c}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_colour_becomes_spans_and_text_is_escaped() {
        let s = "\u{1b}[96msrc/a.ts\u{1b}[0m:\u{1b}[93m73\u{1b}[0m - \u{1b}[91merror\u{1b}[0m \u{1b}[90mTS2339: \u{1b}[0mtype 'A<B>' does not exist";
        assert_eq!(
            to_html(s),
            r#"<span class="ansi-96">src/a.ts</span>:<span class="ansi-93">73</span> - <span class="ansi-91">error</span> <span class="ansi-90">TS2339: </span>type 'A&lt;B&gt;' does not exist"#
        );
        assert_eq!(
            strip(s),
            "src/a.ts:73 - error TS2339: type 'A<B>' does not exist"
        );
    }

    #[test]
    fn weight_stacks_and_other_sequences_vanish() {
        assert_eq!(
            to_html("\u{1b}[1;31mno\u{1b}[22m still red\u{1b}[39m plain\u{1b}[2K\u{1b}[38;5;200mx\u{1b}[0m"),
            r#"<span class="ansi-b ansi-31">no</span><span class="ansi-31"> still red</span> plainx"#
        );
        assert_eq!(to_html("a\u{1b}[96"), "a");
        assert_eq!(to_html("a\u{1b}b"), "ab");
        assert_eq!(to_html("no escapes & <b>"), "no escapes &amp; &lt;b&gt;");
    }
}
