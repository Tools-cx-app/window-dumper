use std::time::{Duration, Instant};

use dumpsys_rs::Dumpsys;

const REFRESH_TIME: Duration = Duration::from_secs(1);

#[derive(Default)]
struct WindowsInfo {
    pub visible_freeform_window: bool,
    pub pid: Option<i32>,
}

struct WindowsDumper {
    cache: WindowsInfo,
    dumper: Dumpsys,
    last_refresh: Instant,
}

impl WindowsInfo {
    fn new(dump: &str) -> Self {
        let pid = Self::parse_top_app(dump);
        let visible_freeform_window = dump.contains("freeform")
            || dump.contains("FlexibleTaskCaptionView")
            || dump.contains("FlexibleTaskIndicatorView");

        Self {
            visible_freeform_window,
            pid,
        }
    }

    fn parse_top_app(dump: &str) -> Option<i32> {
        let Some(focused_app_line) = dump
            .lines()
            .find(|line| line.trim().starts_with("mFocusedApp="))
        else {
            return None;
        };
        let Some(package_name) = Self::extract_package_name(focused_app_line) else {
            return None;
        };

        // Try modern parser, if it fails, fall back to legacy parser.
        let pid = Self::parse_a16_format(dump, package_name)
            .or_else(|| Self::parse_a15_format(dump, package_name));

        pid
    }

    fn extract_package_name(line: &str) -> Option<&str> {
        line.split_whitespace()
            .find(|p| p.contains('/'))?
            .split('/')
            .next()
    }

    // Modern Parser (Android 16+)
    // Parses the PID from the `WINDOW MANAGER WINDOWS` section.
    fn parse_a16_format(dump: &str, package_name: &str) -> Option<i32> {
        let mut in_target_window_section = false;
        for line in dump.lines() {
            if in_target_window_section {
                if line.contains("mSession=") {
                    let session_part = line.split("mSession=").nth(1)?;
                    let content_start = session_part.find('{')? + 1;
                    let content_end = session_part.find('}')?;
                    let content = &session_part[content_start..content_end];
                    let pid_part = content.split_whitespace().nth(1)?;
                    let pid_str = pid_part.split(':').next()?;
                    return pid_str.parse::<i32>().ok();
                }

                if line.contains("Window #") {
                    return None;
                }
            } else if line.contains("Window #") && line.contains(package_name) {
                in_target_window_section = true;
            }
        }

        None
    }

    // Legacy Parser (Android 15 and older)
    // Parses the PID from the `WINDOW MANAGER SESSIONS` section.
    fn parse_a15_format(dump: &str, package_name: &str) -> Option<i32> {
        let mut last_pid_found: Option<i32> = None;
        for line in dump.lines() {
            if line.starts_with("  Session Session{") {
                let content_start = line.find('{')? + 1;
                let content_end = line.find('}')?;
                let content = &line[content_start..content_end];
                let pid_part = content.split_whitespace().nth(1)?;
                let pid_str = pid_part.split(':').next()?;
                last_pid_found = pid_str.parse::<i32>().ok();
            }

            let trimmed_line = line.trim();
            if trimmed_line.starts_with("mPackageName=")
                && let Some(pkg) = trimmed_line.split('=').nth(1)
                && pkg == package_name
            {
                return last_pid_found;
            }
        }

        None
    }
}

impl WindowsDumper {
    fn new() -> Self {
        let dumper = loop {
            match Dumpsys::new() {
                Ok(mut s) => {
                    if s.insert_service("window").is_ok() {
                        break s;
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_secs(1)),
            }
        };

        Self {
            dumper,
            cache: WindowsInfo::default(),
            last_refresh: Instant::now(),
        }
    }

    fn cache(&mut self) -> &WindowsInfo {
        if self.last_refresh.elapsed() > REFRESH_TIME {
            let dump = loop {
                match self.dumper.dump("window", &["visible-apps"]) {
                    Ok(s) => break s,
                    Err(e) => {
                        eprintln!("Failed to dump windows: {e}, retrying");
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            };

            self.cache = WindowsInfo::new(&dump);
            self.last_refresh = Instant::now();
        }

        &self.cache
    }
}

fn main() {
    let mut dumper = WindowsDumper::new();
    loop {
        let cache = dumper.cache();
        println!(
            "{:?}, freeform: {}",
            cache.pid, cache.visible_freeform_window
        );
        std::thread::sleep(Duration::from_secs(1));
    }
}
