//! Scrollback-friendly terminal presentation. Machine-readable output bypasses this module.
use std::{
    future::Future,
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

pub fn clean(text: &str) -> String {
    // Strip complete ANSI CSI / OSC sequences as well as standalone control characters.
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if ch == '\n'
            || (!ch.is_control() && !matches!(ch,'\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        {
            out.push(ch);
        } else if ch == '\t' {
            out.push(' ');
        }
    }
    out
}
pub fn interactive() -> bool {
    io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && std::env::var("TERM").is_ok_and(|t| t != "dumb")
}
#[derive(Clone, Copy)]
pub struct Ui {
    pub color: bool,
    pub width: usize,
}
impl Ui {
    pub fn detect() -> Self {
        let width = terminal_size::terminal_size()
            .map(|(w, _)| usize::from(w.0))
            .or_else(|| std::env::var("COLUMNS").ok()?.parse().ok())
            .unwrap_or(88)
            .clamp(24, 110);
        Self {
            color: io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
                && std::env::var("TERM").is_ok_and(|t| t != "dumb"),
            width,
        }
    }
    pub fn style(&self, text: &str, code: &str) -> String {
        let text = clean(text);
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text
        }
    }
    pub fn rule(&self) {
        println!(
            "{}",
            self.style(&"─".repeat(self.width.saturating_sub(2)), "2")
        );
    }
    pub fn heading(&self, text: &str) {
        println!();
        for line in wrap(&clean(text), self.width.saturating_sub(2)) {
            println!("{}", self.style(&line, "1;36"));
        }
    }
    pub fn text(&self, text: &str) {
        for line in wrap(&clean(text), self.width.saturating_sub(4)) {
            println!("  {line}");
        }
    }
    pub fn note(&self, text: &str) {
        for line in wrap(&clean(text), self.width.saturating_sub(4)) {
            println!("  {}", self.style(&line, "2"));
        }
    }
    pub fn error(&self, text: &str) {
        println!(
            "\n{}",
            self.style("  Could not complete that request", "1;33")
        );
        self.text(text);
    }
}
/// Wrap unstyled text by display columns, including long URLs and wide Unicode characters.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut output = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut columns = 0;
        for word in paragraph.split_whitespace() {
            let count: usize = word.chars().map(|c| c.width().unwrap_or(0)).sum();
            if columns > 0 && columns + 1 + count > width {
                output.push(std::mem::take(&mut line));
                columns = 0;
            }
            if columns > 0 {
                line.push(' ');
                columns += 1;
            }
            for ch in word.chars() {
                let size = ch.width().unwrap_or(0);
                if columns + size > width && !line.is_empty() {
                    output.push(std::mem::take(&mut line));
                    columns = 0;
                }
                line.push(ch);
                columns += size;
            }
        }
        output.push(line);
    }
    output
}
pub async fn progress<F: Future>(enabled: bool, future: F) -> F::Output {
    if !enabled || !io::stderr().is_terminal() || std::env::var_os("NO_COLOR").is_some() {
        return future.await;
    }
    tokio::pin!(future);
    let started = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut frame = 0;
    let result = loop {
        tokio::select! {
            result=&mut future=>break result,
            _=tick.tick()=> {let mut stderr=io::stderr().lock();let _=write!(stderr,"\r\x1b[2K  \x1b[36m{}\x1b[0m Investigating · {}s · Ctrl+C to cancel",frames[frame%frames.len()],started.elapsed().as_secs());let _=stderr.flush();frame+=1;}
        }
    };
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K");
    let _ = stderr.flush();
    result
}
