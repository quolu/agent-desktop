const STARTTIME_FIELD_AFTER_COMM: usize = 19;

/// The process generation token: the kernel start time in clock ticks since
/// boot (`/proc/<pid>/stat` field 22), so a recycled pid never matches a
/// window or ref recorded against an earlier process.
pub(crate) fn process_instance(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_start_time(&stat).map(|ticks| format!("linux-proc-v1:{ticks}"))
}

/// The executable name the kernel records for the process, which is what a
/// user usually types (`chrome`, `nautilus`) when the accessibility name is
/// the product name (`Google Chrome`, `Files`).
pub(crate) fn command_name(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn parse_start_time(stat: &str) -> Option<u64> {
    let after_comm = &stat[stat.rfind(')')? + 1..];
    after_comm
        .split_whitespace()
        .nth(STARTTIME_FIELD_AFTER_COMM)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_time_is_read_past_a_command_name_with_spaces_and_parens() {
        let stat = "4242 (Web Content (x)) S 1 4242 4242 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 30 0 987654 123 456";
        assert_eq!(parse_start_time(stat), Some(987_654));
    }

    #[test]
    fn a_truncated_stat_line_has_no_start_time() {
        assert_eq!(parse_start_time("4242 (x) S 1 2"), None);
        assert_eq!(parse_start_time("garbage"), None);
    }

    #[test]
    fn this_process_has_an_instance_and_a_name() {
        let pid = std::process::id();
        assert!(process_instance(pid).is_some_and(|token| token.starts_with("linux-proc-v1:")));
        assert!(command_name(pid).is_some());
    }
}
