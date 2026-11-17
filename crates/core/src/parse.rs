//! Parsing what people type into a cell (and numeric text in formulas).
//!
//! Recognises numbers with thousands separators, percentages, currency, scientific notation,
//! fractions (`1 1/2`), dates (`3/4/2026`, `2026-03-04`, `4-Mar-2026`, `Mar 4, 2026`), times
//! (`10:30`, `10:30:15 PM`) and date-times. Locale: en-US.

use crate::date::{DateSystem, MONTHS, serial_from_ymd, time_fraction};
use crate::value::{CellError, Value};

/// A parsed entry: its value and the number format Excel would apply automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub format: Option<&'static str>,
}

/// Interprets typed cell input (not formulas). Text that isn't a number, date, boolean or error
/// stays text. A leading apostrophe forces text.
pub fn parse_input(s: &str, sys: DateSystem) -> Parsed {
    if s.is_empty() {
        return Parsed { value: Value::Empty, format: None };
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return Parsed { value: Value::text(rest), format: None };
    }
    let t = s.trim();
    if t.eq_ignore_ascii_case("TRUE") {
        return Parsed { value: Value::Bool(true), format: None };
    }
    if t.eq_ignore_ascii_case("FALSE") {
        return Parsed { value: Value::Bool(false), format: None };
    }
    // Every error value starts with '#', and the full parse lowercases the field to match, so it
    // only runs for the fields that can be one.
    if t.starts_with('#')
        && let Some(e) = CellError::parse(t)
    {
        return Parsed { value: Value::Error(e), format: None };
    }
    if let Some((n, f)) = parse_numeric(t, sys) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

/// Numeric text as arithmetic coerces it ("1,000", "50%", "$3", "1/2/2020", "10:00").
pub fn parse_number_text(s: &str) -> Option<f64> {
    parse_number_text_in(s, DateSystem::D1900)
}

/// Like [`parse_number_text`], with date text read in the given workbook date system.
pub fn parse_number_text_in(s: &str, sys: DateSystem) -> Option<f64> {
    parse_numeric(s.trim(), sys).map(|(n, _)| n)
}

/// Text that Excel's "Number Stored as Text" check flags: it would be a number if typed (signs,
/// thousands separators, currency, percent, exponent, a parenthesised negative). Dates, times and
/// fractions kept as text are not flagged, as in Excel.
pub fn number_stored_as_text(s: &str) -> Option<f64> {
    let t = s.trim();
    match t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(inner) => parse_plain(inner.trim()).map(|(n, _)| -n),
        None => parse_plain(t).map(|(n, _)| n),
    }
}

fn parse_numeric(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    // Parenthesised negatives: (100) = -100. Each level negates, so ((100)) = 100. Stripped in a
    // loop, not by recursion: runtime text (e.g. from REPT) can nest thousands deep (#2180).
    let mut t = t;
    let mut neg = false;
    while let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        t = inner.trim();
        neg = !neg;
    }
    let (n, f) = parse_unparenthesised(t, sys)?;
    Some((if neg { -n } else { n }, f))
}

/// The attempts in order, with the cheap classifications that let a field skip the ones it cannot
/// possibly match. A CSV field is text or a number far more often than it is a date, so each of the
/// three attempts is gated by a byte test instead of running its splits and lowercases first.
fn parse_unparenthesised(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    let first = first_class(*t.as_bytes().first()?);
    // No number, date, time or fraction begins with anything else, and only a month name begins a
    // date with a letter: the two branches that decide most fields.
    if first == First::Other {
        return None;
    }
    if first == First::Letter && !month_starts_at(t.as_bytes(), 0) {
        return None;
    }
    if let Some((n, sci)) = fast_number(t) {
        return Some((n, if sci { Some("0.00E+00") } else { None }));
    }
    let shape = Shape::of(t);
    if shape.number
        && let Some(r) = parse_plain(t)
    {
        return Some(r);
    }
    if !shape.date {
        return None;
    }
    // A fraction, a numeric date and a time each have a byte of their own (':' '/' '-' '.' or an
    // am/pm ending); a month name stands in for it in "4 Mar 2026".
    if !shape.marker && !shape.month {
        return None;
    }
    if shape.marker
        && let Some(r) = parse_fraction(t)
    {
        return Some((r, Some("# ?/?")));
    }
    parse_datetime(t, sys)
}

/// What a field's first byte can start: one branch, before any attempt runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum First {
    /// '0'-'9' or '.': a plain number or a numeric date.
    Digit,
    /// '+', '-' or '$': a signed or currency number.
    Sign,
    /// An ASCII letter: only a month name starts a date there.
    Letter,
    /// Anything else: text, since nothing parses from it.
    Other,
}

fn first_class(b: u8) -> First {
    match b {
        b'0'..=b'9' | b'.' => First::Digit,
        b'+' | b'-' | b'$' => First::Sign,
        b'a'..=b'z' | b'A'..=b'Z' => First::Letter,
        _ => First::Other,
    }
}

/// Bytes a plain number can hold, after its sign: digits, a decimal point, an exponent marker, a
/// sign, a thousands separator, a currency marker, a percent sign and the whitespace "parse_plain"
/// trims. A field with any other byte is text and never pays for the attempt.
const NUMBER_BYTES: [bool; 256] = {
    let mut t = [false; 256];
    let mut d = b'0';
    while d <= b'9' {
        t[d as usize] = true;
        d += 1;
    }
    t[b'.' as usize] = true;
    t[b'e' as usize] = true;
    t[b'E' as usize] = true;
    t[b'+' as usize] = true;
    t[b'-' as usize] = true;
    t[b',' as usize] = true;
    t[b'$' as usize] = true;
    t[b'%' as usize] = true;
    let mut w = b'\t';
    while w <= b'\r' {
        t[w as usize] = true;
        w += 1;
    }
    t[b' ' as usize] = true;
    t
};

/// Bytes a date, a time or a fraction can hold: alphanumerics (month names) and ' , / - . :'.
const DATE_BYTES: [bool; 256] = {
    let mut t = [false; 256];
    let mut c = b'0';
    while c <= b'9' {
        t[c as usize] = true;
        c += 1;
    }
    c = b'a';
    while c <= b'z' {
        t[c as usize] = true;
        c += 1;
    }
    c = b'A';
    while c <= b'Z' {
        t[c as usize] = true;
        c += 1;
    }
    let mut w = b'\t';
    while w <= b'\r' {
        t[w as usize] = true;
        w += 1;
    }
    t[b' ' as usize] = true;
    t[b',' as usize] = true;
    t[b'/' as usize] = true;
    t[b'-' as usize] = true;
    t[b'.' as usize] = true;
    t[b':' as usize] = true;
    t
};

/// What one scan of a field says about which attempts could still match it.
struct Shape {
    /// Every byte could appear in a plain number.
    number: bool,
    /// Every byte could appear in a date, a time or a fraction.
    date: bool,
    /// A byte only a numeric date or a time has (':' '/' '-' '.'), or an am/pm ending ("5 pm").
    marker: bool,
    /// A word starts with a month's first three letters, so a month-name date could.
    month: bool,
}

impl Shape {
    fn of(t: &str) -> Shape {
        let b = t.as_bytes();
        let mut s = Shape { number: true, date: true, marker: false, month: false };
        let mut alpha_before = false;
        for (i, &c) in b.iter().enumerate() {
            // Outside ASCII anything is possible: a no-break space is whitespace the chain trims
            // and a fullwidth digit is no digit at all, so hand the field to the chain untouched
            // rather than classify it on a byte test.
            if !c.is_ascii() {
                return Shape { number: true, date: true, marker: true, month: true };
            }
            s.number &= NUMBER_BYTES[usize::from(c)];
            s.date &= DATE_BYTES[usize::from(c)];
            s.marker |= matches!(c, b':' | b'/' | b'-' | b'.');
            if !alpha_before && c.is_ascii_alphabetic() {
                s.month |= month_starts_at(b, i);
            }
            alpha_before = c.is_ascii_alphabetic();
            // Text that is neither can stop the scan however long the field is.
            if !s.number && !s.date {
                break;
            }
        }
        s.marker |= ampm_ending(b);
        s
    }
}

/// ASCII whitespace as the chain trims it — `str::trim` also trims the Unicode ones, which the
/// gate leaves to the chain (a byte outside ASCII is not classified).
fn is_space(c: u8) -> bool {
    // `u8::is_ascii_whitespace` leaves out the vertical tab, which `char::is_whitespace` trims.
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Whether a time's am/pm marker ends the field with the digits of the time right behind it
/// ("5 pm", "10:30 PM"). Text that merely ends in a/p/m ("Alabama", "Palm") is not a time.
fn ampm_ending(b: &[u8]) -> bool {
    let Some(last) = b.last().map(u8::to_ascii_lowercase) else {
        return false;
    };
    // The marker is "am"/"pm" or Excel's single letter "a"/"p"; an "m" on its own is not one.
    let mut i = match last {
        b'a' | b'p' => b.len() - 1,
        b'm' => {
            let prev = b.len().checked_sub(2).and_then(|j| b.get(j)).map(u8::to_ascii_lowercase);
            if !matches!(prev, Some(b'a' | b'p')) {
                return false;
            }
            b.len() - 2
        }
        _ => return false,
    };
    // parse_time trims the text before the marker, so any whitespace can sit there ("5   pm").
    while i > 0 {
        let Some(c) = b.get(i - 1) else {
            return false;
        };
        if c.is_ascii_digit() {
            return true;
        }
        if is_space(*c) {
            i -= 1;
            continue;
        }
        // Outside ASCII the character may be whitespace the chain trims (a no-break space): let
        // the chain decide instead of guessing.
        return !c.is_ascii();
    }
    false
}

/// Whether a month name could start at b[i]: the first three letters, in either case, are one of
/// January through December, which are all distinct in three letters.
fn month_starts_at(b: &[u8], i: usize) -> bool {
    let Some(w) = b.get(i..).and_then(|rest| rest.get(..3)) else {
        return false;
    };
    let mut l = [0u8; 3];
    for (o, &c) in w.iter().enumerate() {
        l[o] = c.to_ascii_lowercase();
    }
    matches!(&l, b"jan" | b"feb" | b"mar" | b"apr" | b"may" | b"jun" | b"jul" | b"aug" | b"sep" | b"oct" | b"nov" | b"dec")
}

/// Numbers of the common shape (an optional sign, digits, an optional fraction and an optional 'e'
/// exponent) recognised in one pass, with no attempt at a date. Some only for text that
/// [parse_plain] reads to the same value and format (the bool says it is in scientific notation,
/// whose format parse_plain sets); None hands anything else back to the general chain.
fn fast_number(t: &str) -> Option<(f64, bool)> {
    let b = t.as_bytes();
    let (start, neg) = match b.first() {
        Some(b'-') => (1, true),
        Some(b'+') => (1, false),
        _ => (0, false),
    };
    // parse_plain trims the whitespace a sign leaves behind ("- 5").
    let mut i = start;
    while b.get(i).copied().is_some_and(is_space) {
        i += 1;
    }
    let int_start = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let int_len = i - int_start;
    let mut frac = false;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let frac_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        // "123." is left to the chain, which reads it the way it always has.
        if i == frac_start {
            return None;
        }
        frac = true;
    }
    if int_len == 0 && !frac {
        return None;
    }
    let mut sci = false;
    if matches!(b.get(i), Some(b'e' | b'E')) {
        sci = true;
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        // "1e", "1e+": not a number, and only the chain knows what else they are.
        if i == exp_start {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    let digits = t.get(int_start..)?;
    let n = if !frac && !sci && int_len <= 19 {
        // Digits only, short enough to count exactly: "as f64" rounds the integer the same way
        // the nearest-even parse of the same digits would, without the general parser.
        let mut acc: u64 = 0;
        for &c in digits.as_bytes() {
            acc = acc.checked_mul(10)?.checked_add(u64::from(c.wrapping_sub(b'0')))?;
        }
        acc as f64
    } else {
        let n: f64 = digits.parse().ok()?;
        if !n.is_finite() {
            return None;
        }
        n
    };
    Some((if neg { -n } else { n }, sci))
}

/// Signs, currency, thousands separators, percent, exponent.
fn parse_plain(t: &str) -> Option<(f64, Option<&'static str>)> {
    let mut s = t;
    let mut neg = false;
    let mut currency = false;
    let mut percent = false;
    if let Some(r) = s.strip_prefix('-') {
        neg = true;
        s = r.trim_start();
    } else if let Some(r) = s.strip_prefix('+') {
        s = r.trim_start();
    }
    if let Some(r) = s.strip_prefix('$') {
        currency = true;
        s = r.trim_start();
        if !neg && let Some(r2) = s.strip_prefix('-') {
            neg = true;
            s = r2;
        }
    }
    if let Some(r) = s.strip_suffix('%') {
        percent = true;
        s = r.trim_end();
    }
    if s.is_empty() {
        return None;
    }
    let has_sep = s.contains(',');
    let digits: String = if has_sep {
        // Thousands separators must be in groups of three before the decimal point.
        let (int, frac) = s.split_once('.').map_or((s, None), |(a, b)| (a, Some(b)));
        let groups: Vec<&str> = int.split(',').collect();
        let first = groups.first()?;
        if first.is_empty() || first.len() > 3 || groups.iter().skip(1).any(|g| g.len() != 3) {
            return None;
        }
        let mut d = groups.concat();
        if let Some(f) = frac {
            d.push('.');
            d.push_str(f);
        }
        d
    } else {
        s.to_string()
    };
    let b = digits.as_bytes();
    if !b.iter().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    if !b.first().is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
        return None;
    }
    let mut n: f64 = digits.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    let sci = digits.contains(['e', 'E']);
    if percent {
        n /= 100.0;
    }
    if neg {
        n = -n;
    }
    let fmt = if currency {
        Some(if digits.contains('.') { "\"$\"#,##0.00" } else { "\"$\"#,##0" })
    } else if percent {
        Some(if digits.contains('.') { "0.00%" } else { "0%" })
    } else if sci {
        Some("0.00E+00")
    } else if has_sep {
        Some(if digits.contains('.') { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    Some((n, fmt))
}

/// A plain ASCII number: digits, with an optional `.digits` fraction when `frac` is set. Rejects
/// `nan`, `inf`, signs and exponents that `f64::from_str` would accept.
fn plain_number(s: &str, frac: bool) -> Option<f64> {
    let (int, dec) = match s.split_once('.') {
        Some((i, d)) if frac => (i, Some(d)),
        _ => (s, None),
    };
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit());
    if !digits(int) || dec.is_some_and(|d| !digits(d)) {
        return None;
    }
    s.parse().ok()
}

/// `1/2` (only with a whole part, else it's a date), `1 1/2`, `0 3/4`.
fn parse_fraction(t: &str) -> Option<f64> {
    let (whole, frac) = t.split_once(' ')?;
    let neg = whole.starts_with('-');
    let w = plain_number(whole.trim_start_matches('-'), false)?;
    let (a, b) = frac.trim().split_once('/')?;
    let a = plain_number(a, false)?;
    let b = plain_number(b, false)?;
    if b == 0.0 || !w.is_finite() {
        return None;
    }
    let v = w + a / b;
    Some(if neg { -v } else { v })
}

fn month_from_name(s: &str) -> Option<u32> {
    let l = s.to_ascii_lowercase();
    if l.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.to_ascii_lowercase().starts_with(&l)).map(|i| i as u32 + 1)
}

/// Time part: `h:mm`, `h:mm:ss`, `h:mm:ss.000`, `h AM`, with optional AM/PM.
fn parse_time(t: &str) -> Option<(f64, bool, bool)> {
    let t = t.trim();
    let lower = t.to_ascii_lowercase();
    let (body, ampm) = if let Some(b) = lower.strip_suffix("am").or_else(|| lower.strip_suffix('a')) {
        (b.trim().to_string(), Some(false))
    } else if let Some(b) = lower.strip_suffix("pm").or_else(|| lower.strip_suffix('p')) {
        (b.trim().to_string(), Some(true))
    } else {
        (lower.clone(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() == 1 && ampm.is_none() {
        return None;
    }
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty() || p.starts_with('+')) {
        return None;
    }
    let mut h = plain_number(parts.first()?, false)?;
    let m = parts.get(1).map_or(Some(0.0), |p| plain_number(p, false))?;
    let s = parts.get(2).map_or(Some(0.0), |p| plain_number(p, true))?;
    if m >= 60.0 || s >= 60.0 || h < 0.0 || m < 0.0 || s < 0.0 {
        return None;
    }
    if let Some(pm) = ampm {
        if !(0.0..=12.0).contains(&h) {
            return None;
        }
        if h == 12.0 {
            h = 0.0;
        }
        if pm {
            h += 12.0;
        }
    } else if h >= 10000.0 {
        return None;
    }
    Some((time_fraction(h, m, s), parts.len() == 3, ampm.is_some()))
}

fn parse_date(t: &str, sys: DateSystem) -> Option<(f64, &'static str)> {
    let t = t.trim().trim_end_matches(',');
    let num = |s: &str| {
        let s = s.trim();
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse::<i64>().ok()
    };
    // ISO yyyy-mm-dd or yyyy/mm/dd
    let seps: &[char] = &['/', '-', '.'];
    let parts: Vec<&str> = t.split(seps).collect();
    if parts.len() == 3 {
        let (a, b, c) = (parts[0].trim(), parts[1].trim(), parts[2].trim());
        if a.len() == 4
            && let (Some(y), Some(m), Some(d)) = (num(a), num(b), num(c))
        {
            return valid_ymd(sys, y, m, d).map(|s| (s, "yyyy-mm-dd"));
        }
        if let (Some(m), Some(d), Some(y)) = (num(a), num(b), num(c)) {
            let y = if c.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m, d).map(|s| (s, "m/d/yyyy"));
        }
        // 4-Mar-2026
        if let (Some(d), Some(m), Some(y)) = (num(a), month_from_name(b), num(c)) {
            let y = if c.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    if parts.len() == 2 {
        let (a, b) = (parts[0].trim(), parts[1].trim());
        // m/d in the current year is ambiguous without a clock; use month/day of 2026? Excel uses
        // the current year. We accept d-mmm / mmm-d only with month names, and m/d → current year.
        if let (Some(m), Some(d)) = (num(a), num(b)) {
            let y = current_year();
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                return valid_ymd(sys, y, m, d).map(|s| (s, "d-mmm"));
            }
            // m/yyyy
            if (1..=12).contains(&m) && b.len() == 4 {
                return valid_ymd(sys, d, m, 1).map(|s| (s, "mmm-yy"));
            }
        }
        if let (Some(d), Some(m)) = (num(a), month_from_name(b)) {
            return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
        }
        if let (Some(m), Some(y)) = (month_from_name(a), num(b)) {
            let y = if b.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m as i64, 1).map(|s| (s, "mmm-yy"));
        }
    }
    // "Mar 4, 2026" / "March 4 2026" / "4 March 2026"
    let words: Vec<&str> = t.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    if words.len() == 3 {
        // Two-digit years follow the same 2029 rule as the dashed forms above.
        let expand = |w: &str, y: i64| if w.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
        if let (Some(m), Some(d), Some(y)) = (month_from_name(words[0]), num(words[1]), num(words[2])) {
            let y = expand(words[2], y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        if let (Some(d), Some(m), Some(y)) = (num(words[0]), month_from_name(words[1]), num(words[2])) {
            let y = expand(words[2], y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    None
}

fn valid_ymd(sys: DateSystem, y: i64, m: i64, d: i64) -> Option<f64> {
    // Dates before 1900 stay text (`DATE()` would add 1900 to such a year).
    if !(1..=12).contains(&m) || d < 1 || y < 1900 {
        return None;
    }
    let max = crate::date::days_in_month_in(sys, y as i32, m as u32) as i64;
    if d > max {
        return None;
    }
    serial_from_ymd(sys, y, m, d)
}

/// The year used for dates typed without one, from the system clock (browser clock on wasm).
/// Falls back to 2026 only when no clock is available.
pub fn current_year() -> i64 {
    if let Some(ms) = crate::date::unix_now_ms() {
        let days = (ms / 86_400_000.0).floor() as i64;
        return crate::date::civil_from_days(days).0 as i64;
    }
    2026
}

fn parse_datetime(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    if let Some((tf, secs, ampm)) = parse_time(t) {
        let f = match (secs, ampm) {
            // 24 hours or more: elapsed hours, so 25:00 shows as 25:00:00, not 1:00.
            _ if tf >= 1.0 => "[h]:mm:ss",
            (false, false) => "h:mm",
            (true, false) => "h:mm:ss",
            (false, true) => "h:mm AM/PM",
            (true, true) => "h:mm:ss AM/PM",
        };
        return Some((tf, Some(f)));
    }
    if let Some((d, f)) = parse_date(t, sys) {
        return Some((d, Some(f)));
    }
    // date + time: split at the last space(s) before a time-looking tail.
    for (i, _) in t.match_indices(' ') {
        let (a, b) = (t.get(..i)?, t.get(i + 1..)?);
        if let (Some((d, _)), Some((tf, _, _))) = (parse_date(a, sys), parse_time(b)) {
            return Some((d + tf, Some("m/d/yyyy h:mm")));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Parsed {
        parse_input(s, DateSystem::D1900)
    }

    #[test]
    fn non_digit_time_and_fraction_parts_stay_text() {
        for s in ["1:nan", "nan:30", "1:30:NaN", "1 nan/2", "1 1/nan", "inf:00", "1e1:00", "+5:00"] {
            assert_eq!(p(s).value, Value::text(s), "{s}");
        }
    }

    #[test]
    fn signed_date_parts_stay_text() {
        for s in ["+3/4/2026", "3/+4/2026", "3/4/+2026", "1/1/2020 +5:00"] {
            assert_eq!(p(s).value, Value::text(s), "{s}");
        }
        assert_eq!(p("3/4/2026").value, Value::Number(46085.0));
    }

    #[test]
    fn month_name_short_year() {
        for s in ["Mar 4, 26", "Mar 4 26", "March 4 26", "4 March 26", "Mar 4, 2026"] {
            assert_eq!(p(s).value, Value::Number(46085.0), "{s}");
        }
        assert_eq!(p("Mar 4, 99").value, p("Mar 4, 1999").value);
        assert_eq!(p("Mar 4, 1776").value, Value::text("Mar 4, 1776"));
        assert_eq!(p("Feb 30, 26").value, Value::text("Feb 30, 26"));
    }

    #[test]
    fn numbers() {
        assert_eq!(p("42").value, Value::Number(42.0));
        assert_eq!(p("-3.5").value, Value::Number(-3.5));
        assert_eq!(p("1,234").format, Some("#,##0"));
        assert_eq!(p("1,234").value, Value::Number(1234.0));
        assert_eq!(p("1,23").value, Value::text("1,23"));
        assert_eq!(p("12%").value, Value::Number(0.12));
        assert_eq!(p("$1,200.50").value, Value::Number(1200.5));
        assert_eq!(p("$1,200.50").format, Some("\"$\"#,##0.00"));
        assert_eq!(p("(100)").value, Value::Number(-100.0));
        assert_eq!(p("1e3").value, Value::Number(1000.0));
        assert_eq!(p("1 1/2").value, Value::Number(1.5));
        assert_eq!(p(".5").value, Value::Number(0.5));
    }

    #[test]
    fn deeply_nested_parentheses_do_not_overflow() {
        // #2180: coercion used to recurse once per parenthesis and overflow the stack.
        let depth = 100_000;
        let s = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
        assert_eq!(parse_number_text(&s), Some(1.0));
        let odd = format!("{}1{}", "(".repeat(depth + 1), ")".repeat(depth + 1));
        assert_eq!(parse_number_text(&odd), Some(-1.0));
        let unbalanced = format!("{}1{}", "(".repeat(depth), ")".repeat(depth - 1));
        assert_eq!(parse_number_text(&unbalanced), None);
        assert_eq!(p(&s).value, Value::Number(1.0));
        // Existing results are unchanged.
        assert_eq!(parse_number_text("(100)"), Some(-100.0));
        assert_eq!(parse_number_text("((100))"), Some(100.0));
        assert_eq!(parse_number_text("( $1,200.50 )"), Some(-1200.5));
        assert_eq!(parse_number_text("()"), None);
    }

    #[test]
    fn texts_and_specials() {
        assert_eq!(p("hello").value, Value::text("hello"));
        assert_eq!(p("'123").value, Value::text("123"));
        assert_eq!(p("true").value, Value::Bool(true));
        assert_eq!(p("#N/A").value, Value::Error(CellError::NA));
        assert_eq!(p("e").value, Value::text("e"));
        assert_eq!(p("-").value, Value::text("-"));
        assert_eq!(p("1.2.3.4").value, Value::text("1.2.3.4"));
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(p("10/7/2026").value, Value::Number(46302.0));
        assert_eq!(p("2026-10-07").value, Value::Number(46302.0));
        assert_eq!(p("7-Oct-2026").value, Value::Number(46302.0));
        assert_eq!(p("Oct 7, 2026").value, Value::Number(46302.0));
        assert_eq!(p("2/30/2026").value, Value::text("2/30/2026"));
        assert_eq!(p("7/4/1776").value, Value::text("7/4/1776"));
        assert_eq!(p("1776-07-04").value, Value::text("1776-07-04"));
        assert_eq!(p("4-Jul-1776").value, Value::text("4-Jul-1776"));
        assert_eq!(p("12/31/1899").value, Value::text("12/31/1899"));
        assert_eq!(p("1/1/1900").value, Value::Number(1.0));
        assert_eq!(p("12:00").value, Value::Number(0.5));
        assert_eq!(p("6:00 PM").value, Value::Number(0.75));
        assert_eq!(p("12:00 AM").value, Value::Number(0.0));
        assert_eq!(p("10/7/2026 18:00").value, Value::Number(46302.75));
        assert_eq!(p("25:00").value, Value::Number(25.0 / 24.0));
        assert_eq!(p("25:00").format, Some("[h]:mm:ss"));
        assert_eq!(p("23:59").format, Some("h:mm"));
    }

    #[test]
    fn fictitious_1900_02_29() {
        // Excel accepts 29 February 1900 (serial 60) and keeps 1 March 1900 at 61.
        assert_eq!(p("2/29/1900").value, Value::Number(60.0));
        assert_eq!(p("1900-02-29").value, Value::Number(60.0));
        assert_eq!(p("29-Feb-1900").value, Value::Number(60.0));
        assert_eq!(p("2/28/1900").value, Value::Number(59.0));
        assert_eq!(p("3/1/1900").value, Value::Number(61.0));
        assert_eq!(p("2/29/1901").value, Value::text("2/29/1901"));
        assert_eq!(p("2/30/1900").value, Value::text("2/30/1900"));
        // The 1904 system starts after it.
        assert_eq!(parse_input("2/29/1900", DateSystem::D1904).value, Value::text("2/29/1900"));
        assert_eq!(parse_input("2/29/1904", DateSystem::D1904).value, Value::Number(59.0));
    }

    /// The sniffer as it was before the fast path: the same attempts, in the same order, with
    /// nothing skipped and nothing classified. It shares [parse_plain], [parse_fraction] and
    /// [parse_datetime] with the real one on purpose — what the property test below puts on trial
    /// is the new first-byte classification and the fast number, not those attempts.
    fn reference_parse_input(s: &str, sys: DateSystem) -> Parsed {
        if s.is_empty() {
            return Parsed { value: Value::Empty, format: None };
        }
        if let Some(rest) = s.strip_prefix('\'') {
            return Parsed { value: Value::text(rest), format: None };
        }
        let t = s.trim();
        if t.eq_ignore_ascii_case("TRUE") {
            return Parsed { value: Value::Bool(true), format: None };
        }
        if t.eq_ignore_ascii_case("FALSE") {
            return Parsed { value: Value::Bool(false), format: None };
        }
        if let Some(e) = CellError::parse(t) {
            return Parsed { value: Value::Error(e), format: None };
        }
        if let Some((n, f)) = reference_numeric(t, sys) {
            return Parsed { value: Value::number(n), format: f };
        }
        Parsed { value: Value::text(s), format: None }
    }

    fn reference_numeric(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
        let mut t = t;
        let mut neg = false;
        while let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
            t = inner.trim();
            neg = !neg;
        }
        let (n, f) = reference_unparenthesised(t, sys)?;
        Some((if neg { -n } else { n }, f))
    }

    fn reference_unparenthesised(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
        if t.is_empty() {
            return None;
        }
        if let Some(r) = parse_plain(t) {
            return Some(r);
        }
        if let Some(r) = parse_fraction(t) {
            return Some((r, Some("# ?/?")));
        }
        parse_datetime(t, sys)
    }

    /// Eight bytes of splitmix64 at a time, so every run and every machine generates the same
    /// fields (a property test that only sometimes finds a disagreement is worse than none).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        /// A number below `n` (0 for an empty range).
        fn below(&mut self, n: usize) -> usize {
            if n == 0 {
                return 0;
            }
            let n = u64::try_from(n).unwrap_or(u64::MAX);
            usize::try_from(self.next() % n).unwrap_or(0)
        }

        fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
            xs.get(self.below(xs.len())).copied().unwrap_or("")
        }

        /// 1..=`max` digits, the odd leading zero included.
        fn digits(&mut self, max: usize) -> String {
            let n = 1 + self.below(max);
            (0..n).map(|_| char::from(b'0' + u8::try_from(self.below(10)).unwrap_or(0))).collect()
        }
    }

    const SEPS: &[&str] = &["/", "-", ".", ":", " ", "/", "-", " ", "", "\t", "\u{a0}", " / "];
    const WILD: &[&str] = &[
        "1e",
        "--1",
        "1,5",
        "1.",
        ".",
        "-",
        "+",
        "1e+",
        "1e999",
        "1e-3",
        "1.2.3",
        "1.2.3.4",
        "1 2",
        "007",
        "1_000",
        "0x10",
        "١٢٣",
        "１２３",
        "1,234",
        "1,23,4",
        "12,345.67",
        "$1,200.50",
        "50%",
        "-50%",
        "$-5",
        "$ 5",
        "(100)",
        "((100))",
        "( 5 )",
        "()",
        "(1",
        "1)",
        "1 1/2",
        "0 3/4",
        "1 1/nan",
        "1/nan/2",
        "nan",
        "inf",
        "-inf",
        "NaN",
        "10:30",
        "10:30:15",
        "10:30:15.5",
        "25:00",
        "1:60",
        "5 pm",
        "5pm",
        "5 PM",
        "12:00 AM",
        "0:00am",
        "5a",
        "5p",
        "5x",
        "pm",
        "am",
        "5 a",
        "5   pm",
        "5\tpm",
        "5\u{a0}pm",
        "5\nPM",
        "-\u{a0}5",
        "3\u{a0}/4",
        "\u{a0}5",
        "5\u{3000}pm",
        "23 p",
        "5 m",
        "5:30:15.5 pm",
        "5 5 pm",
        "Alabama",
        "South Alabama",
        "Palm",
        "Costa Rica",
        "4 Mar",
        "Mar 4",
        "Mar 4, 2026",
        "Mar 4, 99",
        "4-March-2026",
        "March 4 26",
        "4-Mar-1776",
        "29-Feb-1900",
        "2026-10-07",
        "10/7/2026",
        "7/4/1776",
        "2/30/2026",
        "12/31/1899",
        "1/1/1900",
        "1904",
        "2026-03-04 18:00",
        "10/7/2026 18:00",
        "Sept 30, 2026",
        "sep 3",
        "Se 3",
        "Dece 25",
        "true",
        "TRUE",
        "false",
        "FALSE",
        "TRUEE",
        "#N/A",
        "#div/0!",
        "#REF!",
        "#ref",
        "'123",
        "'",
        "''",
        "'TRUE",
        " , ",
        ",",
        " ",
        "  ",
        "\t7\t",
        "\r\n9",
        "€5",
        "£5",
        "-€5",
        "5%5",
        "%",
        "$",
        "e",
        "E",
        "e5",
        ".5",
        "-.5",
        "+.5",
        "-0",
        "- 5",
        "+ 5",
        "1e5",
        "1E5",
        "1e+5",
        "1e-5",
        "1.e5",
        "1.5e1",
        "99999999999999999999",
        "9999999999999999999",
        "9999999999999999999999999",
        "1234567890123456789",
        "12345678901234567890",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
        "18446744073709551616",
        "0000000000000000000000001",
        "1.7976931348623157e308",
        "1.7976931348623159e308",
        "5e-324",
        "1e-400",
        "Mar",
        "MARCH",
        "march 4, 2026",
        "4 march 2026",
        "13/13/2026",
        "0/0/0",
        "1-2-3",
        "3.4.2026",
        "3-4",
        "3.4",
        "Item 0",
        "foo",
        "bar baz",
        "e-mail",
        "v1.2",
        "H2O",
        "C:\temp",
        "a\tb",
    ];

    /// One generated field: a number, a date-like string, a wild near-number or noise, sometimes
    /// padded or quoted the way a real CSV field is.
    fn field(rng: &mut Rng) -> String {
        let mut s = match rng.below(10) {
            // A number of some shape, with the pieces a real export has.
            0..=2 => {
                let mut f = String::new();
                match rng.below(4) {
                    0 => f.push('-'),
                    1 => f.push('+'),
                    2 => f.push('$'),
                    _ => {}
                }
                f.push_str(&rng.digits(6));
                if rng.below(3) == 0 {
                    f.push('.');
                    f.push_str(&rng.digits(4));
                }
                if rng.below(4) == 0 {
                    f.push(if rng.below(2) == 0 { 'e' } else { 'E' });
                    match rng.below(3) {
                        0 => f.push('-'),
                        1 => f.push('+'),
                        _ => {}
                    }
                    f.push_str(&rng.digits(3));
                }
                if rng.below(6) == 0 {
                    f.push('%');
                }
                f
            }
            // A date-like or time-like string.
            3..=5 => {
                let parts = 1 + rng.below(3);
                let mut f = String::new();
                for i in 0..parts {
                    if i > 0 {
                        f.push_str(rng.pick(SEPS));
                    }
                    match rng.below(4) {
                        0 => f.push_str(rng.pick(&MONTHS)),
                        1 => {
                            let m = rng.pick(&MONTHS);
                            f.push_str(m.get(..1 + rng.below(3)).unwrap_or(m));
                        }
                        2 => f.push_str(rng.pick(&["AM", "pm", "a", "P", "Sep", "Mre", "x"])),
                        _ => f.push_str(&rng.digits(4)),
                    }
                }
                f
            }
            // A wild near-number from the list.
            6..=7 => rng.pick(WILD).to_string(),
            // Noise over a wide alphabet, which is what most CSV text fields are.
            _ => {
                // Whitespace inside a field (a tab, a no-break space) and letters outside ASCII
                // are where a byte test and the chain can disagree, so the noise includes them.
                let alphabet: Vec<char> =
                    "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ .,;:+-*/$%()#'\"é日\t\n\u{a0}\u{3000}".chars().collect();
                let n = rng.below(9);
                (0..n).map(|_| alphabet.get(rng.below(alphabet.len())).copied().unwrap_or('?')).collect()
            }
        };
        // The padding and the text marker a field in the wild can carry.
        match rng.below(8) {
            0 => s.insert(0, ' '),
            1 => s.push(' '),
            2 => s.insert(0, '\t'),
            3 => s.insert(0, '\''),
            _ => {}
        }
        s
    }

    #[test]
    fn classified_sniffer_agrees_with_the_reference_on_generated_fields() {
        let mut rng = Rng(0x5EED_1003);
        let mut numbers = 0;
        let mut dates = 0;
        let mut text = 0;
        let mut other = 0;
        let mut compared = 0;
        for _ in 0..6000 {
            let f = field(&mut rng);
            for sys in [DateSystem::D1900, DateSystem::D1904] {
                let got = parse_input(&f, sys);
                let want = reference_parse_input(&f, sys);
                assert_eq!(got, want, "system {sys:?}, field {f:?}: format {:?} vs {:?}", got.format, want.format);
                compared += 1;
                match want.value {
                    Value::Number(n) => {
                        if (1.0..100_000.0).contains(&n) {
                            numbers += 1
                        } else {
                            dates += 1
                        }
                    }
                    Value::Text(_) => text += 1,
                    _ => other += 1,
                }
            }
        }
        // The corpus as generated (numbers 2237, text 7373, dates and times 2148, other 242), so
        // the bounds below are loose enough to survive a change in the generator but tight enough
        // that a corpus which stopped producing one of the kinds would fail instead of passing
        // vacuously.
        assert!(compared >= 12_000, "only {compared} comparisons");
        assert!(numbers >= 1000, "only {numbers} ordinary numbers");
        assert!(text >= 3000, "only {text} text fields");
        assert!(dates >= 100, "only {dates} dates, times or big numbers");
        assert!(other >= 10, "only {other} booleans, errors and blanks");
    }

    #[test]
    fn the_fast_number_agrees_with_parse_plain() {
        // Every shape the fast path takes is one parse_plain takes too, to the same value and
        // format; that is what makes skipping the chain safe.
        let mut rng = Rng(0xFA57_0021);
        let mut taken = 0;
        for _ in 0..4000 {
            let mut f = String::new();
            if rng.below(3) == 0 {
                f.push(if rng.below(2) == 0 { '-' } else { '+' });
            }
            f.push_str(&rng.digits(20));
            if rng.below(2) == 0 {
                f.push('.');
                f.push_str(&rng.digits(8));
            }
            if rng.below(3) == 0 {
                f.push(if rng.below(2) == 0 { 'e' } else { 'E' });
                if rng.below(2) == 0 {
                    f.push(if rng.below(2) == 0 { '-' } else { '+' });
                }
                f.push_str(&rng.digits(4));
            }
            let Some((n, sci)) = fast_number(&f) else {
                continue;
            };
            taken += 1;
            let (want, fmt) = parse_plain(&f).unwrap_or((f64::NAN, None));
            assert_eq!(n.to_bits(), want.to_bits(), "value of {f:?}");
            let fast_fmt = if sci { Some("0.00E+00") } else { None };
            assert_eq!(fast_fmt, fmt, "format of {f:?}");
        }
        assert!(taken >= 3000, "only {taken} fields took the fast path");
    }

    #[test]
    fn numbers_stored_as_text() {
        for (text, n) in [("123", 123.0), (" 42 ", 42.0), ("-1,000.5", -1000.5), ("$3", 3.0), ("50%", 0.5), ("1E3", 1000.0), ("(100)", -100.0)] {
            assert_eq!(number_stored_as_text(text), Some(n), "{text}");
        }
        // Dates, times, fractions, words and blanks are not flagged.
        for text in ["3/4/2026", "10:30", "1 1/2", "abc", "12abc", "", "  ", "TRUE", "#N/A", "((1))"] {
            assert_eq!(number_stored_as_text(text), None, "{text}");
        }
    }
}
