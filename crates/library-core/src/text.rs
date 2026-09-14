//! The numbers the library shows as words: how big a file is, how long ago something happened, and how many of a thing there are.

/// `None`, `""` and a run of spaces all answer `None`; anything else answers itself, untrimmed.
pub fn non_blank(text: Option<&str>) -> Option<&str> {
    text.filter(|t| !t.trim().is_empty())
}

/// Binary units, decimal spelling, and one decimal below 10 of a unit so every size keeps the same width in a menu column.
pub fn human_size(bytes: u64) -> String {
    const STEP: f64 = 1024.0;
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= STEP && unit < UNITS.len() - 1 {
        value /= STEP;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{:.0} {}", value, UNITS[unit])
    }
}

/// Deliberately coarse past a day, and a stamp in the future (a clock that moved, a hand-edited blob) reads as "just now" rather than as a negative.
pub fn human_age(then_ms: u64, now_ms: u64) -> String {
    const MINUTE: u64 = 60_000;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    let elapsed = now_ms.saturating_sub(then_ms);
    if elapsed < MINUTE {
        return "just now".to_string();
    }
    if elapsed < HOUR {
        return ago(elapsed / MINUTE, "minute", "minutes");
    }
    if elapsed < DAY {
        return ago(elapsed / HOUR, "hour", "hours");
    }
    if elapsed < WEEK {
        return ago(elapsed / DAY, "day", "days");
    }
    ago(elapsed / WEEK, "week", "weeks")
}

fn ago(count: u64, one: &str, many: &str) -> String {
    format!("{} ago", plural(count as usize, one, many))
}

/// The irregular plural is the caller's: `plural(2, "shelf", "shelves")`.
pub fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

pub fn page_line(page: u32, num_pages: u32) -> String {
    if num_pages > 0 {
        format!("Page {page} of {num_pages}")
    } else {
        format!("Page {page}")
    }
}

pub fn display_or_stem(title: Option<&str>, path: &str) -> String {
    if let Some(title) = non_blank(title) {
        return title.to_string();
    }
    crate::book::stem_of(path)
}

#[cfg(test)]
mod tests {
    use super::{display_or_stem, human_age, human_size, non_blank, page_line, plural};

    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    #[test]
    fn bytes_read_as_bytes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1), "1 B");
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn a_size_keeps_one_decimal_until_it_does_not_need_one() {
        assert_eq!(human_size(KB), "1.0 KB");
        assert_eq!(human_size(3 * MB + 200 * KB), "3.2 MB");
        assert_eq!(human_size(3 * GB + 200 * MB), "3.2 GB");
        assert_eq!(human_size(12 * MB + 400 * KB), "12 MB");
        assert_eq!(human_size(30 * KB), "30 KB");
        assert_eq!(human_size(500 * MB), "500 MB");
        assert_eq!(human_size(14 * GB), "14 GB");
    }

    #[test]
    fn the_units_stop_at_the_last_one_they_have() {
        assert_eq!(human_size(2048 * GB), "2.0 TB");
        assert!(
            human_size(u64::MAX).ends_with(" TB"),
            "the largest count a u64 can hold still gets a unit"
        );
    }

    #[test]
    fn an_age_is_coarse_where_coarse_is_enough() {
        let now = 1_700_000_000_000u64;
        let minute = 60_000;
        assert_eq!(human_age(now, now), "just now");
        assert_eq!(human_age(now - 30_000, now), "just now");
        assert_eq!(human_age(now - minute, now), "1 minute ago");
        assert_eq!(human_age(now - 3 * minute, now), "3 minutes ago");
        assert_eq!(human_age(now - 59 * minute, now), "59 minutes ago");
        assert_eq!(human_age(now - 60 * minute, now), "1 hour ago");
        assert_eq!(human_age(now - 5 * 60 * minute, now), "5 hours ago");
        assert_eq!(human_age(now - 24 * 60 * minute, now), "1 day ago");
        assert_eq!(human_age(now - 3 * 24 * 60 * minute, now), "3 days ago");
        assert_eq!(human_age(now - 6 * 24 * 60 * minute, now), "6 days ago");
        assert_eq!(human_age(now - 7 * 24 * 60 * minute, now), "1 week ago");
        assert_eq!(human_age(now - 21 * 24 * 60 * minute, now), "3 weeks ago");
    }

    #[test]
    fn a_count_reads_as_one_thing_or_many() {
        assert_eq!(plural(1, "book", "books"), "1 book");
        assert_eq!(plural(3, "book", "books"), "3 books");
        assert_eq!(plural(0, "shelf", "shelves"), "0 shelves");
        assert_eq!(plural(2, "shelf", "shelves"), "2 shelves");
    }

    #[test]
    fn a_blank_name_is_no_name() {
        assert_eq!(non_blank(None), None);
        assert_eq!(non_blank(Some("")), None);
        assert_eq!(non_blank(Some("   ")), None);
        assert_eq!(non_blank(Some(" Dune ")), Some(" Dune "));
    }

    #[test]
    fn a_resume_point_reads_as_one_line() {
        assert_eq!(page_line(12, 340), "Page 12 of 340");
        assert_eq!(page_line(12, 0), "Page 12");
        assert_eq!(page_line(1, 1), "Page 1 of 1");
    }

    #[test]
    fn a_name_falls_back_to_the_stem_and_then_to_the_address() {
        assert_eq!(display_or_stem(Some("Dune"), "/books/x.pdf"), "Dune");
        assert_eq!(display_or_stem(Some("  "), "/books/dune.pdf"), "dune");
        assert_eq!(display_or_stem(None, "/books/dune.pdf"), "dune");
        assert_eq!(display_or_stem(None, ""), "");
    }

    #[test]
    fn a_stamp_in_the_future_is_not_a_negative_age() {
        assert_eq!(human_age(1_800_000_000_000, 1_700_000_000_000), "just now");
    }
}
