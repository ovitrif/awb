use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::style::{Attribute, Color, Print, SetAttribute, Stylize};
use crossterm::terminal;
use crossterm::terminal::{Clear, ClearType};

pub const CANCEL_HINT: &str = "Press either ⌃ + C, ESC, C or X to cancel and exit.";

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn status(message: impl AsRef<str>) {
    println!("{} {}", "◇".with(Color::Green), message.as_ref().bold());
}

pub fn success(message: impl AsRef<str>) {
    println!("{} {}", "✓".with(Color::Green), message.as_ref().bold());
}

pub fn warn(message: impl AsRef<str>) {
    println!(
        "{} {}",
        "!".with(Color::Yellow),
        format!("Warning: {}", message.as_ref()).with(Color::Yellow)
    );
}

pub fn error(message: impl AsRef<str>) {
    eprintln!(
        "\n{} {}",
        "×".with(Color::Red),
        format!("Error: {}", message.as_ref()).with(Color::Red)
    );
}

pub fn blank_line() {
    println!();
}

pub fn print_qr(qr: &str) {
    println!("{qr}");
}

pub fn title(name: &str, subtitle: &str) {
    println!(
        "{} {}",
        name.with(Color::Green).bold(),
        subtitle.with(Color::DarkGrey)
    );
}

pub fn section<I, S>(title: &str, lines: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    blank_line();
    println!("{} {}", "◇".with(Color::Green), title.bold());

    for line in lines {
        println!("{}  {}", "│".with(Color::DarkGrey), line.as_ref());
    }

    println!("{}", "└".with(Color::DarkGrey));
}

pub fn cancelled() -> anyhow::Error {
    Cancelled.into()
}

pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Cancelled>().is_some()
}

pub fn sleep_or_cancel(duration: Duration) -> Result<()> {
    if duration.is_zero() {
        return Ok(());
    }

    if terminal::enable_raw_mode().is_err() {
        thread::sleep(duration);
        return Ok(());
    }

    let raw_mode = RawModeGuard;
    let deadline = Instant::now() + duration;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());

        if remaining.is_zero() {
            return Ok(());
        }

        if !event::poll(remaining.min(Duration::from_millis(100)))
            .context("failed to poll keypress")?
        {
            continue;
        }

        match event::read().context("failed to read keypress")? {
            Event::Key(key)
                if key.kind == KeyEventKind::Press
                    && is_wait_cancel_key(key.code, key.modifiers) =>
            {
                drop(raw_mode);
                return Err(cancelled());
            }
            _ => {}
        }
    }
}

pub struct Countdown {
    label: String,
    last_seconds: Option<u64>,
    wrote_line: bool,
}

impl Countdown {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            last_seconds: None,
            wrote_line: false,
        }
    }

    pub fn tick(&mut self, remaining: Duration) -> Result<()> {
        let seconds = display_seconds(remaining);

        if self.last_seconds == Some(seconds) {
            return Ok(());
        }

        execute!(
            io::stdout(),
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            Print(format!(
                "{} {}: {seconds} seconds remaining...",
                "◇".with(Color::Green),
                self.label
            ))
        )?;
        io::stdout().flush().context("failed to flush stdout")?;

        self.last_seconds = Some(seconds);
        self.wrote_line = true;

        Ok(())
    }

    pub fn finish(&mut self) {
        if self.wrote_line {
            println!();
            self.wrote_line = false;
            self.last_seconds = None;
        }
    }
}

fn display_seconds(remaining: Duration) -> u64 {
    if remaining.is_zero() {
        0
    } else {
        remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
    }
}

pub fn menu(options: &[&str]) -> Result<usize> {
    if options.is_empty() {
        bail!("menu cannot be shown without options");
    }

    blank_line();
    status("Choose an action");

    if options.len() <= 9 {
        match interactive_menu(options) {
            Ok(value) => return Ok(value),
            Err(error) if is_raw_mode_error(&error) => {
                warn("interactive input is unavailable; press a number and Enter.");
            }
            Err(error) => return Err(error),
        }
    }

    line_menu(options)
}

pub fn menu_with_default(
    options: &[&str],
    default_selection: usize,
    timeout: Duration,
) -> Result<usize> {
    if options.is_empty() {
        bail!("menu cannot be shown without options");
    }

    if !(1..=options.len()).contains(&default_selection) {
        bail!("default menu selection must be one of the numbered options");
    }

    blank_line();
    status("Choose an action");

    if options.len() <= 9 {
        match interactive_menu_with_default(options, default_selection, timeout) {
            Ok(value) => return Ok(value),
            Err(error) if is_raw_mode_error(&error) => {
                warn("interactive input is unavailable; press a number and Enter.");
            }
            Err(error) => return Err(error),
        }
    }

    line_menu(options)
}

fn interactive_menu(options: &[&str]) -> Result<usize> {
    interactive_menu_inner(options, None)
}

fn interactive_menu_with_default(
    options: &[&str],
    default_selection: usize,
    timeout: Duration,
) -> Result<usize> {
    let auto_default = if timeout.is_zero() {
        None
    } else {
        let started_at = Instant::now();
        Some(AutoDefault {
            selection: default_selection - 1,
            started_at,
            deadline: started_at + timeout,
            active: true,
        })
    };

    interactive_menu_inner(options, auto_default)
}

fn interactive_menu_inner(
    options: &[&str],
    mut auto_default: Option<AutoDefault>,
) -> Result<usize> {
    terminal::enable_raw_mode().context("failed to enable raw terminal input")?;
    let raw_mode = RawModeGuard;
    let mut stdout = io::stdout();
    let mut selected = auto_default
        .as_ref()
        .map(|default| default.selection)
        .unwrap_or(0);
    let mut last_auto_default_tick = None;

    render_interactive_menu(&mut stdout, options, selected, &auto_default)?;

    loop {
        let poll_timeout = auto_default
            .as_ref()
            .and_then(AutoDefault::poll_timeout)
            .unwrap_or_else(|| Duration::from_millis(250));

        if !event::poll(poll_timeout).context("failed to poll keypress")? {
            if let Some(default) = auto_default.as_ref() {
                if default.expired() {
                    let option = options[default.selection];
                    drop(raw_mode);
                    confirm_selection(&mut stdout, option)?;
                    return Ok(default.selection + 1);
                }

                let tick = default.elapsed_seconds();
                if last_auto_default_tick != Some(tick) {
                    rerender_interactive_menu_option(
                        &mut stdout,
                        options,
                        selected,
                        default.selection,
                        &auto_default,
                    )?;
                    last_auto_default_tick = Some(tick);
                }
            }

            continue;
        }

        match event::read().context("failed to read keypress")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    drop(raw_mode);
                    println!();
                    return Err(cancelled());
                }
                KeyCode::Esc => {
                    drop(raw_mode);
                    println!();
                    return Err(cancelled());
                }
                KeyCode::Up => {
                    disable_auto_default(&mut auto_default);
                    selected = previous_selection(selected, options.len());
                    rerender_interactive_menu(&mut stdout, options, selected, &auto_default)?;
                }
                KeyCode::Down => {
                    disable_auto_default(&mut auto_default);
                    selected = next_selection(selected, options.len());
                    rerender_interactive_menu(&mut stdout, options, selected, &auto_default)?;
                }
                KeyCode::Home => {
                    disable_auto_default(&mut auto_default);
                    selected = 0;
                    rerender_interactive_menu(&mut stdout, options, selected, &auto_default)?;
                }
                KeyCode::End => {
                    disable_auto_default(&mut auto_default);
                    selected = options.len() - 1;
                    rerender_interactive_menu(&mut stdout, options, selected, &auto_default)?;
                }
                KeyCode::Enter => {
                    let value = selected + 1;
                    let option = options[selected];
                    drop(raw_mode);
                    confirm_selection(&mut stdout, option)?;
                    return Ok(value);
                }
                KeyCode::Char(character) => {
                    if let Some(value) = selection_from_char(character, options.len()) {
                        let option = options[value - 1];
                        drop(raw_mode);
                        confirm_selection(&mut stdout, option)?;
                        return Ok(value);
                    }

                    disable_auto_default(&mut auto_default);
                    render_interactive_menu_prompt(&mut stdout, &auto_default)?;
                    print!("\x07");
                    io::stdout().flush().context("failed to flush stdout")?;
                }
                _ => {
                    disable_auto_default(&mut auto_default);
                    render_interactive_menu_prompt(&mut stdout, &auto_default)?;
                    print!("\x07");
                    io::stdout().flush().context("failed to flush stdout")?;
                }
            },
            _ => {}
        }
    }
}

struct AutoDefault {
    selection: usize,
    started_at: Instant,
    deadline: Instant,
    active: bool,
}

impl AutoDefault {
    fn expired(&self) -> bool {
        self.active && Instant::now() >= self.deadline
    }

    fn poll_timeout(&self) -> Option<Duration> {
        if !self.active {
            return None;
        }

        Some(
            self.deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(250)),
        )
    }

    fn elapsed_seconds(&self) -> u64 {
        Instant::now()
            .saturating_duration_since(self.started_at)
            .as_secs()
    }
}

fn disable_auto_default(auto_default: &mut Option<AutoDefault>) {
    if let Some(default) = auto_default.as_mut() {
        default.active = false;
    }
}

fn confirm_selection<W: Write>(writer: &mut W, option: &str) -> Result<()> {
    execute!(
        writer,
        Clear(ClearType::CurrentLine),
        cursor::MoveToColumn(0),
        Print(format!("{} {option}\r\n", "✓".with(Color::Green)))
    )?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

fn render_interactive_menu<W: Write>(
    writer: &mut W,
    options: &[&str],
    selected: usize,
    auto_default: &Option<AutoDefault>,
) -> Result<()> {
    for (index, option) in options.iter().enumerate() {
        render_interactive_menu_option(writer, option, index, selected, auto_default)?;
    }

    render_interactive_menu_prompt(writer, auto_default)
}

fn render_interactive_menu_option<W: Write>(
    writer: &mut W,
    option: &str,
    index: usize,
    selected: usize,
    auto_default: &Option<AutoDefault>,
) -> Result<()> {
    let option = option_with_auto_default_suffix(option, index, auto_default);
    execute!(
        writer,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    )?;

    if index == selected {
        execute!(
            writer,
            SetAttribute(Attribute::Reverse),
            Print(format!("› {}. {}", index + 1, option)),
            SetAttribute(Attribute::Reset),
            Print("\r\n")
        )?;
    } else {
        execute!(writer, Print(format!("  {}. {}\r\n", index + 1, option)))?;
    }

    Ok(())
}

fn option_with_auto_default_suffix(
    option: &str,
    index: usize,
    auto_default: &Option<AutoDefault>,
) -> String {
    let Some(default) = auto_default else {
        return option.to_string();
    };

    if !default.active || index != default.selection {
        return option.to_string();
    }

    let dot_count = default.elapsed_seconds() as usize;

    if dot_count == 0 {
        option.to_string()
    } else {
        format!("{}{}", option, ".".repeat(dot_count))
    }
}

fn render_interactive_menu_prompt<W: Write>(
    writer: &mut W,
    auto_default: &Option<AutoDefault>,
) -> Result<()> {
    execute!(
        writer,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        Print(interactive_menu_prompt(auto_default))
    )?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

fn interactive_menu_prompt(_auto_default: &Option<AutoDefault>) -> String {
    "Use ↑↓ + Enter, number key, or ESC to cancel: ".to_string()
}

fn rerender_interactive_menu_option<W: Write>(
    writer: &mut W,
    options: &[&str],
    selected: usize,
    option_index: usize,
    auto_default: &Option<AutoDefault>,
) -> Result<()> {
    execute!(
        writer,
        cursor::SavePosition,
        cursor::MoveUp(rerender_menu_option_line_offset(
            options.len(),
            option_index
        )),
        cursor::MoveToColumn(0)
    )?;
    render_interactive_menu_option(
        writer,
        options[option_index],
        option_index,
        selected,
        auto_default,
    )?;
    execute!(writer, cursor::RestorePosition)?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

fn rerender_interactive_menu<W: Write>(
    writer: &mut W,
    options: &[&str],
    selected: usize,
    auto_default: &Option<AutoDefault>,
) -> Result<()> {
    execute!(
        writer,
        cursor::MoveUp(rerender_menu_line_count(options.len())),
        cursor::MoveToColumn(0)
    )?;
    render_interactive_menu(writer, options, selected, auto_default)
}

fn rerender_menu_line_count(option_count: usize) -> u16 {
    option_count.saturating_add(1) as u16
}

fn rerender_menu_option_line_offset(option_count: usize, option_index: usize) -> u16 {
    option_count.saturating_sub(option_index) as u16
}

fn line_menu(options: &[&str]) -> Result<usize> {
    loop {
        print_options(options);
        let label = format!("Choose 1-{}", options.len());
        let input = prompt(&label)?;

        match input.trim().parse::<usize>() {
            Ok(value) if (1..=options.len()).contains(&value) => {
                success(options[value - 1]);
                return Ok(value);
            }
            _ => status("Please enter one of the numbered options."),
        }
    }
}

fn print_options(options: &[&str]) {
    for (index, option) in options.iter().enumerate() {
        println!("  {}. {option}", index + 1);
    }
}

fn selection_from_char(character: char, option_count: usize) -> Option<usize> {
    let value = character.to_digit(10)? as usize;

    if (1..=option_count).contains(&value) {
        Some(value)
    } else {
        None
    }
}

fn is_wait_cancel_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    match code {
        KeyCode::Esc => true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char(character) => matches!(character.to_ascii_lowercase(), 'c' | 'x'),
        _ => false,
    }
}

fn previous_selection(selected: usize, option_count: usize) -> usize {
    if selected == 0 {
        option_count - 1
    } else {
        selected - 1
    }
}

fn next_selection(selected: usize, option_count: usize) -> usize {
    (selected + 1) % option_count
}

fn is_raw_mode_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("raw terminal input"))
}

pub fn prompt(label: &str) -> Result<String> {
    print!("{} {label}: ", "?".with(Color::Green));
    io::stdout().flush().context("failed to flush stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read input")?;

    Ok(input)
}

pub fn prompt_required(label: &str) -> Result<String> {
    loop {
        let input = prompt(label)?;
        let input = input.trim();

        if !input.is_empty() {
            return Ok(input.to_string());
        }

        status("Please enter a value.");
    }
}

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_digit_to_selection() {
        assert_eq!(selection_from_char('1', 2), Some(1));
        assert_eq!(selection_from_char('2', 2), Some(2));
        assert_eq!(selection_from_char('3', 2), None);
        assert_eq!(selection_from_char('x', 2), None);
    }

    #[test]
    fn recognizes_wait_cancel_keys() {
        assert!(is_wait_cancel_key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(is_wait_cancel_key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(is_wait_cancel_key(KeyCode::Char('C'), KeyModifiers::SHIFT));
        assert!(is_wait_cancel_key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ));
        assert!(is_wait_cancel_key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(is_wait_cancel_key(KeyCode::Char('X'), KeyModifiers::SHIFT));
        assert!(!is_wait_cancel_key(KeyCode::Char('q'), KeyModifiers::NONE));
    }

    #[test]
    fn rounds_countdown_seconds_up() {
        assert_eq!(display_seconds(Duration::ZERO), 0);
        assert_eq!(display_seconds(Duration::from_millis(1)), 1);
        assert_eq!(display_seconds(Duration::from_millis(1_001)), 2);
        assert_eq!(display_seconds(Duration::from_secs(2)), 2);
    }

    #[test]
    fn renders_auto_default_prompt_without_countdown() {
        let auto_default = Some(AutoDefault {
            selection: 0,
            started_at: Instant::now(),
            deadline: Instant::now() + Duration::from_secs(5),
            active: true,
        });

        assert_eq!(
            interactive_menu_prompt(&auto_default),
            "Use ↑↓ + Enter, number key, or ESC to cancel: "
        );
    }

    #[test]
    fn renders_regular_prompt_when_auto_default_is_inactive() {
        let auto_default = Some(AutoDefault {
            selection: 0,
            started_at: Instant::now(),
            deadline: Instant::now() + Duration::from_secs(5),
            active: false,
        });

        assert_eq!(
            interactive_menu_prompt(&auto_default),
            "Use ↑↓ + Enter, number key, or ESC to cancel: "
        );
    }

    #[test]
    fn appends_dots_to_active_default_option() {
        let auto_default = Some(AutoDefault {
            selection: 0,
            started_at: Instant::now() - Duration::from_secs(3),
            deadline: Instant::now() + Duration::from_secs(2),
            active: true,
        });

        assert_eq!(
            option_with_auto_default_suffix("Start scrcpy and close awb", 0, &auto_default),
            "Start scrcpy and close awb..."
        );
        assert_eq!(
            option_with_auto_default_suffix("Start scrcpy and wait", 1, &auto_default),
            "Start scrcpy and wait"
        );
    }

    #[test]
    fn rerender_includes_prompt_line() {
        assert_eq!(rerender_menu_line_count(3), 4);
    }

    #[test]
    fn option_rerender_offset_starts_from_prompt_line() {
        assert_eq!(rerender_menu_option_line_offset(3, 0), 3);
        assert_eq!(rerender_menu_option_line_offset(3, 1), 2);
        assert_eq!(rerender_menu_option_line_offset(3, 2), 1);
    }

    #[test]
    fn wraps_arrow_selection() {
        assert_eq!(previous_selection(0, 3), 2);
        assert_eq!(previous_selection(2, 3), 1);
        assert_eq!(next_selection(2, 3), 0);
        assert_eq!(next_selection(0, 3), 1);
    }
}
